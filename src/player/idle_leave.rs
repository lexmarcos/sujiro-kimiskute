use std::{
    sync::{Arc, Weak},
    time::Duration,
};

use async_trait::async_trait;
use serenity::model::id::GuildId;
use tokio::time::sleep;
use tracing::{error, info};

use crate::{
    error::AppError,
    player::{
        guild_player::GuildPlayer, lifecycle::IdleLeaveToken, manager::PlayerManager,
        observer::PlayerObserver, session::GuildSessionService, track::QueuedTrack,
    },
};

pub struct IdleLeaveService {
    weak_self: Weak<IdleLeaveService>,
    players: Arc<PlayerManager>,
    sessions: Arc<GuildSessionService>,
    timeout: Option<Duration>,
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
        })
    }

    pub async fn cancel_for_activity(&self, player: &GuildPlayer) -> Result<(), AppError> {
        let cancellation = player.cancel_idle_leave_for_activity().await?;
        abort_cancellation(cancellation.abort_handle);
        if cancellation.canceled {
            info!(
                guild_id = %player.guild_id(),
                "idle leave canceled by play activity"
            );
        }
        Ok(())
    }

    pub async fn refresh(&self, guild_id: GuildId) {
        let Some(player) = self.players.get(guild_id).await else {
            return;
        };
        self.cancel_timer(&player, "player state changed").await;
        let Some(timeout) = self.timeout else {
            return;
        };
        let Some(token) = player.claim_idle_leave_timer().await else {
            return;
        };

        self.schedule(player, token, timeout).await;
    }

    async fn schedule(&self, player: Arc<GuildPlayer>, token: IdleLeaveToken, timeout: Duration) {
        info!(
            guild_id = %player.guild_id(),
            timeout_seconds = timeout.as_secs(),
            "idle leave scheduled"
        );
        let weak_service = self.weak_self.clone();
        let weak_player = Arc::downgrade(&player);
        let task = tokio::spawn(async move {
            sleep(timeout).await;
            let (Some(service), Some(player)) = (weak_service.upgrade(), weak_player.upgrade())
            else {
                return;
            };
            service.expire(player, token).await;
        });
        let abort_handle = task.abort_handle();
        drop(task);

        if let Some(stale_handle) = player.install_idle_leave_abort(token, abort_handle).await {
            stale_handle.abort();
        }
    }

    async fn expire(self: &Arc<Self>, player: Arc<GuildPlayer>, token: IdleLeaveToken) {
        let Some(operation) = player.claim_idle_leave_expiration(token).await else {
            return;
        };

        info!(guild_id = %player.guild_id(), "idle leave timeout reached");
        if let Err(source) = self
            .sessions
            .finalize_claimed_leave(Arc::clone(&player), operation)
            .await
        {
            error!(
                guild_id = %player.guild_id(),
                error = %source,
                "idle leave failed"
            );
        }
    }

    async fn cancel_timer(&self, player: &GuildPlayer, reason: &'static str) {
        let cancellation = player.cancel_idle_leave_timer().await;
        abort_cancellation(cancellation.abort_handle);
        if cancellation.canceled {
            info!(guild_id = %player.guild_id(), reason, "idle leave canceled");
        }
    }
}

#[async_trait]
impl PlayerObserver for IdleLeaveService {
    async fn player_changed(&self, guild_id: GuildId) {
        self.refresh(guild_id).await;
    }

    async fn track_failed(&self, _guild_id: GuildId, _track: &QueuedTrack) {}
}

fn abort_cancellation(abort_handle: Option<tokio::task::AbortHandle>) {
    if let Some(abort_handle) = abort_handle {
        abort_handle.abort();
    }
}
