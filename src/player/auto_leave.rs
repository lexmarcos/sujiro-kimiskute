use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use serenity::{
    all::Cache,
    model::id::{ChannelId, GuildId},
};
use tokio::time::sleep;
use tracing::{error, info};

use crate::{
    error::AppError,
    player::{
        guild_player::GuildPlayer, lifecycle::AutoLeaveToken, manager::PlayerManager,
        session::GuildSessionService,
    },
};

pub struct AutoLeaveService {
    players: Arc<PlayerManager>,
    sessions: Arc<GuildSessionService>,
    timeout: Duration,
}

impl AutoLeaveService {
    pub fn new(
        players: Arc<PlayerManager>,
        sessions: Arc<GuildSessionService>,
        timeout: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            players,
            sessions,
            timeout,
        })
    }

    pub async fn refresh(self: &Arc<Self>, cache: Arc<Cache>, guild_id: GuildId) {
        let Some(player) = self.players.get(guild_id).await else {
            return;
        };
        let Some(channel_id) = player.active_voice_channel_id().await else {
            self.cancel_timer(&player, "voice connection is inactive")
                .await;
            return;
        };

        if channel_has_humans(&cache, guild_id, channel_id) {
            self.cancel_timer(&player, "a human is present").await;
            return;
        }
        self.schedule(cache, player, channel_id).await;
    }

    pub async fn cancel_for_activity(&self, player: &GuildPlayer) -> Result<(), AppError> {
        let cancellation = player.cancel_auto_leave_for_activity().await?;
        if let Some(abort_handle) = cancellation.abort_handle {
            abort_handle.abort();
        }
        if cancellation.canceled {
            info!(
                guild_id = %player.guild_id(),
                "auto-leave canceled by play activity"
            );
        }
        Ok(())
    }

    async fn schedule(
        self: &Arc<Self>,
        cache: Arc<Cache>,
        player: Arc<GuildPlayer>,
        channel_id: ChannelId,
    ) {
        let Some(token) = player.claim_auto_leave_timer(channel_id).await else {
            return;
        };
        info!(
            guild_id = %player.guild_id(),
            channel_id = %channel_id,
            timeout_seconds = self.timeout.as_secs(),
            "auto-leave scheduled"
        );

        let weak_service = Arc::downgrade(self);
        let weak_player = Arc::downgrade(&player);
        let timeout = self.timeout;
        let task = tokio::spawn(async move {
            sleep(timeout).await;
            let (Some(service), Some(player)) = (weak_service.upgrade(), weak_player.upgrade())
            else {
                return;
            };
            service.expire(cache, player, token).await;
        });
        let abort_handle = task.abort_handle();
        drop(task);

        if let Some(stale_handle) = player.install_auto_leave_abort(token, abort_handle).await {
            stale_handle.abort();
        }
    }

    async fn cancel_timer(&self, player: &GuildPlayer, reason: &'static str) {
        let cancellation = player.cancel_auto_leave_timer().await;
        if let Some(abort_handle) = cancellation.abort_handle {
            abort_handle.abort();
        }
        if cancellation.canceled {
            info!(
                guild_id = %player.guild_id(),
                reason,
                "auto-leave canceled"
            );
        }
    }

    async fn expire(
        self: &Arc<Self>,
        cache: Arc<Cache>,
        player: Arc<GuildPlayer>,
        token: AutoLeaveToken,
    ) {
        if channel_has_humans(&cache, player.guild_id(), token.channel_id) {
            if player.discard_auto_leave_token(token).await {
                info!(
                    guild_id = %player.guild_id(),
                    channel_id = %token.channel_id,
                    "auto-leave canceled after occupancy revalidation"
                );
            }
            return;
        }
        let Some(operation) = player.claim_auto_leave_expiration(token).await else {
            return;
        };

        info!(
            guild_id = %player.guild_id(),
            channel_id = %token.channel_id,
            timeout_seconds = self.timeout.as_secs(),
            "auto-leave timeout reached"
        );
        if let Err(source) = self
            .sessions
            .finalize_claimed_leave(Arc::clone(&player), operation)
            .await
        {
            error!(
                guild_id = %player.guild_id(),
                channel_id = %token.channel_id,
                error = %source,
                "auto-leave failed"
            );
            self.retry_after_failure(cache, player.guild_id());
        }
    }

    fn retry_after_failure(self: &Arc<Self>, cache: Arc<Cache>, guild_id: GuildId) {
        let service = Arc::clone(self);
        let retry: Pin<Box<dyn Future<Output = ()> + Send>> = Box::pin(async move {
            service.refresh(cache, guild_id).await;
        });
        drop(tokio::spawn(retry));
    }
}

fn channel_has_humans(cache: &Cache, guild_id: GuildId, channel_id: ChannelId) -> bool {
    let bot_user_id = cache.current_user().id;
    let Some(guild) = cache.guild(guild_id) else {
        return true;
    };

    guild.voice_states.iter().any(|(user_id, voice_state)| {
        if voice_state.channel_id != Some(channel_id) || *user_id == bot_user_id {
            return false;
        }
        guild
            .members
            .get(user_id)
            .map(|member| !member.user.bot)
            .or_else(|| voice_state.member.as_ref().map(|member| !member.user.bot))
            .unwrap_or(true)
    })
}
