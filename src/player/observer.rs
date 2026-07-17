use async_trait::async_trait;
use serenity::model::id::GuildId;

use crate::player::track::QueuedTrack;

#[async_trait]
pub trait PlayerObserver: Send + Sync {
    async fn player_changed(&self, guild_id: GuildId);

    async fn track_failed(&self, guild_id: GuildId, track: &QueuedTrack);
}
