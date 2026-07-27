# Plan 001: Make `cargo clippy -- -D warnings` pass and enforce it in CI

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 6fcae79..HEAD -- src/discord/handler.rs`
> If that file changed since this plan was written, compare the
> "Current state" excerpt against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: dx
- **Planned at**: commit `6fcae79`, 2026-07-27

## Why this matters

`CLAUDE.md` / `AGENTS.md` in this repo declares a mandatory gate before the
project is considered complete:

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo build --release
```

Right now `cargo clippy -- -D warnings` **fails** at `HEAD`, so that gate is
already red and cannot catch anything new. Nothing in the repository enforces
it either — there is no `.github/` directory and no CI of any kind. Every other
plan in `plans/` uses clippy as its verification gate, so this plan must land
first or the other plans cannot tell their own changes from the pre-existing
failure.

After this plan: the gate is green, and a GitHub Actions workflow re-runs
fmt + clippy + release build on every push and pull request.

## Current state

Files involved:

- `src/discord/handler.rs` — Serenity `EventHandler`; the single clippy failure
  is in `interaction_create` (lines 61–73).
- There is **no** `.github/` directory in this repository. You will create the
  workflow file from scratch.

The exact clippy failure at `HEAD`:

```
error: this `if` can be collapsed into the outer `match`
  --> src/discord/handler.rs:67:17
   |
67 | /                 if !play_requests::dispatch(&context, &component, &self.state).await {
68 | |                     player_controls::dispatch(&context, &component, &self.state).await;
69 | |                 }
   | |_________________^
   = note: `-D clippy::collapsible-match` implied by `-D warnings`
```

The code as it exists today, `src/discord/handler.rs:61-73`:

```rust
    async fn interaction_create(&self, context: Context, interaction: Interaction) {
        match interaction {
            Interaction::Command(command) => {
                commands::dispatch(&context, &command, &self.state).await;
            }
            Interaction::Component(component) => {
                if !play_requests::dispatch(&context, &component, &self.state).await {
                    player_controls::dispatch(&context, &component, &self.state).await;
                }
            }
            _ => {}
        }
    }
```

Behaviour that must be preserved exactly: `play_requests::dispatch` returns
`bool` — `true` means it recognised and handled the component interaction. Only
when it returns `false` may `player_controls::dispatch` run. Both are `async`
and both perform Discord API calls, so both are side-effecting.

Repo conventions that apply here (from `AGENTS.md`, follow them):

- "Prefer early returns. Avoid more than 2 levels of indentation."
- "Functions: 4–20 lines when practical. Split by responsibility."
- Explicit types; no `unwrap()`/`expect()` for recoverable failures.
- Comments explain *why*, not *what*.
- An existing example of a small private helper method on the same struct is
  `DiscordEventHandler::synchronize_commands` at `src/discord/handler.rs:27-43`.
  Match that shape (private `async fn`, takes `&self`, borrows `context`).

Build dependencies (taken from the repo's `Dockerfile`, which is the only
existing record of them): building requires the system packages `libopus-dev`
and `pkg-config`. The CI workflow must install them or the `songbird`
`driver` feature will fail to build.

## Commands you will need

| Purpose        | Command                            | Expected on success            |
|----------------|------------------------------------|--------------------------------|
| Format         | `cargo fmt`                        | exit 0                         |
| Format check   | `cargo fmt --check`                | exit 0, no output              |
| Typecheck      | `cargo check`                      | exit 0                         |
| Lint (the gate)| `cargo clippy --all-targets -- -D warnings` | exit 0, no output     |
| Release build  | `cargo build --release`            | exit 0                         |

Local toolchain this plan was written against: `rustc 1.95.0`, edition 2024,
`rust-version = "1.88"` in `Cargo.toml`.

## Scope

**In scope** (the only files you should modify or create):

- `src/discord/handler.rs` (modify)
- `.github/workflows/ci.yml` (create)

**Out of scope** (do NOT touch, even though they look related):

- `src/discord/play_requests.rs` and `src/discord/player_controls.rs` — the
  dispatch functions themselves are correct; only their call site changes.
- Any `#[allow(...)]` attribute anywhere. Do not silence the lint. If you
  cannot make it pass by restructuring, that is a STOP condition.
- `Cargo.toml`, `Dockerfile`, `install.sh`, `README*.md`.
- Adding tests of any kind. `AGENTS.md` explicitly forbids unit, integration,
  snapshot, and mock-based tests in this phase. This plan has no test step by
  design.

## Git workflow

- Branch: `advisor/001-restore-clippy-gate-and-ci`
- The repo uses Conventional Commits. Recent examples from `git log`:
  `fix(discord): synchronize commands without deletion window`,
  `feat(player): add idle leave and queue estimates`, `chore: release v0.2.0`.
- Suggested commits: `fix(discord): extract component interaction dispatch`
  then `ci: enforce fmt, clippy, and release build`.
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Extract the component dispatch into a helper method

In `src/discord/handler.rs`, add a private async method on
`impl DiscordEventHandler` (the same `impl` block that already contains
`new` and `synchronize_commands`, lines 19–44). Place it after
`synchronize_commands`:

```rust
    /// `play_requests::dispatch` reports whether it owned the interaction, so
    /// player controls must only run for component IDs it did not recognise.
    async fn dispatch_component(
        &self,
        context: &Context,
        component: &ComponentInteraction,
    ) {
        if play_requests::dispatch(context, component, &self.state).await {
            return;
        }
        player_controls::dispatch(context, component, &self.state).await;
    }
```

This requires importing `ComponentInteraction`. The file's existing import is
at `src/discord/handler.rs:3-6`:

```rust
use serenity::{
    all::{Context, EventHandler, GuildId, Interaction, Ready, VoiceState},
    async_trait,
};
```

Add `ComponentInteraction` to that `all::{...}` list, keeping it alphabetically
ordered (it sorts before `Context`).

Then replace the match arm at `src/discord/handler.rs:66-70` so the arm body is
a single call:

```rust
            Interaction::Component(component) => {
                self.dispatch_component(&context, &component).await;
            }
```

Why this shape rather than clippy's own suggestion: clippy proposes folding the
condition into a match guard (`Interaction::Component(component) if !play_requests::dispatch(...).await => {`).
That does compile, but it buries two side-effecting Discord API calls inside a
pattern-match guard, where a later reordering or added arm would silently change
dispatch behaviour. Extraction satisfies the lint, uses the early return that
`AGENTS.md` asks for, and reduces nesting. **Do not use the match-guard form.**

**Verify**: `cargo fmt && cargo clippy --all-targets -- -D warnings` → exit 0,
no warnings and no errors printed.

### Step 2: Confirm the full mandated gate is green

Run the three commands `AGENTS.md` lists, in order.

**Verify**:
- `cargo fmt --check` → exit 0, no output
- `cargo clippy --all-targets -- -D warnings` → exit 0, no output
- `cargo build --release` → exit 0, ends with a `Finished \`release\` profile` line

### Step 3: Add the CI workflow

Create `.github/workflows/ci.yml` with exactly this content:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

env:
  CARGO_TERM_COLOR: always

jobs:
  check:
    name: fmt, clippy, release build
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      # songbird's `driver` feature links against libopus; see Dockerfile.
      - name: Install system dependencies
        run: |
          sudo apt-get update
          sudo apt-get install --yes --no-install-recommends libopus-dev pkg-config

      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy

      - uses: Swatinem/rust-cache@v2

      - name: Check formatting
        run: cargo fmt --check

      - name: Lint
        run: cargo clippy --all-targets -- -D warnings

      - name: Release build
        run: cargo build --locked --release
```

**Verify**: `test -f .github/workflows/ci.yml && echo present` → prints `present`.
The workflow itself only runs on GitHub; you cannot execute it locally. Confirm
instead that every command it runs already passes locally — that is Step 2, which
you have done.

### Step 4: Confirm nothing else was modified

**Verify**: `git status --porcelain` → exactly two entries, and no others:
```
 M src/discord/handler.rs
?? .github/
```
(A pre-existing ` M .gitignore` may also be present — it was already modified
before this plan started. Leave it alone.)

## Test plan

None. `AGENTS.md` forbids adding tests in this phase and instructs validating
changes "with compilation, linting, logs, and manual Discord flows" instead.

The behavioural check, if the operator can run the bot, is a manual Discord flow:
1. Run `/play <any youtube url>` so a player panel appears.
2. Click the panel's **Pause** button → playback pauses and an ephemeral
   "Playback paused" reply appears. This exercises `player_controls::dispatch`
   (the `false` branch of the extracted helper).
3. Run `/play <a youtube playlist url>` and click **Cancel** on the loading
   message → "Playlist canceled". This exercises `play_requests::dispatch`
   (the `true` branch, which must stop before player controls run).

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `cargo fmt --check` exits 0
- [ ] `cargo clippy --all-targets -- -D warnings` exits 0 with no output
- [ ] `cargo build --release` exits 0
- [ ] `grep -c "allow(clippy" src/discord/handler.rs` returns `0`
- [ ] `grep -q "fn dispatch_component" src/discord/handler.rs` succeeds
- [ ] `.github/workflows/ci.yml` exists and contains `-D warnings`
- [ ] `git status --porcelain` shows no modified files outside the in-scope list
- [ ] `plans/README.md` status row for 001 updated

## STOP conditions

Stop and report back (do not improvise) if:

- `src/discord/handler.rs:61-73` does not match the "Current state" excerpt.
- After Step 1, `cargo clippy --all-targets -- -D warnings` reports a *different*
  lint than `collapsible_match` — that means the toolchain differs from the one
  this plan was written against (rustc 1.95.0) and new lints are firing. Report
  the full list rather than fixing them ad hoc.
- The only way you can make clippy pass is by adding an `#[allow(...)]`.
- `cargo build --release` fails with a linker error mentioning `opus` — your
  environment is missing `libopus-dev`; report it rather than changing
  `Cargo.toml` features.

## Maintenance notes

- The workflow uses `dtolnay/rust-toolchain@stable`, so a future stable Rust
  release can introduce new clippy lints and turn CI red without any code
  change. That is the deliberate trade-off: it matches what a developer running
  the `AGENTS.md` gate locally will see. If that churn becomes annoying, pin the
  action to a specific version (`dtolnay/rust-toolchain@1.95.0`) and bump it
  deliberately — but then the local gate and CI can disagree.
- `Cargo.toml` declares `rust-version = "1.88"`, which CI does not currently
  verify. If honouring that MSRV matters, add a second job pinned to 1.88 that
  runs `cargo check` only.
- A reviewer should confirm the extracted helper preserved the early-return
  semantics: `play_requests::dispatch` returning `true` must prevent
  `player_controls::dispatch` from running at all.
</content>
</invoke>
