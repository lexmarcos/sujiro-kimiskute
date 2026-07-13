use serenity::{
    all::{CommandInteraction, Context},
    builder::CreateCommand,
};

use crate::{player::playback::PlaybackControlResult, state::AppState};

use super::{respond, respond_app_error};

pub fn definition() -> CreateCommand {
    CreateCommand::new("resume").description("Retoma a música atual")
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
            "Este comando só pode ser usado em um servidor.",
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
    let result = match state.playback.resume(&player).await {
        Ok(result) => result,
        Err(source) => return respond_app_error(context, command, source).await,
    };

    respond(context, command, response_message(result), false).await
}

fn response_message(result: PlaybackControlResult) -> &'static str {
    match result {
        PlaybackControlResult::Changed => "Reprodução retomada.",
        PlaybackControlResult::NoTrack => "Nenhuma música está tocando.",
        PlaybackControlResult::AlreadyPaused => "A reprodução já está pausada.",
        PlaybackControlResult::AlreadyPlaying => "A reprodução já está tocando.",
    }
}
