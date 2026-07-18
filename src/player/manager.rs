use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use serenity::model::id::GuildId;
use tokio::sync::RwLock;

use crate::{error::AppError, player::guild_player::GuildPlayer};

pub struct PlayerManager {
    players: RwLock<HashMap<GuildId, Arc<GuildPlayer>>>,
    next_instance_id: AtomicU64,
    max_queue_size: usize,
}

impl PlayerManager {
    pub fn new(max_queue_size: usize) -> Result<Self, AppError> {
        if max_queue_size == 0 {
            return Err(AppError::InvalidInput {
                reason: "player queue maximum size must be positive, received 0".to_owned(),
            });
        }
        Ok(Self {
            players: RwLock::new(HashMap::new()),
            next_instance_id: AtomicU64::new(1),
            max_queue_size,
        })
    }

    pub async fn get_or_create(&self, guild_id: GuildId) -> Result<Arc<GuildPlayer>, AppError> {
        if let Some(player) = self.get(guild_id).await {
            return Ok(player);
        }

        let mut players = self.players.write().await;
        if let Some(player) = players.get(&guild_id) {
            return Ok(Arc::clone(player));
        }

        let instance_id = self.next_instance_id.fetch_add(1, Ordering::Relaxed);
        let player = Arc::new(GuildPlayer::new(
            guild_id,
            instance_id,
            self.max_queue_size,
        )?);
        players.insert(guild_id, Arc::clone(&player));
        Ok(player)
    }

    pub async fn get(&self, guild_id: GuildId) -> Option<Arc<GuildPlayer>> {
        self.players.read().await.get(&guild_id).cloned()
    }

    pub async fn all(&self) -> Vec<Arc<GuildPlayer>> {
        self.players.read().await.values().cloned().collect()
    }

    pub async fn remove(&self, guild_id: GuildId) -> Option<Arc<GuildPlayer>> {
        self.players.write().await.remove(&guild_id)
    }

    pub async fn remove_if_same(
        &self,
        guild_id: GuildId,
        instance_id: u64,
    ) -> Option<Arc<GuildPlayer>> {
        let mut players = self.players.write().await;
        let matches_instance = players
            .get(&guild_id)
            .is_some_and(|player| player.instance_id() == instance_id);

        if !matches_instance {
            return None;
        }
        players.remove(&guild_id)
    }
}
