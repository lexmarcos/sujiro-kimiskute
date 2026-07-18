use std::sync::Arc;

use tracing::{info, warn};

use crate::{
    error::AppError,
    player::{
        guild_player::GuildPlayer,
        lifecycle::{LeaveClaim, LeaveOperation},
        manager::PlayerManager,
        observer::PlayerObserver,
    },
    voice::VoiceConnection,
};

pub struct GuildSessionService {
    voice: Arc<VoiceConnection>,
    players: Arc<PlayerManager>,
    observer: tokio::sync::OnceCell<Arc<dyn PlayerObserver>>,
}

impl GuildSessionService {
    pub fn new(voice: Arc<VoiceConnection>, players: Arc<PlayerManager>) -> Arc<Self> {
        Arc::new(Self {
            voice,
            players,
            observer: tokio::sync::OnceCell::new(),
        })
    }

    pub fn initialize_observer(&self, observer: Arc<dyn PlayerObserver>) -> bool {
        self.observer.set(observer).is_ok()
    }

    pub async fn leave(&self, player: Arc<GuildPlayer>) -> Result<LeaveResult, AppError> {
        let operation = match player.claim_leave().await {
            LeaveClaim::Ready(operation) => operation,
            LeaveClaim::AlreadyClosing => return Err(already_closing_error(player.guild_id())),
        };
        self.finalize_claimed_leave(player, operation).await
    }

    pub(crate) async fn finalize_claimed_leave(
        &self,
        player: Arc<GuildPlayer>,
        operation: LeaveOperation,
    ) -> Result<LeaveResult, AppError> {
        if let Some(abort_handle) = operation.auto_leave_abort.as_ref() {
            abort_handle.abort();
        }
        if let Some(abort_handle) = operation.idle_leave_abort.as_ref() {
            abort_handle.abort();
        }
        player
            .invalidate_play_requests(operation.session_epoch)
            .await;
        stop_track_handle(&player, &operation);

        if let Err(error) = self.voice.disconnect(player.guild_id()).await {
            if player
                .reopen_after_failed_leave(operation.session_epoch)
                .await
                && let Some(observer) = self.observer.get()
            {
                observer.player_changed(player.guild_id()).await;
            }
            return Err(error);
        }
        let removed = self
            .players
            .remove_if_same(player.guild_id(), player.instance_id())
            .await;
        if removed.is_none() {
            return Err(obsolete_player_error(player.guild_id()));
        }

        let removed_tracks = operation.removed_from_queue + usize::from(operation.track.is_some());
        info!(
            guild_id = %player.guild_id(),
            removed_tracks,
            "guild player session removed"
        );
        if let Some(observer) = self.observer.get() {
            observer.player_changed(player.guild_id()).await;
        }
        Ok(LeaveResult { removed_tracks })
    }
}

pub struct LeaveResult {
    pub removed_tracks: usize,
}

fn stop_track_handle(player: &GuildPlayer, operation: &LeaveOperation) {
    let Some(handle) = operation.handle.as_ref() else {
        return;
    };
    if let Err(source) = handle.stop() {
        warn!(
            guild_id = %player.guild_id(),
            track_id = operation.track.as_ref().map(|track| track.track.id.as_str()),
            error = %source,
            "failed to stop track while closing guild session"
        );
    }
}

fn already_closing_error(guild_id: serenity::model::id::GuildId) -> AppError {
    AppError::Voice {
        context: format!("guild {guild_id} player session is already closing"),
    }
}

fn obsolete_player_error(guild_id: serenity::model::id::GuildId) -> AppError {
    AppError::Internal {
        context: format!("guild {guild_id} player changed before leave could remove it"),
    }
}
