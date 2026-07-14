use serenity::{
    all::{CommandInteraction, Context},
    builder::CreateCommand,
};

use crate::{player::playback::PlaybackControlResult, state::AppState};

use super::{respond, respond_app_error};

pub fn definition() -> CreateCommand {
    CreateCommand::new("pause").description("Pausa a música atual")
}

pub async fn run(
    context: &Context,
    command: &CommandInteraction,
    state: &AppState,
) -> Result<(), serenity::Error> {
    let Some(guild_id) = command.guild_id else {
        return respond(
            context,
            command,
            "🏠 Use este comando dentro de um servidor.",
            true,
        )
        .await;
    };
    let player = match state
        .voice
        .ensure_same_channel(&context.cache, guild_id, command.user.id)
        .await
    {
        Ok(player) => player,
        Err(source) => return respond_app_error(context, command, source).await,
    };
    let result = match state.playback.pause(&player).await {
        Ok(result) => result,
        Err(source) => return respond_app_error(context, command, source).await,
    };

    respond(context, command, response_message(result), false).await
}

fn response_message(result: PlaybackControlResult) -> &'static str {
    match result {
        PlaybackControlResult::Changed => "⏸️ Reprodução pausada. Use `/resume` para continuar.",
        PlaybackControlResult::NoTrack => "🎵 Nenhuma música está tocando agora.",
        PlaybackControlResult::AlreadyPaused => {
            "⏸️ A reprodução já está pausada. Use `/resume` para continuar."
        }
        PlaybackControlResult::AlreadyPlaying => "▶️ A reprodução já está tocando.",
    }
}
