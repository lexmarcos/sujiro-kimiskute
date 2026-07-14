use std::sync::Arc;

use serenity::{
    all::{CommandInteraction, Context},
    builder::CreateCommand,
};
use tracing::info;

use crate::{localization::BotLanguage, player::playback::PlaybackSkipResult, state::AppState};

use super::{guild_only_message, respond, respond_app_error, truncate_text};

const MAX_TITLE_CHARS: usize = 160;

pub fn definition(language: BotLanguage) -> CreateCommand {
    let description = match language {
        BotLanguage::PtBr => "Pula a música atual",
        BotLanguage::EnUs => "Skips the current track",
    };
    CreateCommand::new("skip").description(description)
}

pub async fn run(
    context: &Context,
    command: &CommandInteraction,
    state: &Arc<AppState>,
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
    let result = match state.playback.skip(player).await {
        Ok(result) => result,
        Err(source) => return respond_app_error(context, command, language, source).await,
    };
    let track = match result {
        PlaybackSkipResult::NoTrack => {
            let message = match language {
                BotLanguage::PtBr => "🎵 Nenhuma música está tocando para pular.",
                BotLanguage::EnUs => "🎵 No track is playing to skip.",
            };
            return respond(context, command, message, false).await;
        }
        PlaybackSkipResult::NoNext => {
            let message = match language {
                BotLanguage::PtBr => "⏭️ Não há próxima música na fila.",
                BotLanguage::EnUs => "⏭️ There is no next track in the queue.",
            };
            return respond(context, command, message, false).await;
        }
        PlaybackSkipResult::Skipped { track } => track,
    };

    info!(
        guild_id = %guild_id,
        user_id = %command.user.id,
        track_id = %track.track.id,
        "skip command completed"
    );
    let title = truncate_text(&track.track.title, MAX_TITLE_CHARS);
    let message = match language {
        BotLanguage::PtBr => format!("⏭️ **{title}** foi pulada."),
        BotLanguage::EnUs => format!("⏭️ Skipped **{title}**."),
    };
    respond(context, command, &message, false).await
}
