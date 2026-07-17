# Media Player Progress Design

## Context

Sujiro already keeps one live Discord player panel with native playback controls. Add useful current-playback information without adding commands, persistence, or a dashboard. The panel should feel like a lightweight media player while remaining accurate and low-overhead.

## Design

### Authoritative playback position

Use Songbird's existing `TrackHandle::get_info()` to obtain the current `TrackState.position`. Do not maintain a second clock from wall time, pause durations, or estimated starts; Songbird already accounts for pause, seek, preparation, and playback timing.

Extend `GuildPlayerSnapshot` with an optional playback position. `GuildPlayer::snapshot()` copies the current handle while holding the player lock, releases the lock, then awaits `get_info()`. A failed or stale handle lookup leaves the position unknown and logs at debug/warn level without breaking the panel.

### Panel presentation

For a playing or paused track with known duration, add a localized progress field:

```text
━━━━●─────── 1:24 / 3:42
Ends <t:...:R>
```

Use a fixed 12-segment bar so the embed remains compact. Clamp position to duration. When paused, omit the projected end timestamp because it would be misleading. When playing, calculate the end timestamp from the remaining duration and render Discord's relative timestamp so the text continues updating client-side between panel edits.

If duration is unknown, display elapsed time only. If Songbird position is temporarily unavailable, retain the duration field and omit progress rather than inventing a value.

### Refresh lifecycle

`PlayerPanelService` owns refresh-task lifecycle because it already owns the active guild→panel mapping. Each active panel may have at most one refresh task.

- Start a task after panel registration or player-change notification when the snapshot is `Playing` and has a current track.
- Refresh every 15 seconds.
- Before every edit, confirm the panel generation still matches the task and playback remains active.
- Stop when paused, idle, disconnected, the guild player is removed, the panel is replaced, the message edit fails, or the service is shut down.
- Resume/new-track notifications create a fresh task only after aborting/replacing the previous one.

Store a monotonically increasing generation and optional `AbortHandle` with each panel entry. The task itself holds only weak service ownership to avoid keeping the application alive.

### Concurrency and failures

Never hold the panel registry lock during Songbird state queries or Discord API calls. Copy the panel identity/generation under the lock, release it, gather the snapshot, then edit. On completion, re-check identity before scheduling another task.

Panel refresh failures remove the matching panel and abort its timer. Songbird `get_info()` failure only removes progress for that render; it does not affect playback.

## Verification

Run:

```bash
CARGO_BUILD_JOBS=2 cargo fmt --check
CARGO_BUILD_JOBS=2 cargo check
CARGO_BUILD_JOBS=2 cargo clippy -- -D warnings
CARGO_BUILD_JOBS=2 cargo build --release
```

Manual Discord checks:

1. Start a known-duration track: progress and elapsed/total appear and advance after roughly 15 seconds.
2. Pause: position freezes, panel updates immediately, and the projected end timestamp disappears.
3. Resume: periodic updates restart and projected end time is recalculated.
4. Seeked/timestamped link: initial position reflects the seek.
5. Skip/previous/new track: progress resets to the authoritative new position; no old timer edits the panel.
6. `/queue` replaces the compact panel: old panel remains disabled and only the new panel updates.
7. Stop/track end/leave: periodic edits cease.
8. Unknown duration or transient Songbird info failure: panel stays usable and displays no false progress.