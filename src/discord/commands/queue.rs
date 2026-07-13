use serenity::{
    all::{CommandInteraction, Context, GuildId},
    builder::CreateCommand,
};

use crate::{
    player::{guild_player::GuildPlayerSnapshot, track::QueuedTrack},
    state::AppState,
};

use super::{MAX_RESPONSE_CHARS, respond, truncate_text};

const MAX_NEXT_TRACKS: usize = 10;
const MAX_TITLE_CHARS: usize = 80;

pub fn definition() -> CreateCommand {
    CreateCommand::new("queue").description("Mostra a fila de músicas")
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
    let message = queue_message(state, guild_id).await;
    respond(context, command, &message, false).await
}

async fn queue_message(state: &AppState, guild_id: GuildId) -> String {
    let Some(player) = state.players.get(guild_id).await else {
        return "A fila está vazia.".to_owned();
    };
    let snapshot = player.snapshot().await;
    if snapshot.current.is_none() && snapshot.queued.is_empty() {
        return "A fila está vazia.".to_owned();
    }
    format_snapshot(&snapshot)
}

fn format_snapshot(snapshot: &GuildPlayerSnapshot) -> String {
    let mut response = String::new();
    if let Some(current) = &snapshot.current {
        response.push_str("**Tocando agora**\n");
        response.push_str(&format_track(current));
    }
    if !snapshot.queued.is_empty() {
        append_upcoming_tracks(&mut response, &snapshot.queued);
    }
    response
}

fn append_upcoming_tracks(response: &mut String, queued: &[QueuedTrack]) {
    if !response.is_empty() {
        response.push_str("\n\n");
    }
    response.push_str("**Próximas**");

    for (index, track) in queued.iter().take(MAX_NEXT_TRACKS).enumerate() {
        let position = index + 1;
        let track_description = format_track(track);
        let line = format!("\n{position}. {track_description}");
        if !append_complete(response, &line) {
            break;
        }
    }
}

fn format_track(track: &QueuedTrack) -> String {
    let title = truncate_text(&track.track.title, MAX_TITLE_CHARS);
    let duration = track
        .track
        .duration_seconds
        .map(|seconds| format!(" — {}", format_duration(seconds)))
        .unwrap_or_default();
    format!("{title}{duration} — <@{}>", track.requested_by)
}

fn format_duration(total_seconds: u64) -> String {
    let hours = total_seconds / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        return format!("{hours}:{minutes:02}:{seconds:02}");
    }
    format!("{minutes}:{seconds:02}")
}

fn append_complete(response: &mut String, addition: &str) -> bool {
    if response.chars().count() + addition.chars().count() > MAX_RESPONSE_CHARS {
        return false;
    }
    response.push_str(addition);
    true
}
