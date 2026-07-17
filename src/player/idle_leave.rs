use std::{
    collections::HashMap,
    sync::{Arc, Weak},
    time::Duration,
};

use async_trait::async_trait;
use serenity::model::id::GuildId;
use tokio::{sync::Mutex, task::AbortHandle, time::sleep};
use tracing::{error, info};

use crate::player::{
    manager::PlayerManager, observer::PlayerObserver, session::GuildSessionService,
    track::QueuedTrack,
};

pub struct IdleLeaveService {
    weak_self: Weak<IdleLeaveService>,
    players: Arc<PlayerManager>,
    sessions: Arc<GuildSessionService>,
    timeout: Option<Duration>,
    timers: Mutex<HashMap<GuildId, IdleLeaveTimer>>,
}

struct IdleLeaveTimer {
    instance_id: u64,
    abort_handle: AbortHandle,
}

impl IdleLeaveService {
    pub fn new(
        players: Arc<PlayerManager>,
        sessions: Arc<GuildSessionService>,
        timeout: Option<Duration>,
    ) -> Arc<Self> {
        Arc::new_cyclic(|weak_self| Self {
            weak_self: weak_self.clone(),
            players,
            sessions,
            timeout,
            timers: Mutex::new(HashMap::new()),
        })
    }

    pub async fn cancel_for_activity(&self, guild_id: GuildId) {
        self.cancel_timer(guild_id, "play activity started").await;
    }

    pub async fn refresh(&self, guild_id: GuildId) {
        self.cancel_timer(guild_id, "player state changed").await;
        let Some(timeout) = self.timeout else {
            return;
        };
        let Some(player) = self.players.get(guild_id).await else {
            return;
        };
        let snapshot = player.snapshot().await;
        if snapshot.voice_channel_id.is_none()
            || snapshot.current.is_some()
            || !snapshot.queued.is_empty()
        {
            return;
        }

        let instance_id = player.instance_id();
        let weak_service = self.weak_self.clone();
        let task = tokio::spawn(async move {
            sleep(timeout).await;
            let Some(service) = weak_service.upgrade() else {
                return;
            };
            service.expire(guild_id, instance_id).await;
        });
        let abort_handle = task.abort_handle();
        drop(task);
        self.timers.lock().await.insert(
            guild_id,
            IdleLeaveTimer {
                instance_id,
                abort_handle,
            },
        );
        info!(
            guild_id = %guild_id,
            timeout_seconds = timeout.as_secs(),
            "idle leave scheduled"
        );
    }

    async fn expire(&self, guild_id: GuildId, instance_id: u64) {
        if !self.remove_matching_timer(guild_id, instance_id).await {
            return;
        }
        let Some(player) = self.players.get(guild_id).await else {
            return;
        };
        if player.instance_id() != instance_id {
            return;
        }
        let snapshot = player.snapshot().await;
        if snapshot.voice_channel_id.is_none()
            || snapshot.current.is_some()
            || !snapshot.queued.is_empty()
        {
            return;
        }

        info!(guild_id = %guild_id, "idle leave timeout reached");
        if let Err(source) = self.sessions.leave(player).await {
            error!(guild_id = %guild_id, error = %source, "idle leave failed");
        }
    }

    async fn cancel_timer(&self, guild_id: GuildId, reason: &'static str) {
        let timer = self.timers.lock().await.remove(&guild_id);
        if let Some(timer) = timer {
            timer.abort_handle.abort();
            info!(guild_id = %guild_id, reason, "idle leave canceled");
        }
    }

    async fn remove_matching_timer(&self, guild_id: GuildId, instance_id: u64) -> bool {
        let mut timers = self.timers.lock().await;
        let matches = timers
            .get(&guild_id)
            .is_some_and(|timer| timer.instance_id == instance_id);
        if matches {
            timers.remove(&guild_id);
        }
        matches
    }
}

#[async_trait]
impl PlayerObserver for IdleLeaveService {
    async fn player_changed(&self, guild_id: GuildId) {
        self.refresh(guild_id).await;
    }

    async fn track_failed(&self, _guild_id: GuildId, _track: &QueuedTrack) {}
}
