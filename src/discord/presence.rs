use std::sync::Arc;

use async_trait::async_trait;
use serenity::{all::ActivityData, gateway::ShardMessenger};
use tokio::sync::OnceCell;

use crate::{
    config::{BotActivityConfig, BotActivityType},
    player::{
        manager::PlayerManager, observer::PlayerObserver, playback_state::PlaybackState,
        track::QueuedTrack,
    },
};

pub struct PresenceService {
    shard: OnceCell<ShardMessenger>,
    players: Arc<PlayerManager>,
    configured_activity: ActivityData,
    current_track_enabled: bool,
}

impl PresenceService {
    pub(crate) fn new(
        players: Arc<PlayerManager>,
        configuration: &BotActivityConfig,
        current_track_enabled: bool,
    ) -> Arc<Self> {
        Arc::new(Self {
            shard: OnceCell::new(),
            players,
            configured_activity: activity_data(configuration),
            current_track_enabled,
        })
    }

    pub fn initialize(&self, shard: ShardMessenger) {
        let _ = self.shard.set(shard);
        self.set_configured();
    }

    async fn refresh(&self) {
        if !self.current_track_enabled {
            self.set_configured();
            return;
        }
        let players = self.players.all().await;
        let mut active_title = None;
        for player in players {
            let snapshot = player.snapshot().await;
            if snapshot.playback_state != PlaybackState::Playing {
                continue;
            }
            let Some(current) = snapshot.current else {
                continue;
            };
            if active_title.is_some() {
                self.set_configured();
                return;
            }
            active_title = Some(current.track.title);
        }

        let activity = active_title
            .map(|title| ActivityData::listening(truncate_presence(&title)))
            .unwrap_or_else(|| self.configured_activity.clone());
        self.set_activity(activity);
    }

    fn set_configured(&self) {
        self.set_activity(self.configured_activity.clone());
    }

    fn set_activity(&self, activity: ActivityData) {
        if let Some(shard) = self.shard.get() {
            shard.set_activity(Some(activity));
        }
    }
}

#[async_trait]
impl PlayerObserver for PresenceService {
    async fn player_changed(&self, _guild_id: serenity::model::id::GuildId) {
        self.refresh().await;
    }

    async fn track_failed(&self, _guild_id: serenity::model::id::GuildId, _track: &QueuedTrack) {}
}

pub(super) fn activity_data(configuration: &BotActivityConfig) -> ActivityData {
    let message = configuration.message();
    match configuration.activity_type() {
        BotActivityType::Playing => ActivityData::playing(message),
        BotActivityType::Watching => ActivityData::watching(message),
        BotActivityType::Listening => ActivityData::listening(message),
        BotActivityType::Competing => ActivityData::competing(message),
    }
}

fn truncate_presence(title: &str) -> String {
    const MAX_PRESENCE_CHARS: usize = 120;
    if title.chars().count() <= MAX_PRESENCE_CHARS {
        return title.to_owned();
    }
    let mut truncated: String = title.chars().take(MAX_PRESENCE_CHARS - 1).collect();
    truncated.push('…');
    truncated
}
