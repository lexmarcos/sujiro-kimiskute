use std::sync::Arc;

use serenity::{
    all::{ComponentInteraction, Context, GuildId},
    builder::EditInteractionResponse,
};
use tracing::{error, info};

use crate::{
    error::AppError,
    localization::BotLanguage,
    player::{
        guild_player::GuildPlayer,
        playback::{PlaybackControlResult, PlaybackPreviousResult, PlaybackSkipResult},
        playback_state::PlaybackState,
        track::QueuedTrack,
    },
    state::AppState,
};

use super::{
    commands::truncate_text,
    player_panel::{
        PREVIOUS_CONTROL_ID, SKIP_CONTROL_ID, STOP_CONTROL_ID, TOGGLE_CONTROL_ID, stopped_message,
    },
};

const MAX_FEEDBACK_TITLE_CHARS: usize = 100;

#[derive(Clone, Copy)]
enum PlayerControl {
    Previous,
    Toggle,
    Skip,
    Stop,
}

struct ControlOutcome {
    feedback: String,
}

pub async fn dispatch(
    context: &Context,
    interaction: &ComponentInteraction,
    state: &Arc<AppState>,
) {
    let Some(control) = PlayerControl::from_custom_id(&interaction.data.custom_id) else {
        return;
    };
    log_received(interaction, control);
    if let Err(source) = interaction.defer_ephemeral(&context.http).await {
        log_discord_error(interaction, control, &source, "defer player control");
        return;
    }

    let language = state.config.bot_language;
    let result = run_control(context, interaction, state, control, language).await;
    match result {
        Ok((outcome, _player)) => {
            if let Some(guild_id) = interaction.guild_id {
                state.player_panels.refresh(guild_id).await;
            }
            edit_feedback(context, interaction, &outcome.feedback, control).await;
            info!(
                guild_id = ?interaction.guild_id,
                user_id = %interaction.user.id,
                control = control.name(),
                "player control completed"
            );
        }
        Err(source) => respond_error(context, interaction, source, control, language).await,
    }
}

async fn run_control(
    context: &Context,
    interaction: &ComponentInteraction,
    state: &Arc<AppState>,
    control: PlayerControl,
    language: BotLanguage,
) -> Result<(ControlOutcome, Arc<GuildPlayer>), AppError> {
    let guild_id = interaction.guild_id.ok_or(AppError::InvalidInput {
        reason: "player control was used outside a guild".to_owned(),
    })?;
    let player = state
        .voice
        .ensure_same_channel(&context.cache, guild_id, interaction.user.id)
        .await?;
    let outcome = execute_control(
        context,
        state,
        guild_id,
        Arc::clone(&player),
        control,
        language,
    )
    .await?;
    Ok((outcome, player))
}

async fn execute_control(
    context: &Context,
    state: &Arc<AppState>,
    guild_id: GuildId,
    player: Arc<GuildPlayer>,
    control: PlayerControl,
    language: BotLanguage,
) -> Result<ControlOutcome, AppError> {
    match control {
        PlayerControl::Previous => previous(state, player, language).await,
        PlayerControl::Toggle => toggle(state, &player, language).await,
        PlayerControl::Skip => skip(state, player, language).await,
        PlayerControl::Stop => stop(context, state, guild_id, &player, language).await,
    }
}

async fn previous(
    state: &AppState,
    player: Arc<GuildPlayer>,
    language: BotLanguage,
) -> Result<ControlOutcome, AppError> {
    let result = state.playback.previous(player).await?;
    let feedback = match (language, result) {
        (BotLanguage::PtBr, PlaybackPreviousResult::NoPrevious) => {
            "⏮️ Não há uma música anterior no histórico.".to_owned()
        }
        (BotLanguage::PtBr, PlaybackPreviousResult::Started { track }) => {
            format!("⏮️ Voltando para **{}**.", feedback_title(&track))
        }
        (BotLanguage::EnUs, PlaybackPreviousResult::NoPrevious) => {
            "⏮️ There is no previous track in the history.".to_owned()
        }
        (BotLanguage::EnUs, PlaybackPreviousResult::Started { track }) => {
            format!("⏮️ Going back to **{}**.", feedback_title(&track))
        }
    };
    Ok(ControlOutcome { feedback })
}

async fn toggle(
    state: &AppState,
    player: &GuildPlayer,
    language: BotLanguage,
) -> Result<ControlOutcome, AppError> {
    let was_paused = player.playback_state().await == PlaybackState::Paused;
    let result = if was_paused {
        state.playback.resume(player).await?
    } else {
        state.playback.pause(player).await?
    };
    Ok(ControlOutcome {
        feedback: toggle_feedback(result, was_paused, language),
    })
}

async fn skip(
    state: &AppState,
    player: Arc<GuildPlayer>,
    language: BotLanguage,
) -> Result<ControlOutcome, AppError> {
    let result = state.playback.skip(player).await?;
    let feedback = match (language, result) {
        (BotLanguage::PtBr, PlaybackSkipResult::NoTrack) => {
            "🎵 Nenhuma música está tocando para pular.".to_owned()
        }
        (BotLanguage::PtBr, PlaybackSkipResult::NoNext) => {
            "⏭️ Não há próxima música na fila.".to_owned()
        }
        (BotLanguage::PtBr, PlaybackSkipResult::Skipped { track }) => {
            format!("⏭️ **{}** foi pulada.", feedback_title(&track))
        }
        (BotLanguage::EnUs, PlaybackSkipResult::NoTrack) => {
            "🎵 No track is playing to skip.".to_owned()
        }
        (BotLanguage::EnUs, PlaybackSkipResult::NoNext) => {
            "⏭️ There is no next track in the queue.".to_owned()
        }
        (BotLanguage::EnUs, PlaybackSkipResult::Skipped { track }) => {
            format!("⏭️ Skipped **{}**.", feedback_title(&track))
        }
    };
    Ok(ControlOutcome { feedback })
}

async fn stop(
    context: &Context,
    state: &AppState,
    guild_id: GuildId,
    player: &GuildPlayer,
    language: BotLanguage,
) -> Result<ControlOutcome, AppError> {
    let stopped = state.playback.stop(player).await?;
    state
        .auto_leave
        .refresh(Arc::clone(&context.cache), guild_id)
        .await;
    Ok(ControlOutcome {
        feedback: stopped_message(stopped.removed_tracks, language),
    })
}

async fn respond_error(
    context: &Context,
    interaction: &ComponentInteraction,
    source: AppError,
    control: PlayerControl,
    language: BotLanguage,
) {
    error!(
        guild_id = ?interaction.guild_id,
        user_id = %interaction.user.id,
        control = control.name(),
        error = %source,
        "player control operation failed"
    );
    edit_feedback(
        context,
        interaction,
        &source.discord_message(language),
        control,
    )
    .await;
}

async fn edit_feedback(
    context: &Context,
    interaction: &ComponentInteraction,
    feedback: &str,
    control: PlayerControl,
) {
    let builder = EditInteractionResponse::new().content(feedback);
    if let Err(source) = interaction.edit_response(&context.http, builder).await {
        log_discord_error(
            interaction,
            control,
            &source,
            "edit player control response",
        );
    }
}

fn toggle_feedback(
    result: PlaybackControlResult,
    was_paused: bool,
    language: BotLanguage,
) -> String {
    match (language, result, was_paused) {
        (BotLanguage::PtBr, PlaybackControlResult::Changed, true) => {
            "▶️ Reprodução retomada.".to_owned()
        }
        (BotLanguage::PtBr, PlaybackControlResult::Changed, false) => {
            "⏸️ Reprodução pausada.".to_owned()
        }
        (BotLanguage::PtBr, PlaybackControlResult::NoTrack, _) => {
            "🎵 Nenhuma música está tocando agora.".to_owned()
        }
        (BotLanguage::PtBr, PlaybackControlResult::AlreadyPaused, _) => {
            "⏸️ A reprodução já está pausada.".to_owned()
        }
        (BotLanguage::PtBr, PlaybackControlResult::AlreadyPlaying, _) => {
            "▶️ A reprodução já está tocando.".to_owned()
        }
        (BotLanguage::EnUs, PlaybackControlResult::Changed, true) => {
            "▶️ Playback resumed.".to_owned()
        }
        (BotLanguage::EnUs, PlaybackControlResult::Changed, false) => {
            "⏸️ Playback paused.".to_owned()
        }
        (BotLanguage::EnUs, PlaybackControlResult::NoTrack, _) => {
            "🎵 No track is playing right now.".to_owned()
        }
        (BotLanguage::EnUs, PlaybackControlResult::AlreadyPaused, _) => {
            "⏸️ Playback is already paused.".to_owned()
        }
        (BotLanguage::EnUs, PlaybackControlResult::AlreadyPlaying, _) => {
            "▶️ Playback is already playing.".to_owned()
        }
    }
}

fn feedback_title(track: &QueuedTrack) -> String {
    truncate_text(&track.track.title, MAX_FEEDBACK_TITLE_CHARS)
}

fn log_received(interaction: &ComponentInteraction, control: PlayerControl) {
    info!(
        guild_id = ?interaction.guild_id,
        user_id = %interaction.user.id,
        control = control.name(),
        "player control received"
    );
}

fn log_discord_error(
    interaction: &ComponentInteraction,
    control: PlayerControl,
    source: &serenity::Error,
    operation: &'static str,
) {
    error!(
        guild_id = ?interaction.guild_id,
        user_id = %interaction.user.id,
        control = control.name(),
        error = %source,
        operation,
        "player control Discord response failed"
    );
}

impl PlayerControl {
    fn from_custom_id(custom_id: &str) -> Option<Self> {
        match custom_id {
            PREVIOUS_CONTROL_ID => Some(Self::Previous),
            TOGGLE_CONTROL_ID => Some(Self::Toggle),
            SKIP_CONTROL_ID => Some(Self::Skip),
            STOP_CONTROL_ID => Some(Self::Stop),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Previous => "previous",
            Self::Toggle => "toggle",
            Self::Skip => "skip",
            Self::Stop => "stop",
        }
    }
}
