# AGENTS.md

## Task routing

- `TRIVIAL` → `trivial_worker`: mechanical, explicit changes.
- `EASY` → `easy_worker`: small, localized changes.
- `MEDIUM` → `medium_worker`: multiple files, investigation, edge cases.
- `HARD` → `hard_worker`: architecture, security, migrations, concurrency, complex debugging.

Read the full plan, classify each task, delegate it, review the result, and run final validation.

Parallelize only independent tasks that do not edit overlapping files.

Escalate when complexity increases:
`TRIVIAL → EASY → MEDIUM → HARD`.

## Code style

* Use idiomatic Rust and explicit types.
* Functions: 4–20 lines when practical. Split by responsibility.
* Files: under 500 lines.
* One responsibility per function and module.
* Prefer early returns. Avoid more than 2 levels of indentation.
* Use specific names. Avoid generic names such as `data`, `utils`, `handler`, or `Manager`.
* Do not use `unwrap()` or `expect()` for recoverable runtime failures.
* Do not duplicate logic. Extract shared behavior only when it is actually reused.
* Error messages must include the invalid value or relevant context.

## Architecture

* Keep Discord commands, voice control, playback state, and source resolution separate.
* Command modules validate input, call application services, and format responses.
* Commands must not execute `yt-dlp` or manipulate Songbird internals directly.
* Keep YouTube-specific logic inside `sources/youtube`.
* Keep playback and queue rules inside `player`.
* Keep shared dependencies inside the application state.
* Do not place business logic in `main.rs`.
* Design source resolution so Spotify can be added later without changing command handlers.
* Avoid speculative abstractions and framework-like code.

## Async and concurrency

* Do not block the Tokio runtime.
* Use `tokio::process::Command` for `yt-dlp`.
* Never execute user input through `sh -c`, `bash -c`, or another shell.
* Do not hold locks during network requests, process execution, or Discord API calls.
* Prevent duplicate playback, double queue advancement, and concurrent connection attempts.
* Limit concurrent `yt-dlp` resolutions with a semaphore.

## Dependencies

* Inject dependencies through constructors or function parameters.
* Avoid mutable global state.
* Reuse shared clients such as `reqwest::Client`.
* Wrap third-party behavior only when it isolates project-specific responsibilities.
* Keep Serenity, Songbird, and `yt-dlp` details out of unrelated modules.
* Enable only necessary crate features.

## Comments

* Write comments explaining why, not what.
* Preserve useful comments during refactors.
* Document public types and functions when their intent is not obvious.
* Reference upstream issues when code exists because of a Discord, Songbird, YouTube, or `yt-dlp` limitation.

## Tests

* Do not create unit, integration, snapshot, or mock-based tests in this phase.
* Validate changes with compilation, linting, logs, and manual Discord flows.
* Do not add test-only abstractions.

## Formatting and validation

Run after each task:

```bash
cargo fmt
cargo check
```

Before considering the project complete:

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo build --release
```

Fix errors and relevant warnings before continuing.

## Logging

* Use `tracing`.
* Prefer structured fields over interpolated log messages.
* Include guild, channel, user, track, command, and duration context when relevant.
* Never log Discord tokens, cookies, PO tokens, secrets, or complete sensitive URLs.
* Keep Discord-facing error messages short and safe.
* Log detailed internal errors separately.

## Scope

* Implement only YouTube playback in this phase.
* Use slash commands only.
* Keep state in memory.
* Do not add Spotify, databases, dashboards, prefix commands, filters, equalizers, lyrics, autoplay, persistent cache, or unrelated features.
* Prioritize working, readable code over premature optimization.
