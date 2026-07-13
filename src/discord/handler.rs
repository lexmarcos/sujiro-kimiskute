use std::sync::Arc;

use serenity::{
    all::{Context, EventHandler, Interaction, Ready, VoiceState},
    async_trait,
};
use tracing::{error, info};

use crate::state::AppState;

use super::commands;

pub struct DiscordEventHandler {
    state: Arc<AppState>,
}

impl DiscordEventHandler {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl EventHandler for DiscordEventHandler {
    async fn ready(&self, context: Context, ready: Ready) {
        info!(
            user_id = %ready.user.id,
            username = %ready.user.name,
            guild_count = ready.guilds.len(),
            "Discord bot ready"
        );

        if let Err(source) = commands::register::register_global(&context.http).await {
            error!(error = %source, "failed to register global commands");
        }
    }

    async fn interaction_create(&self, context: Context, interaction: Interaction) {
        let Interaction::Command(command) = interaction else {
            return;
        };

        commands::dispatch(&context, &command, &self.state).await;
    }

    async fn voice_state_update(&self, context: Context, old: Option<VoiceState>, new: VoiceState) {
        let guild_id = new
            .guild_id
            .or_else(|| old.and_then(|state| state.guild_id));
        let Some(guild_id) = guild_id else {
            return;
        };
        self.state
            .auto_leave
            .refresh(Arc::clone(&context.cache), guild_id)
            .await;
    }
}
