use std::collections::VecDeque;

use serenity::model::id::{ChannelId, GuildId};
use songbird::tracks::TrackHandle;
use tokio::sync::{Mutex, Notify};

use crate::{
    error::AppError,
    player::{
        lifecycle::{AutoLeaveTimer, PlayerLifecycle},
        play_requests::PlayRequestSequencer,
        playback_state::{CurrentTrack, PlaybackOperation, PlaybackState},
        queue::TrackQueue,
        track::QueuedTrack,
        voice_state::{
            VoiceConnectionClaim, VoiceConnectionState, VoiceOperation, existing_connection_claim,
        },
    },
};

mod control;
mod interrupt;
mod lifecycle;
mod play_requests;
mod queue;

#[derive(Clone)]
pub struct GuildPlayerSnapshot {
    pub voice_channel_id: Option<ChannelId>,
    pub current: Option<QueuedTrack>,
    pub queued: Vec<QueuedTrack>,
    pub playback_state: PlaybackState,
    pub session_epoch: u64,
    pub playback_id: u64,
}

pub struct GuildPlayer {
    guild_id: GuildId,
    instance_id: u64,
    inner: Mutex<GuildPlayerState>,
    voice_change: Notify,
    play_requests: PlayRequestSequencer,
}

struct GuildPlayerState {
    voice_channel_id: Option<ChannelId>,
    voice_connection: VoiceConnectionState,
    next_voice_operation_id: u64,
    current: Option<CurrentTrack>,
    queue: TrackQueue,
    history: VecDeque<QueuedTrack>,
    playback_state: PlaybackState,
    session_epoch: u64,
    playback_id: u64,
    queue_advancer_active: bool,
    lifecycle: PlayerLifecycle,
    auto_leave_generation: u64,
    auto_leave_timer: Option<AutoLeaveTimer>,
}

impl GuildPlayer {
    pub fn new(
        guild_id: GuildId,
        instance_id: u64,
        max_queue_size: usize,
    ) -> Result<Self, AppError> {
        Ok(Self {
            guild_id,
            instance_id,
            inner: Mutex::new(GuildPlayerState {
                voice_channel_id: None,
                voice_connection: VoiceConnectionState::Disconnected,
                next_voice_operation_id: 0,
                current: None,
                queue: TrackQueue::new(max_queue_size)?,
                history: VecDeque::with_capacity(max_queue_size),
                playback_state: PlaybackState::Idle,
                session_epoch: 0,
                playback_id: 0,
                queue_advancer_active: false,
                lifecycle: PlayerLifecycle::Active,
                auto_leave_generation: 0,
                auto_leave_timer: None,
            }),
            voice_change: Notify::new(),
            play_requests: PlayRequestSequencer::new(),
        })
    }

    pub fn guild_id(&self) -> GuildId {
        self.guild_id
    }

    pub fn instance_id(&self) -> u64 {
        self.instance_id
    }

    pub async fn voice_channel_id(&self) -> Option<ChannelId> {
        self.inner.lock().await.voice_channel_id
    }

    pub async fn current_track(&self) -> Option<QueuedTrack> {
        self.inner
            .lock()
            .await
            .current
            .as_ref()
            .map(|current| current.track.clone())
    }

    pub async fn playback_state(&self) -> PlaybackState {
        self.inner.lock().await.playback_state
    }

    pub async fn snapshot(&self) -> GuildPlayerSnapshot {
        let state = self.inner.lock().await;
        GuildPlayerSnapshot {
            voice_channel_id: state.voice_channel_id,
            current: state.current.as_ref().map(|current| current.track.clone()),
            queued: state.queue.iter().cloned().collect(),
            playback_state: state.playback_state,
            session_epoch: state.session_epoch,
            playback_id: state.playback_id,
        }
    }

    pub async fn advance_session_epoch(&self) -> u64 {
        let mut state = self.inner.lock().await;
        state.session_epoch = state.session_epoch.wrapping_add(1);
        state.session_epoch
    }

    pub(crate) async fn active_voice_channel_id(&self) -> Option<ChannelId> {
        self.inner.lock().await.voice_connection.channel_id()
    }

    pub(crate) async fn claim_voice_connection(
        &self,
        channel_id: ChannelId,
    ) -> Result<VoiceConnectionClaim, AppError> {
        let mut state = self.inner.lock().await;
        state.ensure_active(self.guild_id)?;
        if let Some(claim) = existing_connection_claim(&state.voice_connection, channel_id)? {
            return Ok(claim);
        }

        let operation = state.new_voice_operation(channel_id);
        state.voice_connection = VoiceConnectionState::Connecting {
            operation_id: operation.operation_id,
            channel_id,
        };
        Ok(VoiceConnectionClaim::Start(operation))
    }

    pub(crate) async fn claim_voice_disconnection(&self) -> VoiceConnectionClaim {
        let mut state = self.inner.lock().await;
        match state.voice_connection {
            VoiceConnectionState::Disconnected => VoiceConnectionClaim::Ready,
            VoiceConnectionState::Connecting { operation_id, .. }
            | VoiceConnectionState::Disconnecting { operation_id, .. } => {
                VoiceConnectionClaim::Wait { operation_id }
            }
            VoiceConnectionState::Connected { channel_id } => {
                let operation = state.new_voice_operation(channel_id);
                state.voice_connection = VoiceConnectionState::Disconnecting {
                    operation_id: operation.operation_id,
                    channel_id,
                };
                VoiceConnectionClaim::Start(operation)
            }
        }
    }

    pub(crate) async fn voice_connection_operation_is_current(
        &self,
        operation: VoiceOperation,
    ) -> bool {
        let state = self.inner.lock().await;
        state.lifecycle == PlayerLifecycle::Active
            && state.session_epoch == operation.session_epoch
            && state.voice_connection.matches_connect(operation)
    }

    pub(crate) async fn finish_voice_connection(
        &self,
        operation: VoiceOperation,
        succeeded: bool,
        instance_is_current: bool,
    ) -> bool {
        let (confirmed, notify_waiters) = {
            let mut state = self.inner.lock().await;
            state.finish_voice_connection(operation, succeeded, instance_is_current)
        };
        if notify_waiters {
            self.voice_change.notify_waiters();
        }
        confirmed
    }

    pub(crate) async fn finish_voice_disconnection(
        &self,
        operation: VoiceOperation,
        succeeded: bool,
        instance_is_current: bool,
    ) -> bool {
        let (confirmed, notify_waiters) = {
            let mut state = self.inner.lock().await;
            state.finish_voice_disconnection(operation, succeeded, instance_is_current)
        };
        if notify_waiters {
            self.voice_change.notify_waiters();
        }
        confirmed
    }

    pub(crate) async fn wait_for_voice_operation(&self, operation_id: u64) {
        let notification = self.voice_change.notified();
        tokio::pin!(notification);
        notification.as_mut().enable();

        let still_running =
            self.inner.lock().await.voice_connection.operation_id() == Some(operation_id);
        if still_running {
            notification.await;
        }
    }

    pub(crate) async fn claim_playback_start(
        &self,
        track: QueuedTrack,
    ) -> Result<PlaybackOperation, AppError> {
        let mut state = self.inner.lock().await;
        state.ensure_active(self.guild_id)?;
        if state.current.is_some()
            || state.playback_state != PlaybackState::Idle
            || state.queue_advancer_active
        {
            return Err(AppError::Voice {
                context: format!("guild {} already has an active track", self.guild_id),
            });
        }

        Ok(state.begin_playback(track))
    }

    pub(crate) async fn install_playback_handle(
        &self,
        operation: PlaybackOperation,
        handle: TrackHandle,
        instance_is_current: bool,
    ) -> bool {
        let mut state = self.inner.lock().await;
        if state.ensure_active(self.guild_id).is_err() {
            return false;
        }
        if !state.current_matches(operation) {
            return false;
        }
        if state.session_epoch != operation.session_epoch || !instance_is_current {
            state.reset_playback();
            return false;
        }

        let Some(current) = state.current.as_mut() else {
            return false;
        };
        current.handle = Some(handle);
        true
    }

    pub(crate) async fn mark_playback_playing(&self, operation: PlaybackOperation) -> bool {
        let mut state = self.inner.lock().await;
        if state.ensure_active(self.guild_id).is_err() {
            return false;
        }
        if !state.current_matches(operation) || state.playback_state != PlaybackState::Starting {
            return false;
        }
        if state
            .current
            .as_ref()
            .and_then(|current| current.handle.as_ref())
            .is_none()
        {
            return false;
        }

        state.playback_state = PlaybackState::Playing;
        true
    }

    pub(crate) async fn clear_playback(&self, operation: PlaybackOperation) -> Option<QueuedTrack> {
        let mut state = self.inner.lock().await;
        if !state.current_matches(operation) {
            return None;
        }

        let current = state.current.take()?;
        state.playback_state = PlaybackState::Idle;
        Some(current.track)
    }
}

impl GuildPlayerState {
    fn begin_playback(&mut self, track: QueuedTrack) -> PlaybackOperation {
        self.playback_id = self.playback_id.wrapping_add(1);
        let operation = PlaybackOperation {
            playback_id: self.playback_id,
            session_epoch: self.session_epoch,
        };
        self.current = Some(CurrentTrack {
            track,
            playback_id: operation.playback_id,
            session_epoch: operation.session_epoch,
            handle: None,
        });
        self.playback_state = PlaybackState::Starting;
        operation
    }

    fn claim_queue_advancer(&mut self) -> bool {
        if self.queue_advancer_active
            || self.current.is_some()
            || self.playback_state != PlaybackState::Idle
            || self.queue.is_empty()
        {
            return false;
        }

        self.queue_advancer_active = true;
        true
    }

    fn record_completed_track(&mut self, track: QueuedTrack) {
        if self.history.len() == self.queue.max_size() {
            self.history.pop_front();
        }
        self.history.push_back(track);
    }

    fn current_matches(&self, operation: PlaybackOperation) -> bool {
        self.current.as_ref().is_some_and(|current| {
            current.playback_id == operation.playback_id
                && current.session_epoch == operation.session_epoch
        })
    }

    fn reset_playback(&mut self) {
        self.current = None;
        self.playback_state = PlaybackState::Idle;
    }

    fn new_voice_operation(&mut self, channel_id: ChannelId) -> VoiceOperation {
        self.next_voice_operation_id = self.next_voice_operation_id.wrapping_add(1);
        VoiceOperation {
            operation_id: self.next_voice_operation_id,
            channel_id,
            session_epoch: self.session_epoch,
        }
    }

    fn finish_voice_connection(
        &mut self,
        operation: VoiceOperation,
        succeeded: bool,
        instance_is_current: bool,
    ) -> (bool, bool) {
        if !self.voice_connection.matches_connect(operation) {
            return (false, false);
        }
        if self.session_epoch != operation.session_epoch || !instance_is_current {
            self.reset_voice_connection();
            return (false, true);
        }

        if succeeded {
            self.voice_channel_id = Some(operation.channel_id);
            self.voice_connection = VoiceConnectionState::Connected {
                channel_id: operation.channel_id,
            };
        } else {
            self.voice_channel_id = None;
            self.voice_connection = VoiceConnectionState::Disconnected;
        }
        (true, true)
    }

    fn finish_voice_disconnection(
        &mut self,
        operation: VoiceOperation,
        succeeded: bool,
        instance_is_current: bool,
    ) -> (bool, bool) {
        if !self.voice_connection.matches_disconnect(operation) {
            return (false, false);
        }
        if self.session_epoch != operation.session_epoch || !instance_is_current {
            self.reset_voice_connection();
            return (false, true);
        }

        if succeeded {
            self.voice_channel_id = None;
            self.voice_connection = VoiceConnectionState::Disconnected;
        } else {
            self.voice_channel_id = Some(operation.channel_id);
            self.voice_connection = VoiceConnectionState::Connected {
                channel_id: operation.channel_id,
            };
        }
        (true, true)
    }

    fn reset_voice_connection(&mut self) {
        self.voice_channel_id = None;
        self.voice_connection = VoiceConnectionState::Disconnected;
    }
}
