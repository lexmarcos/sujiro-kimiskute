use std::{sync::Arc, time::Duration};

use reqwest::Client;
use songbird::{
    Songbird,
    events::{Event, EventData, TrackEvent},
    input::HttpRequest,
    tracks::{Track, TrackHandle},
};
use tracing::{info, warn};

use crate::{
    error::AppError,
    player::{
        guild_player::GuildPlayer,
        manager::PlayerManager,
        observer::PlayerObserver,
        playback_state::{
            ClaimedPlayback, PlaybackControl, PlaybackControlClaim, PlaybackOperation,
            PlaybackSkipClaim, PlaybackState,
        },
        queue::QueueInsertionReceipt,
        track::QueuedTrack,
    },
    sources::TrackResolver,
    voice::events::{PlaybackEndHandler, PlaybackErrorHandler},
};

mod previous;

pub use previous::PlaybackPreviousResult;

pub struct PlaybackService {
    resolver: Arc<dyn TrackResolver>,
    http_client: Client,
    songbird: Arc<Songbird>,
    players: Arc<PlayerManager>,
    observer: Arc<dyn PlayerObserver>,
}

impl PlaybackService {
    pub fn new(
        resolver: Arc<dyn TrackResolver>,
        http_client: Client,
        songbird: Arc<Songbird>,
        players: Arc<PlayerManager>,
        observer: Arc<dyn PlayerObserver>,
    ) -> Arc<Self> {
        Arc::new(Self {
            resolver,
            http_client,
            songbird,
            players,
            observer,
        })
    }

    pub async fn play_single(
        self: &Arc<Self>,
        player: Arc<GuildPlayer>,
        track: QueuedTrack,
    ) -> Result<(), AppError> {
        self.validate_player(&player).await?;
        let resolved_track = track.track.clone();
        let operation = player.claim_playback_start(track).await?;
        self.start_claimed_track(&player, operation, &resolved_track)
            .await
    }

    pub async fn enqueue(
        self: &Arc<Self>,
        player: Arc<GuildPlayer>,
        track: QueuedTrack,
    ) -> Result<usize, AppError> {
        self.validate_player(&player).await?;
        let (position, claimed_advancer) = player.enqueue_for_playback(track).await?;
        if claimed_advancer {
            self.advance_claimed_queue(Arc::clone(&player)).await;
        }
        self.observer.player_changed(player.guild_id()).await;
        Ok(position)
    }

    pub async fn enqueue_prefix(
        self: &Arc<Self>,
        player: Arc<GuildPlayer>,
        tracks: Vec<QueuedTrack>,
        expected_session_epoch: u64,
    ) -> Result<QueueInsertionReceipt, AppError> {
        self.validate_player(&player).await?;
        let (receipt, claimed_advancer) = player
            .enqueue_prefix_for_playback(tracks, expected_session_epoch)
            .await?;
        if claimed_advancer {
            self.advance_claimed_queue(Arc::clone(&player)).await;
        }
        self.observer.player_changed(player.guild_id()).await;
        Ok(receipt)
    }

    pub async fn ensure_queue_advancing(self: &Arc<Self>, player: Arc<GuildPlayer>) {
        if player.claim_queue_advancer().await {
            self.advance_claimed_queue(player).await;
        }
    }

    pub async fn pause(&self, player: &GuildPlayer) -> Result<PlaybackControlResult, AppError> {
        self.apply_control(player, PlaybackControl::Pause).await
    }

    pub async fn resume(&self, player: &GuildPlayer) -> Result<PlaybackControlResult, AppError> {
        self.apply_control(player, PlaybackControl::Resume).await
    }

    pub async fn skip(
        self: &Arc<Self>,
        player: Arc<GuildPlayer>,
    ) -> Result<PlaybackSkipResult, AppError> {
        self.validate_player(&player).await?;
        let skipped = match player.claim_skip().await? {
            PlaybackSkipClaim::NoTrack => return Ok(PlaybackSkipResult::NoTrack),
            PlaybackSkipClaim::NoNext => return Ok(PlaybackSkipResult::NoNext),
            PlaybackSkipClaim::Ready(skipped) => skipped,
        };

        if let Some(handle) = skipped.handle
            && let Err(source) = handle.stop()
        {
            warn!(
                guild_id = %player.guild_id(),
                track_id = %skipped.track.track.id,
                playback_id = skipped.operation.playback_id,
                error = %source,
                "failed to stop skipped track handle"
            );
        }
        if skipped.claimed_advancer {
            let next = player.take_next_for_advancer().await;
            let playback = Arc::clone(self);
            let advancing_player = Arc::clone(&player);
            tokio::spawn(async move {
                playback
                    .advance_claimed_queue_from(advancing_player, next)
                    .await;
            });
        }

        info!(
            guild_id = %player.guild_id(),
            track_id = %skipped.track.track.id,
            playback_id = skipped.operation.playback_id,
            "track skipped"
        );
        self.observer.player_changed(player.guild_id()).await;
        Ok(PlaybackSkipResult::Skipped {
            track: skipped.track,
        })
    }

    pub async fn stop(&self, player: &GuildPlayer) -> Result<PlaybackStopResult, AppError> {
        self.validate_player(player).await?;
        let stopped = player.claim_stop().await?;
        player.invalidate_play_requests(stopped.session_epoch).await;
        let track_id = stopped.track.as_ref().map(|track| track.track.id.clone());
        let removed_tracks = stopped.removed_from_queue + usize::from(stopped.track.is_some());

        if let Some(handle) = stopped.handle
            && let Err(source) = handle.stop()
        {
            warn!(
                guild_id = %player.guild_id(),
                track_id,
                error = %source,
                "failed to stop interrupted track handle"
            );
        }
        info!(
            guild_id = %player.guild_id(),
            removed_tracks,
            removed_from_queue = stopped.removed_from_queue,
            "playback stopped and queue cleared"
        );
        self.observer.player_changed(player.guild_id()).await;

        Ok(PlaybackStopResult { removed_tracks })
    }

    async fn apply_control(
        &self,
        player: &GuildPlayer,
        control: PlaybackControl,
    ) -> Result<PlaybackControlResult, AppError> {
        self.validate_player(player).await?;
        match player.claim_playback_control(control).await? {
            PlaybackControlClaim::NoTrack => Ok(PlaybackControlResult::NoTrack),
            PlaybackControlClaim::AlreadyPaused => Ok(PlaybackControlResult::AlreadyPaused),
            PlaybackControlClaim::AlreadyPlaying => Ok(PlaybackControlResult::AlreadyPlaying),
            PlaybackControlClaim::Ready { handle, operation } => {
                self.apply_ready_control(player, control, handle, operation)
                    .await
            }
        }
    }

    async fn apply_ready_control(
        &self,
        player: &GuildPlayer,
        control: PlaybackControl,
        handle: TrackHandle,
        operation: PlaybackOperation,
    ) -> Result<PlaybackControlResult, AppError> {
        let (expected_state, new_state, operation_name) = control_transition(control);
        control_track(&handle, control)?;
        if !player
            .confirm_playback_control(operation, expected_state, new_state)
            .await
        {
            stop_created_handle(&handle);
            return Err(stale_playback_error());
        }

        info!(
            guild_id = %player.guild_id(),
            playback_id = operation.playback_id,
            operation = operation_name,
            "track playback state changed"
        );
        self.observer.player_changed(player.guild_id()).await;
        Ok(PlaybackControlResult::Changed)
    }

    async fn advance_claimed_queue(self: &Arc<Self>, player: Arc<GuildPlayer>) {
        let next = player.take_next_for_advancer().await;
        self.advance_claimed_queue_from(player, next).await;
    }

    async fn advance_claimed_queue_from(
        self: &Arc<Self>,
        player: Arc<GuildPlayer>,
        mut next: Option<ClaimedPlayback>,
    ) {
        loop {
            let Some(claimed) = next.take() else {
                return;
            };
            let track_id = claimed.track.track.id.clone();
            let operation = claimed.operation;

            match self.start_queue_track(&player, claimed).await {
                Ok(()) => {
                    if !player.finish_advancer_after_start().await {
                        return;
                    }
                }
                Err(error) => warn!(
                    guild_id = %player.guild_id(),
                    track_id,
                    playback_id = operation.playback_id,
                    error = %error,
                    "queued track failed; advancing queue"
                ),
            }
            next = player.take_next_for_advancer().await;
        }
    }

    async fn start_queue_track(
        self: &Arc<Self>,
        player: &Arc<GuildPlayer>,
        claimed: ClaimedPlayback,
    ) -> Result<(), AppError> {
        self.start_claimed_track(player, claimed.operation, &claimed.track.track)
            .await
    }

    async fn start_claimed_track(
        self: &Arc<Self>,
        player: &Arc<GuildPlayer>,
        operation: PlaybackOperation,
        resolved_track: &crate::player::track::ResolvedTrack,
    ) -> Result<(), AppError> {
        let stream_url = match self.resolver.prepare_stream(resolved_track).await {
            Ok(url) => url,
            Err(error) => {
                player.clear_playback(operation).await;
                return Err(error);
            }
        };

        let handle = match self
            .install_paused_track(player, operation, stream_url)
            .await
        {
            Ok(handle) => handle,
            Err(error) => {
                player.clear_playback(operation).await;
                return Err(error);
            }
        };
        if let Some(start_at_seconds) = resolved_track.start_at_seconds
            && let Err(source) = handle
                .seek_async(Duration::from_secs(start_at_seconds))
                .await
        {
            player.clear_playback(operation).await;
            stop_created_handle(&handle);
            return Err(AppError::Voice {
                context: format!(
                    "could not seek track {} to {start_at_seconds} seconds: {source}",
                    resolved_track.id
                ),
            });
        }
        self.start_installed_track(player, operation, &handle)
            .await?;

        info!(
            guild_id = %player.guild_id(),
            track_id = %resolved_track.id,
            playback_id = operation.playback_id,
            "track playback started"
        );
        self.observer.player_changed(player.guild_id()).await;
        Ok(())
    }

    async fn install_paused_track(
        self: &Arc<Self>,
        player: &Arc<GuildPlayer>,
        operation: PlaybackOperation,
        stream_url: String,
    ) -> Result<TrackHandle, AppError> {
        let songbird_track = self.build_paused_track(player, operation, stream_url);
        let call = self
            .songbird
            .get(player.guild_id())
            .ok_or(AppError::Voice {
                context: format!("guild {} has no Songbird call", player.guild_id()),
            })?;
        let handle = call.lock().await.play(songbird_track);
        let is_current = self.player_is_current(player).await;
        let installed = player
            .install_playback_handle(operation, handle.clone(), is_current)
            .await;

        if !installed {
            stop_created_handle(&handle);
            return Err(stale_playback_error());
        }
        Ok(handle)
    }

    fn build_paused_track(
        self: &Arc<Self>,
        player: &GuildPlayer,
        operation: PlaybackOperation,
        stream_url: String,
    ) -> Track {
        let input = HttpRequest::new(self.http_client.clone(), stream_url);
        let mut track = Track::from(input).pause();
        let handler = PlaybackEndHandler::new(
            Arc::downgrade(self),
            player.guild_id(),
            player.instance_id(),
            operation,
        );
        track.events.add_event(
            EventData::new(Event::Track(TrackEvent::End), handler),
            Duration::ZERO,
        );
        track.events.add_event(
            EventData::new(
                Event::Track(TrackEvent::Error),
                PlaybackErrorHandler::new(player.guild_id(), player.instance_id(), operation),
            ),
            Duration::ZERO,
        );
        track
    }

    async fn start_installed_track(
        &self,
        player: &GuildPlayer,
        operation: PlaybackOperation,
        handle: &TrackHandle,
    ) -> Result<(), AppError> {
        if let Err(source) = handle.play() {
            player.clear_playback(operation).await;
            stop_created_handle(handle);
            return Err(track_control_error("start", source));
        }
        if player.mark_playback_playing(operation).await {
            return Ok(());
        }

        stop_created_handle(handle);
        Err(stale_playback_error())
    }

    pub(crate) async fn track_ended(
        self: &Arc<Self>,
        guild_id: serenity::model::id::GuildId,
        instance_id: u64,
        operation: PlaybackOperation,
    ) {
        let Some(player) = self.players.get(guild_id).await else {
            return;
        };
        if player.instance_id() != instance_id {
            return;
        }
        let Some((track, claimed_advancer)) =
            player.finish_playback_and_claim_advancer(operation).await
        else {
            return;
        };

        info!(
            guild_id = %guild_id,
            track_id = %track.track.id,
            playback_id = operation.playback_id,
            "track playback ended"
        );
        self.observer.player_changed(guild_id).await;

        if claimed_advancer {
            let playback = Arc::clone(self);
            tokio::spawn(async move {
                playback.advance_claimed_queue(player).await;
            });
        }
    }

    async fn validate_player(&self, player: &GuildPlayer) -> Result<(), AppError> {
        if !self.player_is_current(player).await {
            return Err(stale_playback_error());
        }
        if player.voice_channel_id().await.is_none() {
            return Err(AppError::Voice {
                context: format!("guild {} is not connected to voice", player.guild_id()),
            });
        }
        Ok(())
    }

    async fn player_is_current(&self, player: &GuildPlayer) -> bool {
        self.players
            .get(player.guild_id())
            .await
            .is_some_and(|current| current.instance_id() == player.instance_id())
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum PlaybackControlResult {
    NoTrack,
    AlreadyPaused,
    AlreadyPlaying,
    Changed,
}

pub enum PlaybackSkipResult {
    NoTrack,
    NoNext,
    Skipped { track: QueuedTrack },
}

pub struct PlaybackStopResult {
    pub removed_tracks: usize,
}

fn control_transition(control: PlaybackControl) -> (PlaybackState, PlaybackState, &'static str) {
    match control {
        PlaybackControl::Pause => (PlaybackState::Playing, PlaybackState::Paused, "pause"),
        PlaybackControl::Resume => (PlaybackState::Paused, PlaybackState::Playing, "resume"),
    }
}

fn control_track(handle: &TrackHandle, control: PlaybackControl) -> Result<(), AppError> {
    let result = match control {
        PlaybackControl::Pause => handle.pause(),
        PlaybackControl::Resume => handle.play(),
    };
    result.map_err(|source| {
        let operation = match control {
            PlaybackControl::Pause => "pause",
            PlaybackControl::Resume => "resume",
        };
        track_control_error(operation, source)
    })
}

fn stop_created_handle(handle: &TrackHandle) {
    if let Err(source) = handle.stop() {
        warn!(error = %source, "could not stop discarded track handle");
    }
}

fn track_control_error(operation: &'static str, source: songbird::error::ControlError) -> AppError {
    AppError::Voice {
        context: format!("could not {operation} Songbird track: {source}"),
    }
}

fn stale_playback_error() -> AppError {
    AppError::Internal {
        context: "playback operation belonged to an obsolete player session".to_owned(),
    }
}
