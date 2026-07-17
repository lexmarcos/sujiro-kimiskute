use std::sync::Arc;

use async_trait::async_trait;
use serenity::model::id::GuildId;

use crate::player::track::QueuedTrack;

#[async_trait]
pub trait PlayerObserver: Send + Sync {
    async fn player_changed(&self, guild_id: GuildId);

    async fn track_failed(&self, guild_id: GuildId, track: &QueuedTrack);
}

pub struct CompositePlayerObserver {
    observers: Vec<Arc<dyn PlayerObserver>>,
}

impl CompositePlayerObserver {
    pub fn new(observers: Vec<Arc<dyn PlayerObserver>>) -> Arc<Self> {
        Arc::new(Self { observers })
    }
}

#[async_trait]
impl PlayerObserver for CompositePlayerObserver {
    async fn player_changed(&self, guild_id: GuildId) {
        for observer in &self.observers {
            observer.player_changed(guild_id).await;
        }
    }

    async fn track_failed(&self, guild_id: GuildId, track: &QueuedTrack) {
        for observer in &self.observers {
            observer.track_failed(guild_id, track).await;
        }
    }
}
