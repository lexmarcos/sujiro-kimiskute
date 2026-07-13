use std::sync::Weak;

use serenity::model::id::GuildId;
use songbird::events::{Event, EventContext, EventHandler};
use songbird::tracks::PlayMode;
use tracing::error;

use crate::player::{playback::PlaybackService, playback_state::PlaybackOperation};

pub(crate) struct PlaybackEndHandler {
    playback: Weak<PlaybackService>,
    guild_id: GuildId,
    instance_id: u64,
    operation: PlaybackOperation,
}

impl PlaybackEndHandler {
    pub(crate) fn new(
        playback: Weak<PlaybackService>,
        guild_id: GuildId,
        instance_id: u64,
        operation: PlaybackOperation,
    ) -> Self {
        Self {
            playback,
            guild_id,
            instance_id,
            operation,
        }
    }
}

#[async_trait::async_trait]
impl EventHandler for PlaybackEndHandler {
    async fn act(&self, _context: &EventContext<'_>) -> Option<Event> {
        if let Some(playback) = self.playback.upgrade() {
            playback
                .track_ended(self.guild_id, self.instance_id, self.operation)
                .await;
        }
        Some(Event::Cancel)
    }
}

pub(crate) struct PlaybackErrorHandler {
    guild_id: GuildId,
    instance_id: u64,
    operation: PlaybackOperation,
}

impl PlaybackErrorHandler {
    pub(crate) fn new(guild_id: GuildId, instance_id: u64, operation: PlaybackOperation) -> Self {
        Self {
            guild_id,
            instance_id,
            operation,
        }
    }
}

#[async_trait::async_trait]
impl EventHandler for PlaybackErrorHandler {
    async fn act(&self, context: &EventContext<'_>) -> Option<Event> {
        let EventContext::Track(tracks) = context else {
            return Some(Event::Cancel);
        };
        let Some((state, _)) = tracks.first() else {
            return Some(Event::Cancel);
        };
        let PlayMode::Errored(source) = &state.playing else {
            return Some(Event::Cancel);
        };

        error!(
            guild_id = %self.guild_id,
            player_instance_id = self.instance_id,
            playback_id = self.operation.playback_id,
            session_epoch = self.operation.session_epoch,
            ready_state = ?state.ready,
            error = %source,
            "Songbird track playback failed"
        );
        Some(Event::Cancel)
    }
}
