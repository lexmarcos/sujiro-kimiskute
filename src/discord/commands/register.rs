use serenity::{
    all::{Command, GuildId},
    builder::CreateCommand,
    http::Http,
};
use thiserror::Error;
use tracing::info;

use crate::localization::BotLanguage;

use super::{leave, pause, play, queue, resume, skip, stop};

#[derive(Debug, Error)]
pub enum CommandRegistrationError {
    #[error("failed to clear application commands for guild {guild_id}: {source}")]
    ClearGuild {
        guild_id: GuildId,
        #[source]
        source: serenity::Error,
    },

    #[error("failed to register current global application commands: {source}")]
    RegisterGlobal {
        #[source]
        source: serenity::Error,
    },
}

impl CommandRegistrationError {
    pub fn guild_id(&self) -> Option<GuildId> {
        match self {
            Self::ClearGuild { guild_id, .. } => Some(*guild_id),
            Self::RegisterGlobal { .. } => None,
        }
    }
}

pub async fn synchronize(
    http: &Http,
    guild_ids: &[GuildId],
    language: BotLanguage,
) -> Result<(), CommandRegistrationError> {
    // Clearing first invalidates command IDs still cached by Discord clients.
    register_global(http, language).await?;
    clear_guilds(http, guild_ids).await
}

async fn clear_guilds(http: &Http, guild_ids: &[GuildId]) -> Result<(), CommandRegistrationError> {
    for guild_id in guild_ids {
        clear_guild(http, *guild_id).await?;
    }
    info!(guild_count = guild_ids.len(), "guild commands cleared");
    Ok(())
}

async fn clear_guild(http: &Http, guild_id: GuildId) -> Result<(), CommandRegistrationError> {
    let remaining = guild_id
        .set_commands(http, Vec::new())
        .await
        .map_err(|source| CommandRegistrationError::ClearGuild { guild_id, source })?;
    info!(%guild_id, command_count = remaining.len(), "guild commands cleared");
    Ok(())
}

async fn register_global(
    http: &Http,
    language: BotLanguage,
) -> Result<(), CommandRegistrationError> {
    let commands: Vec<CreateCommand> = definitions(language);
    let registered = Command::set_global_commands(http, commands)
        .await
        .map_err(|source| CommandRegistrationError::RegisterGlobal { source })?;

    info!(
        command_count = registered.len(),
        "global commands registered"
    );
    Ok(())
}

fn definitions(language: BotLanguage) -> Vec<CreateCommand> {
    vec![
        play::definition(language),
        pause::definition(language),
        resume::definition(language),
        skip::definition(language),
        stop::definition(language),
        queue::definition(language),
        leave::definition(language),
    ]
}
