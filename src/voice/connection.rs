use std::sync::Arc;

use serenity::{
    cache::Cache,
    model::id::{ChannelId, GuildId, UserId},
};
use songbird::{Songbird, error::JoinError};
use tracing::{info, warn};

use crate::{
    error::{AppError, VoiceChannelIssue},
    player::{
        guild_player::GuildPlayer,
        manager::PlayerManager,
        voice_state::{VoiceConnectionClaim, VoiceOperation},
    },
};

pub struct VoiceConnection {
    songbird: Arc<Songbird>,
    players: Arc<PlayerManager>,
}

impl VoiceConnection {
    pub fn new(songbird: Arc<Songbird>, players: Arc<PlayerManager>) -> Self {
        Self { songbird, players }
    }

    pub fn user_channel(
        cache: &Cache,
        guild_id: GuildId,
        user_id: UserId,
    ) -> Result<ChannelId, AppError> {
        let guild = cache.guild(guild_id).ok_or(AppError::InvalidVoiceChannel(
            VoiceChannelIssue::Unavailable,
        ))?;
        guild
            .voice_states
            .get(&user_id)
            .and_then(|voice_state| voice_state.channel_id)
            .ok_or(AppError::InvalidVoiceChannel(
                VoiceChannelIssue::UserNotConnected,
            ))
    }

    pub async fn connect_user(
        &self,
        cache: &Cache,
        guild_id: GuildId,
        user_id: UserId,
    ) -> Result<Arc<GuildPlayer>, AppError> {
        let channel_id = Self::user_channel(cache, guild_id, user_id)?;
        self.connect(guild_id, channel_id).await
    }

    pub async fn ensure_same_channel(
        &self,
        cache: &Cache,
        guild_id: GuildId,
        user_id: UserId,
    ) -> Result<Arc<GuildPlayer>, AppError> {
        let user_channel_id = Self::user_channel(cache, guild_id, user_id)?;
        let player = self
            .players
            .get(guild_id)
            .await
            .ok_or(AppError::InvalidVoiceChannel(
                VoiceChannelIssue::DifferentChannel,
            ))?;
        let bot_channel_id = player.active_voice_channel_id().await;

        if bot_channel_id != Some(user_channel_id) {
            return Err(AppError::InvalidVoiceChannel(
                VoiceChannelIssue::DifferentChannel,
            ));
        }
        Ok(player)
    }

    pub async fn connect(
        &self,
        guild_id: GuildId,
        channel_id: ChannelId,
    ) -> Result<Arc<GuildPlayer>, AppError> {
        let player = self.players.get_or_create(guild_id).await?;
        loop {
            match player.claim_voice_connection(channel_id).await? {
                VoiceConnectionClaim::Ready => return Ok(player),
                VoiceConnectionClaim::Wait { operation_id } => {
                    player.wait_for_voice_operation(operation_id).await;
                }
                VoiceConnectionClaim::Start(operation) => {
                    return self.perform_connection(player, operation).await;
                }
            }
        }
    }

    pub async fn disconnect(&self, guild_id: GuildId) -> Result<(), AppError> {
        let Some(player) = self.players.get(guild_id).await else {
            return Ok(());
        };
        loop {
            match player.claim_voice_disconnection().await {
                VoiceConnectionClaim::Ready => return Ok(()),
                VoiceConnectionClaim::Wait { operation_id } => {
                    player.wait_for_voice_operation(operation_id).await;
                }
                VoiceConnectionClaim::Start(operation) => {
                    return self.perform_disconnection(player, operation).await;
                }
            }
        }
    }

    async fn perform_connection(
        &self,
        player: Arc<GuildPlayer>,
        operation: VoiceOperation,
    ) -> Result<Arc<GuildPlayer>, AppError> {
        let guild_id = player.guild_id();
        let join_result = self.songbird.join(guild_id, operation.channel_id).await;
        let is_current = self.player_is_current(&player).await;
        let operation_is_current = player
            .voice_connection_operation_is_current(operation)
            .await;
        if join_result.is_err() || !is_current || !operation_is_current {
            self.remove_stale_call(guild_id).await;
        }
        let succeeded = join_result.is_ok() && is_current;
        let confirmed = player
            .finish_voice_connection(operation, succeeded, is_current)
            .await;

        if join_result.is_ok() && (!confirmed || !is_current) {
            return Err(stale_operation_error("connection"));
        }
        if let Err(source) = join_result {
            return Err(voice_error("connect", source));
        }
        if !confirmed {
            return Err(stale_operation_error("connection"));
        }

        info!(
            guild_id = %guild_id,
            channel_id = %operation.channel_id,
            "voice channel connected"
        );
        Ok(player)
    }

    async fn perform_disconnection(
        &self,
        player: Arc<GuildPlayer>,
        operation: VoiceOperation,
    ) -> Result<(), AppError> {
        let guild_id = player.guild_id();
        let remove_result = self.songbird.remove(guild_id).await;
        let succeeded = matches!(&remove_result, Ok(()) | Err(JoinError::NoCall));
        let is_current = self.player_is_current(&player).await;
        let confirmed = player
            .finish_voice_disconnection(operation, succeeded, is_current)
            .await;

        if !confirmed || !is_current {
            return Err(stale_operation_error("disconnection"));
        }
        if let Err(source) = remove_result
            && !matches!(source, JoinError::NoCall)
        {
            return Err(voice_error("disconnect", source));
        }

        info!(
            guild_id = %guild_id,
            channel_id = %operation.channel_id,
            "voice channel disconnected"
        );
        Ok(())
    }

    async fn player_is_current(&self, player: &GuildPlayer) -> bool {
        self.players
            .get(player.guild_id())
            .await
            .is_some_and(|current| current.instance_id() == player.instance_id())
    }

    async fn remove_stale_call(&self, guild_id: GuildId) {
        match self.songbird.remove(guild_id).await {
            Ok(()) | Err(JoinError::NoCall) => {}
            Err(source) => warn!(
                guild_id = %guild_id,
                error = %source,
                "failed to remove stale voice call"
            ),
        }
    }
}

fn voice_error(operation: &'static str, source: JoinError) -> AppError {
    AppError::Voice {
        context: format!("could not {operation} Songbird voice call: {source}"),
    }
}

fn stale_operation_error(operation: &'static str) -> AppError {
    AppError::Internal {
        context: format!("voice {operation} result belonged to an obsolete player operation"),
    }
}
