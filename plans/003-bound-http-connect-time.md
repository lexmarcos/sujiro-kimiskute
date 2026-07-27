# Plan 003: Bound connect time on the shared HTTP client

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 6fcae79..HEAD -- src/state.rs`
> If that file changed since this plan was written, compare the
> "Current state" excerpt against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: MED
- **Depends on**: plans/001-restore-clippy-gate-and-ci.md
- **Category**: bug
- **Planned at**: commit `6fcae79`, 2026-07-27

## Why this matters

`AppState::build` constructs one shared `reqwest::Client` with no timeouts of
any kind. That client is handed to Songbird as the audio source transport
(`HttpRequest::new(self.http_client.clone(), stream_url)` in
`src/player/playback.rs:379`), so it is what actually fetches the YouTube CDN
audio stream for every track.

`reqwest`'s default connect timeout is *none*: it inherits the operating
system's TCP behaviour, which can hang for minutes on an unreachable or
blackholed CDN endpoint. When that happens, the bot has already transitioned
the track to `Playing` (`mark_playback_playing`, `src/player/playback.rs:417`)
and installed its `TrackEvent::Error` handler — but a stalled connect never
produces an error event, so `track_failed` never fires, the queue never
advances, and the guild sits silently on a track that will never play. The only
recovery is a user manually pressing skip or stop.

A connect timeout converts that indefinite hang into a normal track error,
which the existing recovery path already handles: `PlaybackErrorHandler`
(`src/voice/events.rs:69-97`) → `PlaybackService::track_failed`
(`src/player/playback.rs:425`) → one retry, then skip to the next track.

## Current state

File involved:

- `src/state.rs` — `AppState::build`; the client is constructed at lines 43–48
  and is the only `reqwest::Client` in the project.

`src/state.rs:1` — the current import line:

```rust
use std::sync::Arc;
```

`src/state.rs:42-49` — the code as it exists today:

```rust
    pub fn build(config: Arc<AppConfig>, songbird: Arc<Songbird>) -> Result<Arc<Self>, AppError> {
        let http_client =
            reqwest::Client::builder()
                .build()
                .map_err(|source| AppError::Internal {
                    context: format!("could not build shared HTTP client: {source}"),
                })?;
        let players = Arc::new(PlayerManager::new(config.max_queue_size)?);
```

The one consumer that makes this delicate — `src/player/playback.rs:373-380`:

```rust
    fn build_paused_track(
        self: &Arc<Self>,
        player: &GuildPlayer,
        operation: PlaybackOperation,
        stream_url: String,
    ) -> Track {
        let input = HttpRequest::new(self.http_client.clone(), stream_url);
```

This is a **long-lived streaming response**: the body is read for the entire
duration of the track. That is why only the connect phase may be bounded.

Verified API facts (reqwest 0.12.28, the version resolved in `Cargo.lock`) —
you do not need to re-check these:

- `ClientBuilder::connect_timeout(Duration)` exists and bounds only connection
  establishment (DNS + TCP + TLS). It does **not** bound response body reads.
- `ClientBuilder::timeout(Duration)` bounds the *whole* request including the
  body. Using it here would kill every track after that many seconds.
- `ClientBuilder::read_timeout(Duration)` bounds the gap between individual
  read operations.

Repo conventions that apply (from `AGENTS.md`):

- Explicit types; avoid speculative abstractions and framework-like code.
- Named constants over inline magic numbers — an exemplar is the constant block
  at the top of `src/config.rs:7-18`, and `MAX_DETAILED_TRACKS` etc. at
  `src/discord/player_panel.rs:36-41`.
- Comments explain *why*, and reference upstream limitations where relevant.

## Commands you will need

| Purpose        | Command                                     | Expected on success        |
|----------------|---------------------------------------------|----------------------------|
| Format         | `cargo fmt`                                 | exit 0                     |
| Format check   | `cargo fmt --check`                         | exit 0, no output          |
| Typecheck      | `cargo check`                               | exit 0                     |
| Lint (the gate)| `cargo clippy --all-targets -- -D warnings` | exit 0, no output          |
| Release build  | `cargo build --release`                     | exit 0                     |

## Scope

**In scope** (the only file you should modify):

- `src/state.rs`

**Out of scope** (do NOT touch, even though they look related):

- `src/config.rs` — do **not** add a new environment variable for this. The
  timeout is an internal safety bound, not an operator knob, and `AGENTS.md`
  says to avoid speculative configuration. If the operator later needs it
  tunable, that is a separate change.
- `src/player/playback.rs` — the retry and recovery path already handles the
  resulting error; it needs no change.
- `src/sources/youtube/process.rs` — `yt-dlp` execution already has its own
  timeout (`YT_DLP_TIMEOUT_SECONDS`) and is unrelated to this HTTP client.
- Adding `ClientBuilder::timeout(...)`. This would break audio playback. See
  STOP conditions.
- Adding tests. `AGENTS.md` forbids them in this phase.

## Git workflow

- Branch: `advisor/003-bound-http-connect-time`
- Conventional Commits, matching `git log`.
- Suggested commit: `fix(player): bound audio stream connect time`.
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Add the constant

At the top of `src/state.rs`, after the `use` block (which currently ends at
line 23 with `};`) and before `pub struct AppState`, add:

```rust
/// Songbird streams track audio through this client, so only the connect phase
/// may be bounded — a total request timeout would cut every track short. An
/// unbounded connect leaves a track stuck in `Playing` with no error event, so
/// the queue would never advance.
const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
```

Update the import on line 1 to bring `Duration` in:

```rust
use std::{sync::Arc, time::Duration};
```

**Verify**: `cargo check` → exit 0.

### Step 2: Apply the connect timeout

In `AppState::build`, change the client construction (lines 43–48) to:

```rust
        let http_client = reqwest::Client::builder()
            .connect_timeout(HTTP_CONNECT_TIMEOUT)
            .build()
            .map_err(|source| AppError::Internal {
                context: format!("could not build shared HTTP client: {source}"),
            })?;
```

Do not change the error message or the `map_err` shape.

**Verify**:
- `cargo fmt && cargo clippy --all-targets -- -D warnings` → exit 0, no output
- `grep -c "\.timeout(" src/state.rs` → `0` (this must be `connect_timeout`
  only; a bare `.timeout(` would break playback)
- `grep -c "connect_timeout" src/state.rs` → `1`

### Step 3: Confirm the full gate and the scope

**Verify**:
- `cargo fmt --check` → exit 0
- `cargo clippy --all-targets -- -D warnings` → exit 0, no output
- `cargo build --release` → exit 0
- `git status --porcelain` → only `src/state.rs` modified (a pre-existing
  ` M .gitignore` may also be present; leave it alone)

## Test plan

None. `AGENTS.md` forbids adding tests in this phase and directs validation via
"compilation, linting, logs, and manual Discord flows".

The behavioural check that matters most is a **regression** check — proving the
change did not break normal streaming. Manual Discord flow for the operator:

1. `/play <youtube url>` for a track longer than 30 seconds. It must play to
   completion without cutting out. This is the check that would fail if
   `.timeout()` had been used instead of `.connect_timeout()`.
2. Queue two tracks and let the first end naturally — the second must start.
3. If the operator wants to exercise the timeout itself, they can block
   outbound traffic to `*.googlevideo.com` and confirm the bot logs
   `Songbird track playback failed` (from `src/voice/events.rs:81`) within
   ~10 seconds and moves on, rather than hanging.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `cargo fmt --check` exits 0
- [ ] `cargo clippy --all-targets -- -D warnings` exits 0 with no output
- [ ] `cargo build --release` exits 0
- [ ] `grep -c "connect_timeout" src/state.rs` returns `1`
- [ ] `grep -cE "^\s*\.timeout\(" src/state.rs` returns `0`
- [ ] `grep -c "HTTP_CONNECT_TIMEOUT" src/state.rs` returns `2` (the const and its use)
- [ ] No files outside `src/state.rs` are modified (`git status --porcelain`)
- [ ] `plans/README.md` status row for 003 updated

## STOP conditions

Stop and report back (do not improvise) if:

- `src/state.rs:42-49` does not match the "Current state" excerpt.
- `ClientBuilder::connect_timeout` does not exist — the resolved `reqwest`
  version would then differ from 0.12.28 and this plan needs revisiting.
- You are tempted to add `.timeout(...)` or `.read_timeout(...)` to make
  something work. Neither belongs in this plan; report why instead.
- Adding the timeout appears to require a config change or touching
  `src/player/playback.rs`.

## Maintenance notes

- **Deliberately deferred**: `read_timeout` would also catch a connection that
  establishes and then stalls mid-stream — a real failure mode this plan does
  not cover. It is riskier, because a legitimately slow CDN or a buffering
  pause could trip it and cut a track that would otherwise have recovered.
  Choosing a safe value needs observation of real playback first. If tracks are
  observed hanging *after* audio has started, that is the follow-up.
- The 10-second value is a judgement call, not a measured one: long enough for
  TLS over a slow link, short enough that a user notices a skip rather than a
  hang. Revisit if operators report spurious failures on high-latency networks.
- If a second `reqwest::Client` is ever introduced, it must not be built with
  bare `Client::new()` — it would silently reintroduce the unbounded connect.
- A reviewer should verify the call is `connect_timeout`, not `timeout`; the
  two differ by one word and the wrong one breaks all playback after 10s.
</content>
