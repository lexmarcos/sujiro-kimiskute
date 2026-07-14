use std::sync::Arc;

use serenity::{
    all::{CommandInteraction, Context},
    builder::CreateCommand,
};
use tracing::info;

use crate::discord::player_panel::stopped_message;
use crate::{localization::BotLanguage, state::AppState};

use super::{guild_only_message, respond, respond_app_error};

pub fn definition(language: BotLanguage) -> CreateCommand {
    let description = match language {
        BotLanguage::PtBr => "Para a reprodução e limpa a fila",
        BotLanguage::EnUs => "Stops playback and clears the queue",
    };
    CreateCommand::new("stop").description(description)
}

pub async fn run(
    context: &Context,
    command: &CommandInteraction,
    state: &AppState,
) -> Result<(), serenity::Error> {
    let language = state.config.bot_language;
    let Some(guild_id) = command.guild_id else {
        return respond(context, command, guild_only_message(language), true).await;
    };
    let player = match state
        .voice
        .ensure_same_channel(&context.cache, guild_id, command.user.id)
        .await
    {
        Ok(player) => player,
        Err(source) => return respond_app_error(context, command, language, source).await,
    };
    let stopped = match state.playback.stop(&player).await {
        Ok(stopped) => stopped,
        Err(source) => return respond_app_error(context, command, language, source).await,
    };
    state
        .auto_leave
        .refresh(Arc::clone(&context.cache), guild_id)
        .await;

    info!(
        guild_id = %guild_id,
        user_id = %command.user.id,
        removed_tracks = stopped.removed_tracks,
        "stop command completed"
    );
    respond(
        context,
        command,
        &stopped_message(stopped.removed_tracks, language),
        false,
    )
    .await
}
