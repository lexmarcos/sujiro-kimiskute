use std::{
    io::ErrorKind,
    path::PathBuf,
    process::{Output, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::{
    process::Command,
    select,
    sync::{Notify, Semaphore, SemaphorePermit, TryAcquireError},
    time::timeout,
};
use tracing::{info, warn};

use crate::{error::AppError, sources::youtube::extractor_args::with_required_youtube_arguments};

pub struct YoutubeProcess {
    executable_path: PathBuf,
    extra_arguments: Vec<String>,
    execution_timeout: Duration,
    resolution_slots: Arc<Semaphore>,
    /// Asks running background resolutions to stop so an interactive request can
    /// take their slot.
    preempt_background: Notify,
}

impl YoutubeProcess {
    pub fn new(
        executable_path: PathBuf,
        extra_arguments: Vec<String>,
        execution_timeout: Duration,
        resolution_slots: Arc<Semaphore>,
    ) -> Self {
        Self {
            executable_path,
            extra_arguments: with_required_youtube_arguments(extra_arguments),
            execution_timeout,
            resolution_slots,
            preempt_background: Notify::new(),
        }
    }

    pub async fn execute(&self, arguments: &[String]) -> Result<String, AppError> {
        let permit = match self.resolution_slots.try_acquire() {
            Ok(permit) => permit,
            Err(TryAcquireError::NoPermits) => self.wait_for_slot().await?,
            Err(TryAcquireError::Closed) => return Err(semaphore_closed_error()),
        };
        self.execute_with_slot(arguments, permit).await
    }

    /// Waits for a slot after asking background resolutions to release theirs.
    /// A command must not queue behind speculative work: with a single
    /// configured slot, that would add a whole yt-dlp run to its latency.
    /// Every running prefetch stops, which frees more slots than this caller
    /// needs but keeps the release path free of slot accounting.
    async fn wait_for_slot(&self) -> Result<SemaphorePermit<'_>, AppError> {
        self.preempt_background.notify_waiters();
        self.resolution_slots
            .acquire()
            .await
            .map_err(|_| semaphore_closed_error())
    }

    /// Runs yt-dlp only while no interactive request needs the slot: it never
    /// waits for one, and gives the slot up as soon as a command asks for it.
    /// Returns `Ok(None)` in both cases, leaving the caller to resolve normally
    /// when the track is actually played.
    pub async fn execute_without_waiting(
        &self,
        arguments: &[String],
    ) -> Result<Option<String>, AppError> {
        // Registered before the slot is taken so a preemption signal raised while
        // acquiring is still observed: `notify_waiters` only wakes waiters that
        // already exist.
        let preempted = self.preempt_background.notified();
        tokio::pin!(preempted);
        preempted.as_mut().enable();

        let permit = match self.resolution_slots.try_acquire() {
            Ok(permit) => permit,
            Err(TryAcquireError::NoPermits) => return Ok(None),
            Err(TryAcquireError::Closed) => return Err(semaphore_closed_error()),
        };

        // Dropping the execution future releases the slot, and `kill_on_drop`
        // stops the yt-dlp process it left running.
        select! {
            biased;
            () = &mut preempted => Ok(None),
            result = self.execute_with_slot(arguments, permit) => result.map(Some),
        }
    }

    async fn execute_with_slot(
        &self,
        arguments: &[String],
        _slot: SemaphorePermit<'_>,
    ) -> Result<String, AppError> {
        let started_at = Instant::now();

        info!("yt-dlp process starting");
        let output = self.execute_with_timeout(arguments, started_at).await?;
        let status = exit_status(&output);
        log_process_finished(started_at, &status, output.stderr.len());
        log_process_diagnostics(&output);

        if !output.status.success() {
            return Err(unsuccessful_status_error(output.status.code()));
        }

        String::from_utf8(output.stdout).map_err(|_| invalid_stdout_error())
    }

    async fn execute_with_timeout(
        &self,
        arguments: &[String],
        started_at: Instant,
    ) -> Result<Output, AppError> {
        let mut command = Command::new(&self.executable_path);
        command
            .args(&self.extra_arguments)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        match timeout(self.execution_timeout, command.output()).await {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(source)) => {
                log_process_finished(started_at, "spawn_error", 0);
                Err(process_start_error(source))
            }
            Err(_) => {
                log_process_finished(started_at, "timeout", 0);
                Err(AppError::Timeout {
                    operation: "yt-dlp resolution",
                    duration: self.execution_timeout,
                })
            }
        }
    }
}

fn exit_status(output: &Output) -> String {
    output
        .status
        .code()
        .map_or_else(|| "signal".to_owned(), |code| code.to_string())
}

fn log_process_finished(started_at: Instant, status: &str, stderr_len: usize) {
    info!(
        duration_ms = started_at.elapsed().as_secs_f64() * 1_000.0,
        status, stderr_len, "yt-dlp process finished"
    );
}

/// Enough of yt-dlp's own diagnostics to act on, without letting a verbose run
/// flood the log.
const MAX_DIAGNOSTIC_LINES: usize = 5;
const MAX_DIAGNOSTIC_CHARS: usize = 600;

/// Reports what yt-dlp complained about. A successful run still gets logged,
/// because warnings such as a missing JavaScript runtime only become visible as
/// a failure much later, when the audio driver is refused the media URL.
fn log_process_diagnostics(output: &Output) {
    let Some(diagnostics) = diagnostic_lines(&output.stderr) else {
        return;
    };
    if output.status.success() {
        info!(diagnostics, "yt-dlp reported diagnostics");
        return;
    }
    warn!(diagnostics, "yt-dlp reported diagnostics");
}

fn diagnostic_lines(stderr: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(stderr);
    let collected = text
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("WARNING:") || line.starts_with("ERROR:"))
        .take(MAX_DIAGNOSTIC_LINES)
        .map(without_url_queries)
        .collect::<Vec<String>>()
        .join(" | ");
    if collected.is_empty() {
        return None;
    }
    Some(truncated(collected))
}

/// Drops the query string of every URL in the line. yt-dlp quotes signed media
/// URLs in some messages, and those carry the PO token and signature.
fn without_url_queries(line: &str) -> String {
    let mut sanitized = String::with_capacity(line.len());
    let mut rest = line;

    while let Some(scheme_at) = rest.find("://") {
        let after_scheme = scheme_at + "://".len();
        let url_end = rest[after_scheme..]
            .find(char::is_whitespace)
            .map_or(rest.len(), |end| after_scheme + end);
        let (url, remainder) = rest.split_at(url_end);
        match url.find('?') {
            Some(query_at) => {
                sanitized.push_str(&url[..query_at]);
                sanitized.push_str("?<redacted>");
            }
            None => sanitized.push_str(url),
        }
        rest = remainder;
    }

    sanitized.push_str(rest);
    sanitized
}

fn truncated(mut value: String) -> String {
    let Some((cut, _)) = value.char_indices().nth(MAX_DIAGNOSTIC_CHARS) else {
        return value;
    };
    value.truncate(cut);
    value.push('…');
    value
}

fn process_start_error(source: std::io::Error) -> AppError {
    let context = if source.kind() == ErrorKind::NotFound {
        "configured yt-dlp executable was not found".to_owned()
    } else {
        format!("could not start yt-dlp: {source}")
    };
    AppError::YtDlp { context }
}

fn unsuccessful_status_error(status_code: Option<i32>) -> AppError {
    let context = status_code.map_or_else(
        || "yt-dlp was terminated before completing".to_owned(),
        |code| format!("yt-dlp exited with status code {code}"),
    );
    AppError::YtDlp { context }
}

fn invalid_stdout_error() -> AppError {
    AppError::YtDlp {
        context: "yt-dlp returned stdout that was not valid UTF-8".to_owned(),
    }
}

fn semaphore_closed_error() -> AppError {
    AppError::Internal {
        context: "yt-dlp resolution semaphore is closed".to_owned(),
    }
}
