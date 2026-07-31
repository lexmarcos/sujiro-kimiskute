use std::sync::Arc;

use serenity::{
    all::{ComponentInteraction, Context, GuildId},
    builder::CreateInteractionResponseFollowup,
};
use tracing::{error, info};

use crate::{
    error::AppError,
    localization::BotLanguage,
    player::{
        guild_player::GuildPlayer,
        playback::{PlaybackControlResult, PlaybackPreviousResult, PlaybackSkipResult},
        playback_state::PlaybackState,
    },
    state::AppState,
};

use super::player_panel::{
    PREVIOUS_CONTROL_ID, SKIP_CONTROL_ID, STOP_CONTROL_ID, TOGGLE_CONTROL_ID,
};

#[derive(Clone, Copy)]
enum PlayerControl {
    Previous,
    Toggle,
    Skip,
    Stop,
}

enum ControlOutcome {
    Changed,
    Unchanged(String),
}

struct PlayerControlRequest {
    control: PlayerControl,
    generation: Option<u64>,
}

pub async fn dispatch(
    context: &Context,
    interaction: &ComponentInteraction,
    state: &Arc<AppState>,
) {
    let Some(request) = PlayerControlRequest::from_custom_id(&interaction.data.custom_id) else {
        return;
    };
    let control = request.control;
    log_received(interaction, control);
    if let Err(source) = interaction.defer(&context.http).await {
        log_discord_error(interaction, control, &source, "defer player control");
        return;
    }

    let language = state.config.bot_language;
    if !interaction_is_active(interaction, state, request.generation).await {
        send_feedback(context, interaction, stale_panel_message(language), control).await;
        return;
    }
    let result = run_control(context, interaction, state, control, language).await;
    match result {
        Ok((outcome, _player)) => {
            if let ControlOutcome::Unchanged(feedback) = outcome {
                send_feedback(context, interaction, &feedback, control).await;
            }
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

async fn interaction_is_active(
    interaction: &ComponentInteraction,
    state: &AppState,
    generation: Option<u64>,
) -> bool {
    let (Some(guild_id), Some(generation)) = (interaction.guild_id, generation) else {
        return false;
    };
    state
        .player_panels
        .interaction_is_active(
            guild_id,
            interaction.channel_id,
            interaction.message.id,
            generation,
        )
        .await
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
        PlayerControl::Stop => stop(context, state, guild_id, &player).await,
    }
}

async fn previous(
    state: &AppState,
    player: Arc<GuildPlayer>,
    language: BotLanguage,
) -> Result<ControlOutcome, AppError> {
    let result = state.playback.previous(player).await?;
    let outcome = match (language, result) {
        (BotLanguage::PtBr, PlaybackPreviousResult::NoPrevious) => {
            ControlOutcome::Unchanged("⏮️ Não há uma música anterior no histórico.".to_owned())
        }
        (BotLanguage::PtBr, PlaybackPreviousResult::Started { .. }) => ControlOutcome::Changed,
        (BotLanguage::EnUs, PlaybackPreviousResult::NoPrevious) => {
            ControlOutcome::Unchanged("⏮️ There is no previous track in the history.".to_owned())
        }
        (BotLanguage::EnUs, PlaybackPreviousResult::Started { .. }) => ControlOutcome::Changed,
    };
    Ok(outcome)
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
    Ok(toggle_outcome(result, language))
}

async fn skip(
    state: &AppState,
    player: Arc<GuildPlayer>,
    language: BotLanguage,
) -> Result<ControlOutcome, AppError> {
    let result = state.playback.skip(player).await?;
    let outcome = match (language, result) {
        (BotLanguage::PtBr, PlaybackSkipResult::NoTrack) => {
            ControlOutcome::Unchanged("🎵 Nenhuma música está tocando para pular.".to_owned())
        }
        (BotLanguage::PtBr, PlaybackSkipResult::NoNext) => {
            ControlOutcome::Unchanged("⏭️ Não há próxima música na fila.".to_owned())
        }
        (BotLanguage::PtBr, PlaybackSkipResult::Skipped { .. }) => ControlOutcome::Changed,
        (BotLanguage::EnUs, PlaybackSkipResult::NoTrack) => {
            ControlOutcome::Unchanged("🎵 No track is playing to skip.".to_owned())
        }
        (BotLanguage::EnUs, PlaybackSkipResult::NoNext) => {
            ControlOutcome::Unchanged("⏭️ There is no next track in the queue.".to_owned())
        }
        (BotLanguage::EnUs, PlaybackSkipResult::Skipped { .. }) => ControlOutcome::Changed,
    };
    Ok(outcome)
}

async fn stop(
    context: &Context,
    state: &AppState,
    guild_id: GuildId,
    player: &GuildPlayer,
) -> Result<ControlOutcome, AppError> {
    state.playback.stop(player).await?;
    state
        .auto_leave
        .refresh(Arc::clone(&context.cache), guild_id)
        .await;
    Ok(ControlOutcome::Changed)
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
    send_feedback(
        context,
        interaction,
        &source.discord_message(language),
        control,
    )
    .await;
}

async fn send_feedback(
    context: &Context,
    interaction: &ComponentInteraction,
    feedback: &str,
    control: PlayerControl,
) {
    let builder = CreateInteractionResponseFollowup::new()
        .content(feedback)
        .ephemeral(true);
    if let Err(source) = interaction.create_followup(&context.http, builder).await {
        log_discord_error(
            interaction,
            control,
            &source,
            "send player control feedback",
        );
    }
}

fn toggle_outcome(result: PlaybackControlResult, language: BotLanguage) -> ControlOutcome {
    match (language, result) {
        (BotLanguage::PtBr, PlaybackControlResult::Changed) => ControlOutcome::Changed,
        (BotLanguage::PtBr, PlaybackControlResult::NoTrack) => {
            ControlOutcome::Unchanged("🎵 Nenhuma música está tocando agora.".to_owned())
        }
        (BotLanguage::PtBr, PlaybackControlResult::AlreadyPaused) => {
            ControlOutcome::Unchanged("⏸️ A reprodução já está pausada.".to_owned())
        }
        (BotLanguage::PtBr, PlaybackControlResult::AlreadyPlaying) => {
            ControlOutcome::Unchanged("▶️ A reprodução já está tocando.".to_owned())
        }
        (BotLanguage::EnUs, PlaybackControlResult::Changed) => ControlOutcome::Changed,
        (BotLanguage::EnUs, PlaybackControlResult::NoTrack) => {
            ControlOutcome::Unchanged("🎵 No track is playing right now.".to_owned())
        }
        (BotLanguage::EnUs, PlaybackControlResult::AlreadyPaused) => {
            ControlOutcome::Unchanged("⏸️ Playback is already paused.".to_owned())
        }
        (BotLanguage::EnUs, PlaybackControlResult::AlreadyPlaying) => {
            ControlOutcome::Unchanged("▶️ Playback is already playing.".to_owned())
        }
    }
}

fn stale_panel_message(language: BotLanguage) -> &'static str {
    match language {
        BotLanguage::PtBr => {
            "Este painel não está mais ativo. Use o painel mais recente ou `/queue`."
        }
        BotLanguage::EnUs => {
            "This player panel is no longer active. Use the latest panel or `/queue`."
        }
    }
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

impl PlayerControlRequest {
    fn from_custom_id(custom_id: &str) -> Option<Self> {
        if let Some(control) = PlayerControl::from_base_id(custom_id) {
            return Some(Self {
                control,
                generation: None,
            });
        }
        let (control_id, generation) = custom_id.rsplit_once(':')?;
        let control = PlayerControl::from_base_id(control_id)?;
        Some(Self {
            control,
            generation: generation.parse().ok(),
        })
    }
}

impl PlayerControl {
    fn from_base_id(control_id: &str) -> Option<Self> {
        match control_id {
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
