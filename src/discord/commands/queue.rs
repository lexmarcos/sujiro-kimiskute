use serenity::{
    all::{CommandInteraction, Context, GuildId},
    builder::{CreateCommand, CreateInteractionResponse, CreateInteractionResponseMessage},
};

use crate::{
    localization::BotLanguage, player::guild_player::GuildPlayerSnapshot, state::AppState,
};

use super::{guild_only_message, respond};
use crate::discord::player_panel::{
    control_row, now_playing_embed, now_playing_message, upcoming_tracks,
};

pub fn definition(language: BotLanguage) -> CreateCommand {
    let description = match language {
        BotLanguage::PtBr => "Mostra a música atual e as próximas da fila",
        BotLanguage::EnUs => "Shows the current track and the next tracks in the queue",
    };
    CreateCommand::new("queue").description(description)
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
    let Some(snapshot) = queue_snapshot(state, guild_id).await else {
        return respond(context, command, empty_queue_message(language), false).await;
    };
    respond_with_snapshot(context, command, &snapshot, language).await
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
    language: BotLanguage,
) -> Result<(), serenity::Error> {
    let message = match now_playing_embed(snapshot, language) {
        Some(embed) => CreateInteractionResponseMessage::new()
            .content(now_playing_message(language))
            .embed(embed)
            .components(vec![control_row(snapshot, language)]),
        None => CreateInteractionResponseMessage::new()
            .content(waiting_queue_message(snapshot, language)),
    };
    command
        .create_response(&context.http, CreateInteractionResponse::Message(message))
        .await
}

fn waiting_queue_message(snapshot: &GuildPlayerSnapshot, language: BotLanguage) -> String {
    let upcoming = upcoming_tracks(&snapshot.queued).unwrap_or_default();
    match language {
        BotLanguage::PtBr => format!("⏳ **Preparando a próxima música**\n{upcoming}"),
        BotLanguage::EnUs => format!("⏳ **Preparing the next track**\n{upcoming}"),
    }
}

fn empty_queue_message(language: BotLanguage) -> &'static str {
    match language {
        BotLanguage::PtBr => "📭 A fila está vazia. Use `/play` para adicionar uma música.",
        BotLanguage::EnUs => "📭 The queue is empty. Use `/play` to add a track.",
    }
}
