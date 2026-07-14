use std::sync::Arc;

use serenity::{
    all::{Context, EventHandler, GuildId, Interaction, Ready, VoiceState},
    async_trait,
};
use tokio::sync::OnceCell;
use tracing::{error, info};

use crate::state::AppState;

use super::{commands, player_controls};

pub struct DiscordEventHandler {
    state: Arc<AppState>,
    commands_registered: OnceCell<()>,
}

impl DiscordEventHandler {
    pub fn new(state: Arc<AppState>) -> Self {
        Self {
            state,
            commands_registered: OnceCell::new(),
        }
    }

    async fn synchronize_commands(&self, context: &Context, guild_ids: &[GuildId]) {
        let result = self
            .commands_registered
            .get_or_try_init(|| async move {
                commands::register::reset_and_register(&context.http, guild_ids).await
            })
            .await;

        if let Err(source) = result {
            error!(
                guild_id = ?source.guild_id(),
                error = %source,
                "failed to reset application commands"
            );
        }
    }
}

#[async_trait]
impl EventHandler for DiscordEventHandler {
    async fn ready(&self, context: Context, ready: Ready) {
        let guild_ids: Vec<GuildId> = ready.guilds.iter().map(|guild| guild.id).collect();
        info!(
            user_id = %ready.user.id,
            username = %ready.user.name,
            guild_count = guild_ids.len(),
            "Discord bot ready"
        );

        self.synchronize_commands(&context, &guild_ids).await;
    }

    async fn interaction_create(&self, context: Context, interaction: Interaction) {
        match interaction {
            Interaction::Command(command) => {
                commands::dispatch(&context, &command, &self.state).await;
            }
            Interaction::Component(component) => {
                player_controls::dispatch(&context, &component, &self.state).await;
            }
            _ => {}
        }
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
