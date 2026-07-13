use std::sync::Arc;

use serenity::{
    all::{CommandInteraction, Context},
    builder::CreateCommand,
};
use tracing::info;

use crate::state::AppState;

use super::{respond, respond_app_error};

pub fn definition() -> CreateCommand {
    CreateCommand::new("skip").description("Pula a música atual")
}

pub async fn run(
    context: &Context,
    command: &CommandInteraction,
    state: &Arc<AppState>,
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
    let skipped = match state.playback.skip(player).await {
        Ok(skipped) => skipped,
        Err(source) => return respond_app_error(context, command, source).await,
    };
    let Some(track) = skipped else {
        return respond(context, command, "Nenhuma música está tocando.", false).await;
    };

    info!(
        guild_id = %guild_id,
        user_id = %command.user.id,
        track_id = %track.track.id,
        "skip command completed"
    );
    respond(
        context,
        command,
        &format!("Pulando: {}", track.track.title),
        false,
    )
    .await
}
