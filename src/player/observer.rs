use async_trait::async_trait;
use serenity::model::id::GuildId;

#[async_trait]
pub trait PlayerObserver: Send + Sync {
    async fn player_changed(&self, guild_id: GuildId);
}
