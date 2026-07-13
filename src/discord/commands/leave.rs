use serenity::{
    all::{CommandInteraction, Context},
    builder::CreateCommand,
};
use tracing::info;

use crate::state::AppState;

use super::{respond, respond_app_error};

pub fn definition() -> CreateCommand {
    CreateCommand::new("leave").description("Desconecta o bot do canal de voz")
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
    let result = match state.sessions.leave(player).await {
        Ok(result) => result,
        Err(source) => return respond_app_error(context, command, source).await,
    };

    info!(
        guild_id = %guild_id,
        user_id = %command.user.id,
        removed_tracks = result.removed_tracks,
        "leave command completed"
    );
    respond(context, command, "Desconectado do canal de voz.", false).await
}
