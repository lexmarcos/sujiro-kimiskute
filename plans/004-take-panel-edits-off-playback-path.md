# Plan 004: Take Discord panel edits off the playback path

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 6fcae79..HEAD -- src/discord/player_panel.rs`
> If that file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED
- **Depends on**: plans/001-restore-clippy-gate-and-ci.md, plans/002-dedupe-gateway-presence-updates.md
- **Category**: perf
- **Planned at**: commit `6fcae79`, 2026-07-27

## Why this matters

`PlayerPanelService` is a `PlayerObserver`, and its `player_changed` performs a
Discord `edit_message` HTTP request **inline, awaited**. `PlaybackService`
notifies observers at 12 points, several of which sit directly in the
queue-advance loop:

```rust
// src/player/playback.rs:242-266 — advance_claimed_queue_from
        loop {
            // ...
            match self.start_queue_track(&player, claimed).await {   // <- awaits observer
                Ok(()) => {
                    if !player.finish_advancer_after_start().await {
                        return;
                    }
                }
            // ...
            next = player.take_next_for_advancer().await;
        }
```

`start_queue_track` → `start_claimed_track` → `self.observer.player_changed(...).await`
(`src/player/playback.rs:324`). So starting each queued track blocks on a round
trip to the Discord REST API before the loop can continue. Under skip-spam this
is worse than plain latency: Serenity's HTTP client transparently sleeps to
honour 429 rate limits, so the queue advance stalls for as long as the limiter
says. Discord allows roughly 5 message edits per 5 seconds per channel, and the
panel is edited on *every* player change plus once per `PLAYER_PANEL_UPDATE_SECONDS`.

There is a second, subtler problem: nothing serialises these refreshes today.
Two `player_changed` calls arriving close together each run `refresh`
concurrently, issuing two overlapping `edit_message` calls for the same message
— so the panel can land on the older of two snapshots.

After this plan: `player_changed` returns immediately after marking the guild
dirty, a single background task per guild performs the edits, and concurrent
change events collapse into at most one extra refresh. Playback no longer waits
on Discord.

Note that no change to `src/player/observer.rs` is needed. Once the panel
observer returns promptly, the whole sequential fan-out is fast: the presence
observer is already cheap after plan 002, and `IdleLeaveService::player_changed`
only touches in-memory locks and spawns a timer.

## Current state

File involved:

- `src/discord/player_panel.rs` (845 lines) — the entire change lands here.

`src/discord/player_panel.rs:59-66` — the struct as it exists today:

```rust
pub struct PlayerPanelService {
    weak_self: Weak<PlayerPanelService>,
    http: OnceCell<Arc<Http>>,
    players: Arc<PlayerManager>,
    language: BotLanguage,
    update_interval: Option<Duration>,
    panels: Mutex<HashMap<GuildId, ActivePanel>>,
}
```

`src/discord/player_panel.rs:68-82` — the constructor, which already uses
`Arc::new_cyclic`, so `weak_self` is available for spawning:

```rust
impl PlayerPanelService {
    pub fn new(
        players: Arc<PlayerManager>,
        language: BotLanguage,
        update_interval: Option<Duration>,
    ) -> Arc<Self> {
        Arc::new_cyclic(|weak_self| Self {
            weak_self: weak_self.clone(),
            http: OnceCell::new(),
            players,
            language,
            update_interval,
            panels: Mutex::new(HashMap::new()),
        })
    }
```

`src/discord/player_panel.rs:356-361` — the observer impl you will change:

```rust
#[async_trait]
impl PlayerObserver for PlayerPanelService {
    async fn player_changed(&self, guild_id: GuildId) {
        self.refresh(guild_id).await;
    }
```

`src/discord/player_panel.rs:147-190` — `refresh`, the expensive method. **Do
not change its body.** It is called from two places and its internal
generation/revision bookkeeping is what makes stale updates safe:

```rust
    pub async fn refresh(&self, guild_id: GuildId) {
        let Some((panel, displaced_refresh)) = self.begin_refresh(guild_id).await else {
            return;
        };
        abort_refresh(displaced_refresh);
        let Some(http) = self.http.get() else {
            return;
        };
        let Some(player) = self.players.get(guild_id).await else {
            self.disable(&panel).await;
            self.remove_if_same(guild_id, panel).await;
            return;
        };
        let snapshot = player.snapshot().await;
        // ... builds the embed, then:
        if let Err(source) = panel
            .channel_id
            .edit_message(http, panel.message_id, builder)
            .await
        // ...
    }
```

The other caller of `refresh` is `register` at `src/discord/player_panel.rs:118`:

```rust
        self.refresh(guild_id).await;
```

That call must **stay awaited and synchronous** — `/play` calls `register` right
after posting its response, and the user should see the rendered panel before
the command completes. Only the observer path becomes asynchronous.

Repo conventions that apply (from `AGENTS.md`):

- "Prevent duplicate playback, double queue advancement, and concurrent
  connection attempts." — the coalescing below is the same discipline applied to
  panel edits.
- "Do not hold locks during network requests, process execution, or Discord API
  calls." The existing code is careful about this; your new lock must be
  released before `refresh` runs.
- Explicit types; functions 4–20 lines; one responsibility per function.
- Comments explain *why*. An exemplar of the generation/revision guarding style
  is `start_refresh_task` at `src/discord/player_panel.rs:204-227`.

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

- `src/discord/player_panel.rs`

**Out of scope** (do NOT touch, even though they look related):

- `src/player/observer.rs` — the sequential fan-out is fine once the panel
  observer returns promptly. Do not make `CompositePlayerObserver` spawn tasks
  or run observers concurrently; that would reorder notifications.
- `src/player/playback.rs` — do not remove or move any `player_changed` call.
  They are the correctness signal; this plan only changes what receiving one
  costs.
- `PlayerPanelService::refresh`, `begin_refresh`, `start_refresh_task`,
  `refresh_generation`, `remove_if_same`, `disable` — their bodies stay exactly
  as they are. You are adding a layer in front of `refresh`, not changing it.
- `PlayerPanelService::register` (line 88) — its `self.refresh(guild_id).await`
  on line 118 must remain a direct awaited call.
- `PlayerObserver::track_failed` in this same file (lines 362–387) — it also
  performs a Discord call (`channel_id.say`), but it only runs on playback
  failure, not on every transition. Leave it; see maintenance notes.
- Adding tests. `AGENTS.md` forbids them in this phase.

## Git workflow

- Branch: `advisor/004-take-panel-edits-off-playback-path`
- Conventional Commits, matching `git log` (e.g. `Fix player panel concurrency`
  is the most recent change to this file).
- Suggested commit: `fix(discord): coalesce player panel refreshes off the playback path`.
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Add the coalescing state

In `src/discord/player_panel.rs`, add a field to `PlayerPanelService`
(lines 59–66):

```rust
pub struct PlayerPanelService {
    weak_self: Weak<PlayerPanelService>,
    http: OnceCell<Arc<Http>>,
    players: Arc<PlayerManager>,
    language: BotLanguage,
    update_interval: Option<Duration>,
    panels: Mutex<HashMap<GuildId, ActivePanel>>,
    refresh_requests: Mutex<HashMap<GuildId, RefreshRequest>>,
}
```

Add the value type immediately after the `ActivePanel` struct (which ends at
line 57):

```rust
/// Tracks whether a guild already has a refresh task running, and whether more
/// changes arrived while it was running.
#[derive(Default)]
struct RefreshRequest {
    in_flight: bool,
    dirty: bool,
}
```

Initialise it in `new` (lines 74–81), inside the `Arc::new_cyclic` closure:

```rust
            refresh_requests: Mutex::new(HashMap::new()),
```

**Verify**: `cargo check` → exit 0.

### Step 2: Add the request/coalesce methods

Add these two methods inside `impl PlayerPanelService`, immediately after
`refresh` (which ends at line 190) and before `async fn panel`:

```rust
    /// Playback notifies observers inline, so the panel's Discord edit must not
    /// be awaited on that path. At most one refresh task runs per guild; changes
    /// arriving while it runs collapse into a single follow-up refresh.
    async fn request_refresh(&self, guild_id: GuildId) {
        {
            let mut requests = self.refresh_requests.lock().await;
            let request = requests.entry(guild_id).or_default();
            if request.in_flight {
                request.dirty = true;
                return;
            }
            request.in_flight = true;
        }

        let Some(service) = self.weak_self.upgrade() else {
            self.refresh_requests.lock().await.remove(&guild_id);
            return;
        };
        drop(tokio::spawn(async move {
            service.run_coalesced_refresh(guild_id).await;
        }));
    }

    async fn run_coalesced_refresh(self: Arc<Self>, guild_id: GuildId) {
        loop {
            self.refresh(guild_id).await;

            let mut requests = self.refresh_requests.lock().await;
            let Some(request) = requests.get_mut(&guild_id) else {
                return;
            };
            if !request.dirty {
                requests.remove(&guild_id);
                return;
            }
            request.dirty = false;
        }
    }
```

Two properties this relies on, both already true — do not change them:

- The `refresh_requests` guard is dropped before `self.refresh(...)` runs, so no
  lock is held across a Discord API call.
- `refresh` re-reads `player.snapshot()` every time, so the final iteration
  always renders the newest state. That is what makes collapsing safe.

**Verify**: `cargo check` → exit 0.

### Step 3: Route the observer through the coalescer

Change the `PlayerObserver` impl (lines 356–361) so `player_changed` no longer
awaits `refresh`:

```rust
#[async_trait]
impl PlayerObserver for PlayerPanelService {
    async fn player_changed(&self, guild_id: GuildId) {
        self.request_refresh(guild_id).await;
    }
```

Leave `track_failed` below it untouched.

**Verify**:
- `cargo fmt && cargo clippy --all-targets -- -D warnings` → exit 0, no output
- `grep -n "self.refresh(guild_id).await" src/discord/player_panel.rs` → exactly
  **two** lines: one inside `register` (~line 118) and one inside
  `run_coalesced_refresh`. The one that used to be in `player_changed` must be gone.

### Step 4: Confirm the full gate and the scope

**Verify**:
- `cargo fmt --check` → exit 0
- `cargo clippy --all-targets -- -D warnings` → exit 0, no output
- `cargo build --release` → exit 0
- `git status --porcelain` → only `src/discord/player_panel.rs` modified (a
  pre-existing ` M .gitignore` may also be present; leave it alone)

## Test plan

None. `AGENTS.md` forbids adding tests in this phase and directs validation via
"compilation, linting, logs, and manual Discord flows".

This plan changes timing-sensitive behaviour, so the manual flow matters more
than usual. For the operator:

1. **Panel still updates**: `/play <youtube url>`. The panel appears and shows
   the track. Press **Pause** → the panel's state field changes to Paused
   within a second. Press **Resume** → it changes back.
2. **Coalescing under burst**: queue 5 short tracks, then press **Next**
   rapidly 5 times. The panel must end up showing the correct final track — not
   an earlier one — and must not be left showing a track that is no longer
   playing. This is the property the `dirty` flag guarantees.
3. **Queue advance is no longer gated on Discord**: let a queue of 3 tracks play
   through. Consecutive tracks should start promptly at each transition. In the
   logs, `track playback ended` and the next `track playback started`
   (`src/player/playback.rs:318`, `:518`) should be close together rather than
   separated by a panel edit round trip.
4. **Panel teardown still works**: `/leave` (or wait for auto-leave). The panel's
   buttons must be removed and the idle message shown.
5. **No task leak**: after playback stops and the bot is idle, further `/play`
   cycles must not slow down or accumulate; the `refresh_requests` entry is
   removed when a guild goes quiet.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `cargo fmt --check` exits 0
- [ ] `cargo clippy --all-targets -- -D warnings` exits 0 with no output
- [ ] `cargo build --release` exits 0
- [ ] `grep -q "fn request_refresh" src/discord/player_panel.rs` succeeds
- [ ] `grep -q "fn run_coalesced_refresh" src/discord/player_panel.rs` succeeds
- [ ] `grep -c "self.request_refresh(guild_id).await" src/discord/player_panel.rs` returns `1`
- [ ] `grep -c "self.refresh(guild_id).await" src/discord/player_panel.rs` returns `2`
- [ ] `git diff --name-only` lists only `src/discord/player_panel.rs`
- [ ] `plans/README.md` status row for 004 updated

## STOP conditions

Stop and report back (do not improvise) if:

- `src/discord/player_panel.rs:59-66`, `:147-190`, or `:356-361` do not match
  the "Current state" excerpts.
- The borrow checker forces you to change the signature of `refresh`,
  `begin_refresh`, or `start_refresh_task`. It should not — `run_coalesced_refresh`
  owns an `Arc<Self>` and `refresh` takes `&self`.
- You conclude the fix requires editing `src/player/observer.rs` or
  `src/player/playback.rs`. It does not; report what led you there.
- Manual flow step 2 shows the panel settling on a stale track. That means the
  dirty/in-flight handshake is wrong — report rather than adding sleeps or
  retries.
- You find yourself needing to hold `refresh_requests` across `self.refresh(...)`.

## Maintenance notes

- **Deliberately left in scope for later**: `PlayerObserver::track_failed` in
  this file still performs `channel_id.say(...)` inline on the playback path
  (`src/discord/player_panel.rs:378`). It is called from
  `src/player/playback.rs:472` and `:490`, i.e. only when a track fails, so it
  does not affect steady-state playback. If failure storms ever become a
  problem, it wants the same treatment.
- The coalescing map is keyed by `GuildId` and entries are removed when a guild
  goes quiet, so it does not grow with time — but if a future change makes
  `refresh` able to hang indefinitely, an `in_flight` entry would pin that
  guild's panel forever. `refresh` currently always terminates because every
  path either returns or completes one HTTP call.
- The periodic progress refresh (`run_refresh_loop`, line 316) is a separate
  mechanism and still calls `refresh_generation` directly. It is already
  bounded to one task per panel by `start_refresh_task`; this plan does not
  change it. If both are ever unified, keep the generation checks.
- A reviewer should scrutinise: the guard is dropped before `refresh`; the
  `dirty` flag is cleared *before* the next iteration rather than after; and the
  entry is removed on the not-dirty path so the map cannot leak.
</content>
