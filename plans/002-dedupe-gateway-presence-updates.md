# Plan 002: Stop sending a gateway presence update on every player change

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 6fcae79..HEAD -- src/discord/presence.rs src/player/guild_player.rs`
> If either file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: plans/001-restore-clippy-gate-and-ci.md
- **Category**: perf
- **Planned at**: commit `6fcae79`, 2026-07-27

## Why this matters

`PresenceService` implements `PlayerObserver`, so its `player_changed` runs on
**every** player state change — there are 12 such call sites in
`src/player/playback.rs` alone (enqueue, skip, stop, pause, resume, track start,
track end, queue advance). Each one currently ends in
`ShardMessenger::set_activity`, and in Serenity 0.12.5 that always sends a real
gateway Presence Update — there is no dedup or throttle in the library:

```rust
// serenity-0.12.5/src/gateway/bridge/shard_runner.rs:314-317
ShardRunnerMessage::SetActivity(activity) => {
    self.shard.set_activity(activity);
    self.shard.update_presence().await.is_ok()
},
```

Discord limits presence updates to **5 per 20 seconds per session**. A normal
track transition emits at least two (track ended, next track started); a skip
emits about three. Two skips inside 20 seconds already breaches the limit, and
the gateway responds by closing the connection, forcing a reconnect that
interrupts voice.

Worse, the payload is almost always *identical*: with the default
`BOT_ACTIVITY_CURRENT_TRACK=false`, every one of those updates re-sends the
exact same configured activity.

Separately, when `BOT_ACTIVITY_CURRENT_TRACK=true` the refresh loops over
**every** guild and calls `GuildPlayer::snapshot()` on each, which clones the
entire queue and performs an async round-trip to the Songbird audio driver —
per event, per guild — just to read one title.

After this plan: a presence update reaches the gateway only when the activity
actually changes, and computing it no longer clones queues or talks to the
audio driver.

## Current state

Files involved:

- `src/discord/presence.rs` (107 lines) — the whole `PresenceService`. This is
  where most of the change lands.
- `src/player/guild_player.rs` — `GuildPlayer`; you will add one cheap accessor
  here (lines 106–150 hold the existing accessors).

`src/discord/presence.rs:15-20` — the struct as it exists today:

```rust
pub struct PresenceService {
    shard: OnceCell<ShardMessenger>,
    players: Arc<PlayerManager>,
    configured_activity: ActivityData,
    current_track_enabled: bool,
}
```

`src/discord/presence.rs:36-77` — the methods you will change:

```rust
    pub fn initialize(&self, shard: ShardMessenger) {
        let _ = self.shard.set(shard);
        self.set_configured();
    }

    async fn refresh(&self) {
        if !self.current_track_enabled {
            self.set_configured();
            return;
        }
        let players = self.players.all().await;
        let mut active_title = None;
        for player in players {
            let snapshot = player.snapshot().await;
            if snapshot.playback_state != PlaybackState::Playing {
                continue;
            }
            let Some(current) = snapshot.current else {
                continue;
            };
            if active_title.is_some() {
                self.set_configured();
                return;
            }
            active_title = Some(current.track.title);
        }

        let activity = active_title
            .map(|title| ActivityData::listening(truncate_presence(&title)))
            .unwrap_or_else(|| self.configured_activity.clone());
        self.set_activity(activity);
    }

    fn set_configured(&self) {
        self.set_activity(self.configured_activity.clone());
    }

    fn set_activity(&self, activity: ActivityData) {
        if let Some(shard) = self.shard.get() {
            shard.set_activity(Some(activity));
        }
    }
```

The expensive call is `player.snapshot()`, defined at
`src/player/guild_player.rs:123-150`. Note what it does beyond reading state —
this is why it must not be used here:

```rust
    pub async fn snapshot(&self) -> GuildPlayerSnapshot {
        let (mut snapshot, handle) = {
            let state = self.inner.lock().await;
            let snapshot = GuildPlayerSnapshot {
                // ...
                queued: state.queue.iter().cloned().collect(),   // clones the whole queue
                // ...
            };
            // ...
        };
        if let Some(handle) = handle
            && let Ok(track_state) = handle.get_info().await     // round-trip to the audio driver
        {
            snapshot.position_seconds = Some(track_state.position.as_secs());
        }
        snapshot
    }
```

Useful facts, already verified — you do not need to re-check these:

- `serenity::all::ActivityData` derives `Clone, Debug, Serialize, PartialEq, Eq`
  (`serenity-0.12.5/src/gateway/mod.rs:73`), so you can compare two activities
  directly with `==`.
- `GuildPlayer` already imports `PlaybackState` at `src/player/guild_player.rs:12`,
  so the new accessor needs no new import there.
- `PresenceService::initialize` is called from `src/discord/handler.rs:57`
  inside the `ready` event. Keep it a **synchronous** `fn` so that call site
  does not have to change (plan 001 also edits that file).

Repo conventions that apply (from `AGENTS.md`):

- Explicit types; functions 4–20 lines; one responsibility per function.
- No `unwrap()`/`expect()` for recoverable runtime failures — this matters for
  the `std::sync::Mutex` you will add; recover from poisoning instead.
- Comments explain *why*. Reference upstream limitations where relevant — an
  exemplar of that style is `src/player/queue.rs:50-53`.

## Commands you will need

| Purpose        | Command                                     | Expected on success        |
|----------------|---------------------------------------------|----------------------------|
| Format         | `cargo fmt`                                 | exit 0                     |
| Format check   | `cargo fmt --check`                         | exit 0, no output          |
| Typecheck      | `cargo check`                               | exit 0                     |
| Lint (the gate)| `cargo clippy --all-targets -- -D warnings` | exit 0, no output          |
| Release build  | `cargo build --release`                     | exit 0                     |

## Scope

**In scope** (the only files you should modify):

- `src/discord/presence.rs`
- `src/player/guild_player.rs` (add one accessor method only)

**Out of scope** (do NOT touch, even though they look related):

- `src/player/observer.rs` — changing the observer fan-out is plan 004's job.
  Do not make `player_changed` spawn tasks or change its signature here.
- `src/discord/player_panel.rs` — it is the other `PlayerObserver`, also plan 004.
- `src/discord/handler.rs` — `initialize` must stay synchronous so line 57
  needs no change.
- `GuildPlayer::snapshot` — the panel and `/queue` legitimately need the full
  snapshot including `position_seconds`. Do not change or "optimise" it.
- Adding tests. `AGENTS.md` forbids them in this phase.

## Git workflow

- Branch: `advisor/002-dedupe-gateway-presence-updates`
- Conventional Commits, matching `git log` (e.g. `feat(player): add idle leave and queue estimates`).
- Suggested commit: `fix(discord): send presence updates only when they change`.
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Add a cheap "currently playing title" accessor to `GuildPlayer`

In `src/player/guild_player.rs`, inside the existing `impl GuildPlayer` block,
add this method immediately after `playback_state` (which ends at line 121):

```rust
    /// Presence refresh needs only the playing title; `snapshot` additionally
    /// clones the queue and round-trips to the audio driver for the position.
    pub async fn playing_title(&self) -> Option<String> {
        let state = self.inner.lock().await;
        if state.playback_state != PlaybackState::Playing {
            return None;
        }
        state
            .current
            .as_ref()
            .map(|current| current.track.track.title.clone())
    }
```

**Verify**: `cargo check` → exit 0.

### Step 2: Give `PresenceService` a last-sent activity cache

In `src/discord/presence.rs`, add the field to the struct (currently lines 15–20):

```rust
pub struct PresenceService {
    shard: OnceCell<ShardMessenger>,
    players: Arc<PlayerManager>,
    configured_activity: ActivityData,
    current_track_enabled: bool,
    last_activity: Mutex<Option<ActivityData>>,
}
```

Use `std::sync::Mutex`, **not** `tokio::sync::Mutex` — the guard is never held
across an `.await`, and a sync mutex is what lets `initialize` and
`set_activity` stay non-async. Update the imports at the top of the file: the
file currently imports `use std::sync::Arc;` (line 1) and
`use tokio::sync::OnceCell;` (line 5). Change the first to:

```rust
use std::sync::{Arc, Mutex};
```

Initialise the field in `PresenceService::new` (lines 23–34), inside the
`Arc::new(Self { ... })` literal:

```rust
            last_activity: Mutex::new(None),
```

**Verify**: `cargo check` → exit 0.

### Step 3: Dedupe in `set_activity`

Replace `set_activity` (currently lines 73–77) with:

```rust
    // Serenity sends a gateway presence update for every `set_activity` call and
    // Discord allows only 5 per 20 seconds per session, so identical activities
    // must not reach the shard.
    fn set_activity(&self, activity: ActivityData) {
        let Some(shard) = self.shard.get() else {
            return;
        };
        {
            let mut last_activity = match self.last_activity.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            if last_activity.as_ref() == Some(&activity) {
                return;
            }
            *last_activity = Some(activity.clone());
        }
        shard.set_activity(Some(activity));
    }
```

The inner block exists so the guard is dropped before `shard.set_activity`.

**Verify**: `cargo clippy --all-targets -- -D warnings` → exit 0, no output.

### Step 4: Force one presence send per `ready`

A reconnect starts a fresh Discord session, so the cache must not suppress the
first update after the bot becomes ready. Replace `initialize` (lines 36–39):

```rust
    pub fn initialize(&self, shard: ShardMessenger) {
        let _ = self.shard.set(shard);
        // A new gateway session starts with no activity, so the cache must not
        // suppress the first update after `ready`.
        self.clear_last_activity();
        self.set_configured();
    }
```

And add the helper next to `set_activity`:

```rust
    fn clear_last_activity(&self) {
        let mut last_activity = match self.last_activity.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        *last_activity = None;
    }
```

**Verify**: `cargo check` → exit 0.

### Step 5: Remove the per-guild `snapshot` from the refresh path

Replace `refresh` (currently lines 41–67) with these two methods. Keep the
`set_configured()` call in the disabled branch — with Step 3 in place it is now
a cheap no-op, and keeping it preserves the existing invariant that the
activity is always the configured one when the feature is off.

```rust
    async fn refresh(&self) {
        if !self.current_track_enabled {
            self.set_configured();
            return;
        }
        self.set_activity(self.current_track_activity().await);
    }

    /// Falls back to the configured activity unless exactly one guild is
    /// playing, because a single presence cannot represent several tracks.
    async fn current_track_activity(&self) -> ActivityData {
        let mut active_title = None;
        for player in self.players.all().await {
            let Some(title) = player.playing_title().await else {
                continue;
            };
            if active_title.is_some() {
                return self.configured_activity.clone();
            }
            active_title = Some(title);
        }

        active_title
            .map(|title| ActivityData::listening(truncate_presence(&title)))
            .unwrap_or_else(|| self.configured_activity.clone())
    }
```

`PlaybackState` is now unused in this file. Remove it from the import at
`src/discord/presence.rs:9-12`, leaving:

```rust
    player::{manager::PlayerManager, observer::PlayerObserver, track::QueuedTrack},
```

**Verify**: `cargo clippy --all-targets -- -D warnings` → exit 0, no output
(in particular no `unused_imports` warning).

### Step 6: Confirm the full gate and the scope

**Verify**:
- `cargo fmt --check` → exit 0
- `cargo clippy --all-targets -- -D warnings` → exit 0, no output
- `cargo build --release` → exit 0
- `grep -c "snapshot()" src/discord/presence.rs` → `0`
- `git status --porcelain` → only `src/discord/presence.rs` and
  `src/player/guild_player.rs` modified (a pre-existing ` M .gitignore` may also
  be present; leave it alone)

## Test plan

None. `AGENTS.md` forbids adding tests in this phase and directs validation via
"compilation, linting, logs, and manual Discord flows".

Manual Discord flow for the operator, if they can run the bot:

1. With the default `BOT_ACTIVITY_CURRENT_TRACK=false`, start the bot and run
   `/play <youtube url>`, then press **Next**/**Pause** several times quickly.
   The bot's status must stay the configured activity, and the gateway must not
   disconnect. Before this change the same burst sent one presence update per
   action.
2. Set `BOT_ACTIVITY_CURRENT_TRACK=true`, restart, and `/play` a two-track
   queue. The status should show the playing track's title, change once when
   the track changes, and revert to the configured message when playback ends.
3. Queue tracks in two guilds at once — the status must fall back to the
   configured activity.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `cargo fmt --check` exits 0
- [ ] `cargo clippy --all-targets -- -D warnings` exits 0 with no output
- [ ] `cargo build --release` exits 0
- [ ] `grep -c "snapshot()" src/discord/presence.rs` returns `0`
- [ ] `grep -q "fn playing_title" src/player/guild_player.rs` succeeds
- [ ] `grep -q "last_activity" src/discord/presence.rs` succeeds
- [ ] `grep -q "tokio::sync::Mutex" src/discord/presence.rs` fails (must use `std::sync::Mutex`)
- [ ] No files outside the in-scope list are modified (`git status --porcelain`)
- [ ] `plans/README.md` status row for 002 updated

## STOP conditions

Stop and report back (do not improvise) if:

- `src/discord/presence.rs:36-77` or `src/player/guild_player.rs:123-150` do not
  match the "Current state" excerpts.
- `ActivityData` does not compare with `==` (i.e. the compiler reports it does
  not implement `PartialEq`) — the pinned Serenity version would then differ
  from 0.12.5 and the dedup approach needs rethinking.
- Making `set_activity` dedupe would require `initialize` to become `async` —
  it must not; report instead of changing `src/discord/handler.rs`.
- Clippy demands a change to any file outside the in-scope list.

## Maintenance notes

- **Known, deliberately not fixed here**: `PresenceService` stores a single
  `ShardMessenger` in a `OnceCell` set from the `ready` handler
  (`src/discord/handler.rs:57`). Because `OnceCell::set` only succeeds once, a
  reconnect that produces a new messenger will keep the stale one, and a
  multi-shard deployment would only ever update presence on one shard. The bot
  currently runs a single shard via `client.start()`
  (`src/discord/client.rs:36`), so this is latent. If presence ever "silently
  stops updating after a reconnect", this is the first place to look.
- If a future change makes presence depend on more than the title (e.g. elapsed
  time), do **not** reach for `GuildPlayer::snapshot` — extend `playing_title`
  or add another narrow accessor, otherwise the N+1 returns.
- A reviewer should check that the `last_activity` guard is never held across an
  `.await`, which is what keeps `std::sync::Mutex` sound here.
- Plan 004 changes how `player_changed` is dispatched. It should not conflict —
  it edits `observer.rs` and `player_panel.rs`, not this file — but if both are
  in flight, land this one first.
</content>
