use std::sync::Arc;

use serenity::{
    all::{
        Cache, ChannelId, CommandDataOptionValue, CommandInteraction, CommandOptionType, Context,
        GuildId, UserId,
    },
    builder::{
        CreateActionRow, CreateButton, CreateCommand, CreateCommandOption, EditInteractionResponse,
    },
};
use tracing::error;

use crate::{
    error::{AppError, VoiceChannelIssue},
    localization::BotLanguage,
    player::{
        guild_player::{GuildPlayer, GuildPlayerSnapshot},
        play_requests::{PlayCommitReceipt, PlayRequestReservation, PlayRequestTicket},
        track::QueuedTrack,
    },
    sources::resolver::{MAX_TRACK_INPUT_CHARS, TrackInputKind, normalize_track_input},
    state::AppState,
    voice::VoiceConnection,
};

use super::{MAX_RESPONSE_CHARS, guild_only_message, respond, respond_app_error, truncate_text};
use crate::discord::{
    play_requests::CANCEL_PLAY_PREFIX,
    player_panel::{control_row, format_duration, now_playing_embed},
};

const MAX_TITLE_CHARS: usize = 160;

pub fn definition(language: BotLanguage) -> CreateCommand {
    let (description, query_description) = match language {
        BotLanguage::PtBr => (
            "Adiciona uma música à fila",
            "URL do YouTube ou texto para busca",
        ),
        BotLanguage::EnUs => (
            "Adds a track to the queue",
            "YouTube URL or text to search for",
        ),
    };
    let query = CreateCommandOption::new(CommandOptionType::String, "query", query_description)
        .required(true)
        .max_length(MAX_TRACK_INPUT_CHARS);

    CreateCommand::new("play")
        .description(description)
        .add_option(query)
}

pub async fn run(
    context: &Context,
    command: &CommandInteraction,
    state: &Arc<AppState>,
) -> Result<(), serenity::Error> {
    let language = state.config.bot_language;
    let Some(guild_id) = command.guild_id else {
        return respond(context, command, guild_only_message(language), true).await;
    };
    let query = match normalized_query(command) {
        Ok(query) => query,
        Err(error) => return respond_app_error(context, command, language, error).await,
    };
    let channel_id = match VoiceConnection::user_channel(&context.cache, guild_id, command.user.id)
    {
        Ok(channel_id) => channel_id,
        Err(error) => {
            return respond(context, command, &error.discord_message(language), true).await;
        }
    };

    command.defer(&context.http).await?;
    let input_kind = match state.track_resolver.classify(&query) {
        Ok(input_kind) => input_kind,
        Err(error) => return edit_error(context, command, error, language).await,
    };
    let player = match state.players.get_or_create(guild_id).await {
        Ok(player) => player,
        Err(error) => return edit_error(context, command, error, language).await,
    };
    if let Err(error) = state.auto_leave.cancel_for_activity(&player).await {
        return edit_error(context, command, error, language).await;
    }
    let ticket = match player
        .reserve_play_request(channel_id, command.user.id)
        .await
    {
        Ok(ticket) => ticket,
        Err(error) => return edit_error(context, command, error, language).await,
    };
    edit_loading(context, command, ticket.reservation, input_kind, language).await?;
    spawn_resolution(context, state, &player, ticket.reservation, query).await;
    let commit = wait_for_commit(ticket).await;

    match commit {
        Ok(receipt) => {
            let snapshot = player.snapshot().await;
            edit_success(context, command, receipt, &snapshot, language).await
        }
        Err(error) => {
            error!(
                guild_id = %guild_id,
                user_id = %command.user.id,
                error = %error,
                "play command failed"
            );
            edit_error(context, command, error, language).await
        }
    }
}

async fn spawn_resolution(
    context: &Context,
    state: &Arc<AppState>,
    player: &Arc<GuildPlayer>,
    reservation: PlayRequestReservation,
    query: String,
) {
    let resolver = Arc::clone(&state.track_resolver);
    let resolution_task = tokio::spawn(async move { resolver.resolve(&query).await });
    if !player
        .install_play_request_abort(reservation, resolution_task.abort_handle())
        .await
    {
        return;
    }
    let cache = Arc::clone(&context.cache);
    let state = Arc::clone(state);
    let weak_player = Arc::downgrade(player);
    drop(tokio::spawn(async move {
        let resolution = match resolution_task.await {
            Ok(resolution) => resolution,
            Err(source) if source.is_cancelled() => return,
            Err(source) => Err(AppError::Internal {
                context: format!(
                    "play resolution task for sequence {} failed: {source}",
                    reservation.sequence
                ),
            }),
        };
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

async fn edit_loading(
    context: &Context,
    command: &CommandInteraction,
    reservation: PlayRequestReservation,
    input_kind: TrackInputKind,
    language: BotLanguage,
) -> Result<(), serenity::Error> {
    let content = loading_message(input_kind, language);
    let mut response = EditInteractionResponse::new().content(content);
    if input_kind == TrackInputKind::Collection {
        let custom_id = format!(
            "{CANCEL_PLAY_PREFIX}{}:{}",
            reservation.sequence, reservation.session_epoch
        );
        let label = match language {
            BotLanguage::PtBr => "Cancelar",
            BotLanguage::EnUs => "Cancel",
        };
        response = response.components(vec![CreateActionRow::Buttons(vec![
            CreateButton::new(custom_id)
                .label(label)
                .style(serenity::all::ButtonStyle::Secondary),
        ])]);
    }
    command.edit_response(&context.http, response).await?;
    Ok(())
}

fn loading_message(input_kind: TrackInputKind, language: BotLanguage) -> &'static str {
    match (language, input_kind) {
        (BotLanguage::PtBr, TrackInputKind::Collection) => {
            "⏳ Carregando a playlist e verificando as músicas…"
        }
        (BotLanguage::PtBr, _) => "⏳ Procurando e preparando a música…",
        (BotLanguage::EnUs, TrackInputKind::Collection) => {
            "⏳ Loading the playlist and checking its tracks…"
        }
        (BotLanguage::EnUs, _) => "⏳ Finding and preparing the track…",
    }
}

async fn wait_for_commit(ticket: PlayRequestTicket) -> Result<PlayCommitReceipt, AppError> {
    ticket.response.await.map_err(|_| AppError::Internal {
        context: format!(
            "play request sequence {} completed without a response",
            ticket.reservation.sequence
        ),
    })?
}

pub(crate) async fn drain_ready_requests(
    cache: Arc<Cache>,
    state: Arc<AppState>,
    player: Arc<GuildPlayer>,
) {
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
    resolution: Result<crate::sources::resolver::TrackResolution, AppError>,
}

async fn commit_request(
    cache: &Cache,
    state: &AppState,
    player: &Arc<GuildPlayer>,
    request: CommitRequest,
) -> Result<PlayCommitReceipt, AppError> {
    validate_commit_state(state, player, request.reservation).await?;
    let resolution = request.resolution?;
    let first_track = resolution
        .tracks
        .first()
        .cloned()
        .ok_or(AppError::Resolution {
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

    let unavailable = resolution.unavailable;
    let queued = resolution
        .tracks
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
        unavailable,
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
    snapshot: &GuildPlayerSnapshot,
    language: BotLanguage,
) -> Result<(), serenity::Error> {
    let response = match now_playing_embed(snapshot, language) {
        Some(embed) => EditInteractionResponse::new()
            .content(success_message(&receipt, language))
            .embed(embed)
            .components(vec![control_row(snapshot, language)]),
        None => EditInteractionResponse::new()
            .content(success_message(&receipt, language))
            .components(Vec::new()),
    };
    command.edit_response(&context.http, response).await?;
    Ok(())
}

async fn edit_error(
    context: &Context,
    command: &CommandInteraction,
    error: AppError,
    language: BotLanguage,
) -> Result<(), serenity::Error> {
    command
        .edit_response(
            &context.http,
            EditInteractionResponse::new()
                .content(error.discord_message(language))
                .components(Vec::new()),
        )
        .await?;
    Ok(())
}

fn success_message(receipt: &PlayCommitReceipt, language: BotLanguage) -> String {
    let duration = receipt
        .first_track
        .duration_seconds
        .map(|seconds| format!(" · ⏱️ `{}`", format_duration(seconds)))
        .unwrap_or_default();
    let requester = format!("<@{}>", receipt.requested_by);
    let title = truncate_text(&receipt.first_track.title, MAX_TITLE_CHARS);

    let notes = resolution_notes(receipt.unavailable, receipt.omitted, language);
    let message = match (language, receipt.added) {
        (BotLanguage::PtBr, 1) => format!(
            "✅ **{title}** adicionada à fila{duration}.\n📍 Posição: **{}** · Solicitada por {requester}{notes}",
            receipt.first_position
        ),
        (BotLanguage::PtBr, _) => format!(
            "✅ **{} músicas** adicionadas à fila.\n🎵 Primeira: **{title}**{duration}\n📍 Começa na posição **{}** · Solicitada por {requester}{notes}",
            receipt.added, receipt.first_position
        ),
        (BotLanguage::EnUs, 1) => format!(
            "✅ **{title}** added to the queue{duration}.\n📍 Position: **{}** · Requested by {requester}{notes}",
            receipt.first_position
        ),
        (BotLanguage::EnUs, _) => format!(
            "✅ **{} tracks** added to the queue.\n🎵 First: **{title}**{duration}\n📍 Starts at position **{}** · Requested by {requester}{notes}",
            receipt.added, receipt.first_position
        ),
    };
    truncate_text(&message, MAX_RESPONSE_CHARS)
}

fn resolution_notes(unavailable: usize, omitted: usize, language: BotLanguage) -> String {
    let mut notes = String::new();
    if unavailable > 0 {
        let message = match (language, unavailable) {
            (BotLanguage::PtBr, 1) => "1 música indisponível foi ignorada".to_owned(),
            (BotLanguage::PtBr, count) => {
                format!("{count} músicas indisponíveis foram ignoradas")
            }
            (BotLanguage::EnUs, 1) => "1 unavailable track was skipped".to_owned(),
            (BotLanguage::EnUs, count) => {
                format!("{count} unavailable tracks were skipped")
            }
        };
        notes.push_str("\n⚠️ ");
        notes.push_str(&message);
        notes.push('.');
    }
    if omitted > 0 {
        let message = match (language, omitted) {
            (BotLanguage::PtBr, 1) => "1 música não coube no limite da fila".to_owned(),
            (BotLanguage::PtBr, count) => {
                format!("{count} músicas não couberam no limite da fila")
            }
            (BotLanguage::EnUs, 1) => "1 track did not fit within the queue limit".to_owned(),
            (BotLanguage::EnUs, count) => {
                format!("{count} tracks did not fit within the queue limit")
            }
        };
        notes.push_str("\n⚠️ ");
        notes.push_str(&message);
        notes.push('.');
    }
    notes
}
