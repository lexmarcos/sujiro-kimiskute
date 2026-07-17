use std::sync::Arc;

use serenity::{
    all::{ComponentInteraction, Context},
    builder::{EditInteractionResponse, EditMessage},
};
use tracing::{error, info};

use crate::{
    localization::BotLanguage,
    player::play_requests::{PlayRequestCancellation, PlayRequestReservation},
    state::AppState,
};

use super::commands::play::drain_ready_requests;

pub const CANCEL_PLAY_PREFIX: &str = "sujiro:play:cancel:";

pub async fn dispatch(
    context: &Context,
    interaction: &ComponentInteraction,
    state: &Arc<AppState>,
) -> bool {
    let Some(reservation) = parse_reservation(&interaction.data.custom_id) else {
        return false;
    };
    if let Err(source) = interaction.defer_ephemeral(&context.http).await {
        error!(
            guild_id = ?interaction.guild_id,
            user_id = %interaction.user.id,
            error = %source,
            "failed to defer play cancellation"
        );
        return true;
    }

    let language = state.config.bot_language;
    let result = cancel_request(context, interaction, state, reservation).await;
    let message = match result {
        CancelResult::Canceled => canceled_message(language),
        CancelResult::Forbidden => forbidden_message(language),
        CancelResult::NotFound => unavailable_message(language),
    };
    if let Err(source) = interaction
        .edit_response(
            &context.http,
            EditInteractionResponse::new().content(message),
        )
        .await
    {
        error!(
            guild_id = ?interaction.guild_id,
            user_id = %interaction.user.id,
            error = %source,
            "failed to respond to play cancellation"
        );
    }
    true
}

enum CancelResult {
    Canceled,
    Forbidden,
    NotFound,
}

async fn cancel_request(
    context: &Context,
    interaction: &ComponentInteraction,
    state: &Arc<AppState>,
    reservation: PlayRequestReservation,
) -> CancelResult {
    let Some(guild_id) = interaction.guild_id else {
        return CancelResult::NotFound;
    };
    let Some(player) = state.players.get(guild_id).await else {
        return CancelResult::NotFound;
    };
    match player
        .cancel_play_request(reservation, interaction.user.id)
        .await
    {
        PlayRequestCancellation::Canceled { should_drain } => {
            disable_cancel_button(context, interaction).await;
            if should_drain {
                let cache = Arc::clone(&context.cache);
                let state = Arc::clone(state);
                tokio::spawn(async move {
                    drain_ready_requests(cache, state, player).await;
                });
            }
            info!(
                guild_id = %guild_id,
                user_id = %interaction.user.id,
                sequence = reservation.sequence,
                "play request canceled"
            );
            CancelResult::Canceled
        }
        PlayRequestCancellation::Forbidden => CancelResult::Forbidden,
        PlayRequestCancellation::NotFound => CancelResult::NotFound,
    }
}

async fn disable_cancel_button(context: &Context, interaction: &ComponentInteraction) {
    let result = interaction
        .channel_id
        .edit_message(
            &context.http,
            interaction.message.id,
            EditMessage::new().components(Vec::new()),
        )
        .await;
    if let Err(source) = result {
        error!(
            guild_id = ?interaction.guild_id,
            user_id = %interaction.user.id,
            error = %source,
            "failed to disable canceled play request button"
        );
    }
}

fn parse_reservation(custom_id: &str) -> Option<PlayRequestReservation> {
    let encoded = custom_id.strip_prefix(CANCEL_PLAY_PREFIX)?;
    let (sequence, session_epoch) = encoded.split_once(':')?;
    Some(PlayRequestReservation {
        sequence: sequence.parse().ok()?,
        session_epoch: session_epoch.parse().ok()?,
    })
}

fn canceled_message(language: BotLanguage) -> &'static str {
    match language {
        BotLanguage::PtBr => "🚫 Playlist cancelada.",
        BotLanguage::EnUs => "🚫 Playlist canceled.",
    }
}

fn forbidden_message(language: BotLanguage) -> &'static str {
    match language {
        BotLanguage::PtBr => "🔒 Só quem pediu a playlist pode cancelar.",
        BotLanguage::EnUs => "🔒 Only the requester can cancel this playlist.",
    }
}

fn unavailable_message(language: BotLanguage) -> &'static str {
    match language {
        BotLanguage::PtBr => "ℹ️ Este pedido já terminou ou foi cancelado.",
        BotLanguage::EnUs => "ℹ️ This request already finished or was canceled.",
    }
}
