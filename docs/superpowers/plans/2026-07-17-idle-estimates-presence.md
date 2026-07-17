# Idle, Queue Estimate, and Presence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add queue-idle disconnect, honest queue completion estimates, and optional single-active-guild track presence without adding commands or persistence.

**Architecture:** A dedicated idle-leave service observes player state and owns an independent per-guild timer. Queue timing is derived from the authoritative current position plus known track durations. A Discord presence service observes all guild snapshots and shows a track only when exactly one guild is actively playing.

**Tech Stack:** Rust, Tokio, Serenity, Songbird

---

### Task 1: Configuration

- Add `IDLE_LEAVE_SECONDS=300`, where `0` disables it.
- Add `BOT_ACTIVITY_CURRENT_TRACK=false`.
- Document both values.

### Task 2: Idle disconnect

- Add independent idle timer state to `GuildPlayer` lifecycle.
- Schedule when connected with no current or queued tracks.
- Cancel on `/play`, enqueue, or playback restart.
- Abort during manual/automatic leave.

### Task 3: Queue estimates

- Add known remaining seconds and unknown-track count to snapshots.
- Render remaining duration and finish timestamp only when honest.

### Task 4: Dynamic presence

- Add a global presence service initialized from the ready context.
- Show current title only when exactly one guild is playing.
- Fall back to configured presence for zero or multiple active guilds.

### Task 5: Verify

- Run formatting, check, Clippy, release build.
- Commit coherent phases.
- Restart AITest and verify ready logs.