use std::sync::Arc;

use serenity::{
    all::{CommandInteraction, Context},
    builder::CreateCommand,
};
use tracing::info;

use crate::{player::playback::PlaybackSkipResult, state::AppState};

use super::{respond, respond_app_error, truncate_text};

const MAX_TITLE_CHARS: usize = 160;

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
    let result = match state.playback.skip(player).await {
        Ok(result) => result,
        Err(source) => return respond_app_error(context, command, source).await,
    };
    let track = match result {
        PlaybackSkipResult::NoTrack => {
            return respond(
                context,
                command,
                "🎵 Nenhuma música está tocando para pular.",
                false,
            )
            .await;
        }
        PlaybackSkipResult::NoNext => {
            return respond(context, command, "⏭️ Não há próxima música na fila.", false).await;
        }
        PlaybackSkipResult::Skipped { track } => track,
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
        &format!(
            "⏭️ **{}** foi pulada.",
            truncate_text(&track.track.title, MAX_TITLE_CHARS)
        ),
        false,
    )
    .await
}
