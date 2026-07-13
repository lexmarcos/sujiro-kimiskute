use std::{
    io::ErrorKind,
    path::PathBuf,
    process::{Output, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::{process::Command, sync::Semaphore, time::timeout};
use tracing::info;

use crate::error::AppError;

pub struct YoutubeProcess {
    executable_path: PathBuf,
    extra_arguments: Vec<String>,
    execution_timeout: Duration,
    resolution_slots: Arc<Semaphore>,
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
            extra_arguments,
            execution_timeout,
            resolution_slots,
        }
    }

    pub async fn execute(&self, arguments: &[String]) -> Result<String, AppError> {
        let _permit = self
            .resolution_slots
            .acquire()
            .await
            .map_err(|_| semaphore_closed_error())?;
        let started_at = Instant::now();

        info!("yt-dlp process starting");
        let output = self.execute_with_timeout(arguments, started_at).await?;
        let status = exit_status(&output);
        log_process_finished(started_at, &status, output.stderr.len());

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
