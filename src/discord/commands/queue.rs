use serenity::{
    all::{CommandInteraction, Context, GuildId},
    builder::{CreateCommand, CreateInteractionResponse, CreateInteractionResponseMessage},
};

use crate::{player::guild_player::GuildPlayerSnapshot, state::AppState};

use super::respond;
use crate::discord::player_panel::{control_row, now_playing_embed, upcoming_tracks};

pub fn definition() -> CreateCommand {
    CreateCommand::new("queue").description("Mostra a música atual e as próximas da fila")
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
    let Some(snapshot) = queue_snapshot(state, guild_id).await else {
        return respond(context, command, empty_queue_message(), false).await;
    };
    respond_with_snapshot(context, command, &snapshot).await
}

async fn queue_snapshot(state: &AppState, guild_id: GuildId) -> Option<GuildPlayerSnapshot> {
    let player = state.players.get(guild_id).await?;
    let snapshot = player.snapshot().await;
    if snapshot.current.is_none() && snapshot.queued.is_empty() {
        return None;
    }
    Some(snapshot)
}

async fn respond_with_snapshot(
    context: &Context,
    command: &CommandInteraction,
    snapshot: &GuildPlayerSnapshot,
) -> Result<(), serenity::Error> {
    let message = match now_playing_embed(snapshot) {
        Some(embed) => CreateInteractionResponseMessage::new()
            .content("🎵 **Tocando agora**")
            .embed(embed)
            .components(vec![control_row(snapshot)]),
        None => CreateInteractionResponseMessage::new().content(waiting_queue_message(snapshot)),
    };
    command
        .create_response(&context.http, CreateInteractionResponse::Message(message))
        .await
}

fn waiting_queue_message(snapshot: &GuildPlayerSnapshot) -> String {
    let upcoming = upcoming_tracks(&snapshot.queued).unwrap_or_default();
    format!("⏳ **Preparando a próxima música**\n{upcoming}")
}

fn empty_queue_message() -> &'static str {
    "📭 A fila está vazia. Use `/play` para adicionar uma música."
}
