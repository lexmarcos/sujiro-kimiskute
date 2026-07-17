# Media Player Progress Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Show accurate playback progress and keep the active Discord panel refreshed while music is playing.

**Architecture:** Read authoritative position from Songbird through the current track handle. Render progress in the existing panel, and let `PlayerPanelService` own one generation-guarded refresh task per guild.

**Tech Stack:** Rust, Tokio, Serenity, Songbird

---

### Task 1: Expose authoritative position

**Files:** `src/player/guild_player.rs`

- [ ] Copy the current track handle under the player lock, release the lock, then call `get_info()`.
- [ ] Add `position_seconds: Option<u64>` to `GuildPlayerSnapshot`.
- [ ] Keep playback usable when Songbird timing is unavailable.

### Task 2: Render progress

**Files:** `src/discord/player_panel.rs`

- [ ] Add a 12-segment progress bar and elapsed/total formatter.
- [ ] Show a Discord relative end timestamp only while playing with known duration.
- [ ] Show elapsed-only output for unknown duration.

### Task 3: Refresh while playing

**Files:** `src/discord/player_panel.rs`

- [ ] Store a generation and optional refresh `AbortHandle` per active panel.
- [ ] Replace/abort the previous timer whenever panel or playback state changes.
- [ ] Refresh every 15 seconds only while the matching panel is playing.
- [ ] Stop on pause, idle, panel replacement, removal, or edit failure.

### Task 4: Verify and run

- [ ] Run `CARGO_BUILD_JOBS=2 cargo fmt --check`.
- [ ] Run `CARGO_BUILD_JOBS=2 cargo check`.
- [ ] Run `CARGO_BUILD_JOBS=2 cargo clippy -- -D warnings`.
- [ ] Run `CARGO_BUILD_JOBS=2 cargo build --release`.
- [ ] Commit the feature.
- [ ] Restart the improvements bot and verify Discord-ready logs.