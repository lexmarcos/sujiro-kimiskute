use serenity::{
    all::{CommandInteraction, Context},
    builder::CreateCommand,
};
use tracing::info;

use crate::{localization::BotLanguage, state::AppState};

use super::{guild_only_message, respond, respond_app_error};

pub fn definition(language: BotLanguage) -> CreateCommand {
    let description = match language {
        BotLanguage::PtBr => "Desconecta o bot do canal de voz",
        BotLanguage::EnUs => "Disconnects the bot from the voice channel",
    };
    CreateCommand::new("leave").description(description)
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
    state.idle_leave.cancel_for_activity(guild_id).await;
    let result = match state.sessions.leave(player).await {
        Ok(result) => result,
        Err(source) => return respond_app_error(context, command, language, source).await,
    };

    info!(
        guild_id = %guild_id,
        user_id = %command.user.id,
        removed_tracks = result.removed_tracks,
        "leave command completed"
    );
    let message = match language {
        BotLanguage::PtBr => "Desconectado do canal de voz.",
        BotLanguage::EnUs => "Disconnected from the voice channel.",
    };
    respond(context, command, message, false).await
}
