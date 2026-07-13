use std::sync::Arc;

use serenity::{
    all::{
        Cache, ChannelId, CommandDataOptionValue, CommandInteraction, CommandOptionType, Context,
        GuildId, UserId,
    },
    builder::{CreateCommand, CreateCommandOption, EditInteractionResponse},
};
use tracing::error;

use crate::{
    error::{AppError, VoiceChannelIssue},
    player::{
        guild_player::GuildPlayer,
        play_requests::{PlayCommitReceipt, PlayRequestReservation, PlayRequestTicket},
        track::{QueuedTrack, ResolvedTrack},
    },
    sources::resolver::{MAX_TRACK_INPUT_CHARS, normalize_track_input},
    state::AppState,
    voice::VoiceConnection,
};

use super::{MAX_RESPONSE_CHARS, respond, respond_app_error, truncate_text};

const MAX_TITLE_CHARS: usize = 160;

pub fn definition() -> CreateCommand {
    let query = CreateCommandOption::new(
        CommandOptionType::String,
        "query",
        "URL do YouTube ou texto para busca",
    )
    .required(true)
    .max_length(MAX_TRACK_INPUT_CHARS);

    CreateCommand::new("play")
        .description("Adiciona uma música à fila")
        .add_option(query)
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
    let query = match normalized_query(command) {
        Ok(query) => query,
        Err(error) => return respond_app_error(context, command, error).await,
    };
    let channel_id = match VoiceConnection::user_channel(&context.cache, guild_id, command.user.id)
    {
        Ok(channel_id) => channel_id,
        Err(error) => {
            return respond(context, command, &error.discord_message(), true).await;
        }
    };

    command.defer(&context.http).await?;
    let player = match state.players.get_or_create(guild_id).await {
        Ok(player) => player,
        Err(error) => return edit_error(context, command, error).await,
    };
    if let Err(error) = state.auto_leave.cancel_for_activity(&player).await {
        return edit_error(context, command, error).await;
    }
    let ticket = match player
        .reserve_play_request(channel_id, command.user.id)
        .await
    {
        Ok(ticket) => ticket,
        Err(error) => return edit_error(context, command, error).await,
    };
    spawn_resolution(context, state, &player, ticket.reservation, query);
    let commit = wait_for_commit(ticket).await;

    match commit {
        Ok(receipt) => edit_success(context, command, receipt).await,
        Err(error) => {
            error!(
                guild_id = %guild_id,
                user_id = %command.user.id,
                error = %error,
                "play command failed"
            );
            edit_error(context, command, error).await
        }
    }
}

fn spawn_resolution(
    context: &Context,
    state: &Arc<AppState>,
    player: &Arc<GuildPlayer>,
    reservation: PlayRequestReservation,
    query: String,
) {
    let resolver = Arc::clone(&state.track_resolver);
    let resolution_task = tokio::spawn(async move { resolver.resolve(&query).await });
    let cache = Arc::clone(&context.cache);
    let state = Arc::clone(state);
    let weak_player = Arc::downgrade(player);
    drop(tokio::spawn(async move {
        let resolution = resolution_task.await.unwrap_or_else(|source| {
            Err(AppError::Internal {
                context: format!(
                    "play resolution task for sequence {} failed: {source}",
                    reservation.sequence
                ),
            })
        });
        let Some(player) = weak_player.upgrade() else {
            return;
        };
        if player
            .publish_play_resolution(reservation, resolution)
            .await
        {
            drain_ready_requests(cache, state, player).await;
        }
    }));
}

async fn wait_for_commit(ticket: PlayRequestTicket) -> Result<PlayCommitReceipt, AppError> {
    ticket.response.await.map_err(|_| AppError::Internal {
        context: format!(
            "play request sequence {} completed without a response",
            ticket.reservation.sequence
        ),
    })?
}

async fn drain_ready_requests(cache: Arc<Cache>, state: Arc<AppState>, player: Arc<GuildPlayer>) {
    while let Some(request) = player.take_next_play_request().await {
        let sequence = request.reservation.sequence;
        let response = request.response;
        let commit = CommitRequest {
            reservation: request.reservation,
            channel_id: request.channel_id,
            requested_by: request.requested_by,
            resolution: request.resolution,
        };
        let result = commit_request(&cache, &state, &player, commit).await;
        if response.send(result).is_err() {
            error!(
                guild_id = %player.guild_id(),
                sequence,
                "play request receiver was dropped"
            );
        }
    }
}

struct CommitRequest {
    reservation: PlayRequestReservation,
    channel_id: ChannelId,
    requested_by: UserId,
    resolution: Result<Vec<ResolvedTrack>, AppError>,
}

async fn commit_request(
    cache: &Cache,
    state: &AppState,
    player: &Arc<GuildPlayer>,
    request: CommitRequest,
) -> Result<PlayCommitReceipt, AppError> {
    validate_commit_state(state, player, request.reservation).await?;
    let tracks = request.resolution?;
    let first_track = tracks.first().cloned().ok_or(AppError::Resolution {
        context: "YouTube resolver returned no tracks".to_owned(),
    })?;

    revalidate_user_channel(
        cache,
        player.guild_id(),
        request.requested_by,
        request.channel_id,
    )?;
    let connected = state
        .voice
        .connect(player.guild_id(), request.channel_id)
        .await?;
    validate_commit_state(state, &connected, request.reservation).await?;
    revalidate_user_channel(
        cache,
        player.guild_id(),
        request.requested_by,
        request.channel_id,
    )?;

    let queued = tracks
        .into_iter()
        .map(|track| QueuedTrack {
            track,
            requested_by: request.requested_by,
        })
        .collect();
    let receipt = state
        .playback
        .enqueue_prefix(connected, queued, request.reservation.session_epoch)
        .await?;
    validate_commit_state(state, player, request.reservation).await?;
    let first_position = receipt.first_position.ok_or(AppError::QueueFull {
        limit: state.config.max_queue_size,
    })?;

    Ok(PlayCommitReceipt {
        first_track,
        requested_by: request.requested_by,
        first_position,
        added: receipt.added,
        omitted: receipt.omitted,
    })
}

async fn validate_commit_state(
    state: &AppState,
    player: &GuildPlayer,
    reservation: PlayRequestReservation,
) -> Result<(), AppError> {
    let is_current = state
        .players
        .get(player.guild_id())
        .await
        .is_some_and(|current| current.instance_id() == player.instance_id());
    let session_is_current = player
        .play_request_session_is_current(reservation.session_epoch)
        .await;
    if is_current && session_is_current {
        return Ok(());
    }

    Err(AppError::Internal {
        context: format!(
            "play request sequence {} belongs to an obsolete guild session",
            reservation.sequence
        ),
    })
}

fn revalidate_user_channel(
    cache: &Cache,
    guild_id: GuildId,
    requested_by: UserId,
    reserved_channel_id: ChannelId,
) -> Result<(), AppError> {
    let current_channel = VoiceConnection::user_channel(cache, guild_id, requested_by)?;
    if current_channel == reserved_channel_id {
        return Ok(());
    }
    Err(AppError::InvalidVoiceChannel(
        VoiceChannelIssue::DifferentChannel,
    ))
}

fn normalized_query(command: &CommandInteraction) -> Result<String, AppError> {
    let query = query_value(command).ok_or(AppError::InvalidInput {
        reason: "play command query is missing or has the wrong type".to_owned(),
    })?;
    normalize_track_input(query).map(str::to_owned)
}

fn query_value(command: &CommandInteraction) -> Option<&str> {
    command
        .data
        .options
        .iter()
        .find(|option| option.name == "query")
        .and_then(|option| match &option.value {
            CommandDataOptionValue::String(value) => Some(value.as_str()),
            _ => None,
        })
}

async fn edit_success(
    context: &Context,
    command: &CommandInteraction,
    receipt: PlayCommitReceipt,
) -> Result<(), serenity::Error> {
    command
        .edit_response(
            &context.http,
            EditInteractionResponse::new().content(success_message(&receipt)),
        )
        .await?;
    Ok(())
}

async fn edit_error(
    context: &Context,
    command: &CommandInteraction,
    error: AppError,
) -> Result<(), serenity::Error> {
    command
        .edit_response(
            &context.http,
            EditInteractionResponse::new().content(error.discord_message()),
        )
        .await?;
    Ok(())
}

fn success_message(receipt: &PlayCommitReceipt) -> String {
    let duration = receipt
        .first_track
        .duration_seconds
        .map(|seconds| format!("\nDuração: {}", format_duration(seconds)))
        .unwrap_or_default();
    let requester = format!("<@{}>", receipt.requested_by);
    let title = truncate_text(&receipt.first_track.title, MAX_TITLE_CHARS);

    let message = if receipt.added == 1 && receipt.omitted == 0 {
        format!(
            "Adicionado à fila: {}{duration}\nSolicitado por: {requester}\nPosição: {}",
            title, receipt.first_position
        )
    } else {
        let omitted = if receipt.omitted > 0 {
            format!("\nOmitidas por limite: {}", receipt.omitted)
        } else {
            String::new()
        };
        format!(
            "Adicionadas à fila: {} músicas\nPrimeira: {}{duration}\nSolicitado por: {requester}\nPrimeira posição: {}{omitted}",
            receipt.added, title, receipt.first_position
        )
    };
    truncate_text(&message, MAX_RESPONSE_CHARS)
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
