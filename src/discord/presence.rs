use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serenity::{all::ActivityData, gateway::ShardMessenger};
use tokio::sync::OnceCell;

use crate::{
    config::{BotActivityConfig, BotActivityType},
    player::{manager::PlayerManager, observer::PlayerObserver, track::QueuedTrack},
};

pub struct PresenceService {
    shard: OnceCell<ShardMessenger>,
    players: Arc<PlayerManager>,
    configured_activity: ActivityData,
    current_track_enabled: bool,
    last_activity: Mutex<Option<ActivityData>>,
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
            last_activity: Mutex::new(None),
        })
    }

    pub fn initialize(&self, shard: ShardMessenger) {
        let _ = self.shard.set(shard);
        // A new gateway session starts with no activity, so the cache must not
        // suppress the first update after `ready`.
        self.clear_last_activity();
        self.set_configured();
    }

    async fn refresh(&self) {
        if !self.current_track_enabled {
            self.set_configured();
            return;
        }
        self.set_activity(self.current_track_activity().await);
    }

    /// Falls back to the configured activity unless exactly one guild is
    /// playing, because a single presence cannot represent several tracks.
    async fn current_track_activity(&self) -> ActivityData {
        let mut active_title = None;
        for player in self.players.all().await {
            let Some(title) = player.playing_title().await else {
                continue;
            };
            if active_title.is_some() {
                return self.configured_activity.clone();
            }
            active_title = Some(title);
        }

        active_title
            .map(|title| ActivityData::listening(truncate_presence(&title)))
            .unwrap_or_else(|| self.configured_activity.clone())
    }

    fn set_configured(&self) {
        self.set_activity(self.configured_activity.clone());
    }

    // Serenity sends a gateway presence update for every `set_activity` call and
    // Discord allows only 5 per 20 seconds per session, so identical activities
    // must not reach the shard.
    fn set_activity(&self, activity: ActivityData) {
        let Some(shard) = self.shard.get() else {
            return;
        };
        {
            let mut last_activity = match self.last_activity.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            if last_activity.as_ref() == Some(&activity) {
                return;
            }
            *last_activity = Some(activity.clone());
        }
        shard.set_activity(Some(activity));
    }

    fn clear_last_activity(&self) {
        let mut last_activity = match self.last_activity.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        *last_activity = None;
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
