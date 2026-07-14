use serenity::{
    all::{CommandInteraction, Context},
    builder::CreateCommand,
};

use crate::{localization::BotLanguage, player::playback::PlaybackControlResult, state::AppState};

use super::{guild_only_message, respond, respond_app_error};

pub fn definition(language: BotLanguage) -> CreateCommand {
    let description = match language {
        BotLanguage::PtBr => "Pausa a música atual",
        BotLanguage::EnUs => "Pauses the current track",
    };
    CreateCommand::new("pause").description(description)
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
    let result = match state.playback.pause(&player).await {
        Ok(result) => result,
        Err(source) => return respond_app_error(context, command, language, source).await,
    };

    respond(context, command, response_message(result, language), false).await
}

fn response_message(result: PlaybackControlResult, language: BotLanguage) -> &'static str {
    match (language, result) {
        (BotLanguage::PtBr, PlaybackControlResult::Changed) => {
            "⏸️ Reprodução pausada. Use `/resume` para continuar."
        }
        (BotLanguage::PtBr, PlaybackControlResult::NoTrack) => {
            "🎵 Nenhuma música está tocando agora."
        }
        (BotLanguage::PtBr, PlaybackControlResult::AlreadyPaused) => {
            "⏸️ A reprodução já está pausada. Use `/resume` para continuar."
        }
        (BotLanguage::PtBr, PlaybackControlResult::AlreadyPlaying) => {
            "▶️ A reprodução já está tocando."
        }
        (BotLanguage::EnUs, PlaybackControlResult::Changed) => {
            "⏸️ Playback paused. Use `/resume` to continue."
        }
        (BotLanguage::EnUs, PlaybackControlResult::NoTrack) => "🎵 No track is playing right now.",
        (BotLanguage::EnUs, PlaybackControlResult::AlreadyPaused) => {
            "⏸️ Playback is already paused. Use `/resume` to continue."
        }
        (BotLanguage::EnUs, PlaybackControlResult::AlreadyPlaying) => {
            "▶️ Playback is already playing."
        }
    }
}
