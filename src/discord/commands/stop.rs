use std::sync::Arc;

use serenity::{
    all::{CommandInteraction, Context},
    builder::CreateCommand,
};
use tracing::info;

use crate::state::AppState;

use super::{respond, respond_app_error};

pub fn definition() -> CreateCommand {
    CreateCommand::new("stop").description("Para a reprodução e limpa a fila")
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
    let stopped = match state.playback.stop(&player).await {
        Ok(stopped) => stopped,
        Err(source) => return respond_app_error(context, command, source).await,
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
        "Reprodução interrompida e fila limpa.",
        false,
    )
    .await
}
