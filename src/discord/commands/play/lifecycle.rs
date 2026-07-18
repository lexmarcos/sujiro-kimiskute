use std::sync::Arc;

use serenity::all::{Cache, Context};
use tracing::error;

use super::{CommitRequest, commit_request};
use crate::{
    player::{guild_player::GuildPlayer, play_requests::PendingPlayRequest},
    state::AppState,
};

pub(crate) fn spawn_drainer(context: &Context, state: &Arc<AppState>, player: Arc<GuildPlayer>) {
    let cache = Arc::clone(&context.cache);
    let state = Arc::clone(state);
    tokio::spawn(async move {
        drain_ready_requests(cache, state, player).await;
    });
}

pub(crate) async fn drain_ready_requests(
    cache: Arc<Cache>,
    state: Arc<AppState>,
    player: Arc<GuildPlayer>,
) {
    while let Some(request) = player.take_next_play_request().await {
        commit_ready_request(&cache, &state, &player, request).await;
    }
    refresh_if_quiescent_with_cache(cache, &state, &player).await;
}

async fn commit_ready_request(
    cache: &Cache,
    state: &AppState,
    player: &Arc<GuildPlayer>,
    request: PendingPlayRequest,
) {
    let sequence = request.reservation.sequence;
    let response = request.response;
    let commit = CommitRequest {
        reservation: request.reservation,
        channel_id: request.channel_id,
        requested_by: request.requested_by,
        resolution: request.resolution,
    };
    let result = commit_request(cache, state, player, commit).await;
    if response.send(result).is_err() {
        error!(
            guild_id = %player.guild_id(),
            sequence,
            "play request receiver was dropped"
        );
    }
}

pub(crate) async fn refresh_if_quiescent(
    context: &Context,
    state: &Arc<AppState>,
    player: &Arc<GuildPlayer>,
) {
    refresh_if_quiescent_with_cache(Arc::clone(&context.cache), state, player).await;
}

async fn refresh_if_quiescent_with_cache(
    cache: Arc<Cache>,
    state: &AppState,
    player: &Arc<GuildPlayer>,
) {
    if player.has_outstanding_play_requests().await {
        return;
    }
    state.idle_leave.refresh(player.guild_id()).await;
    state.auto_leave.refresh(cache, player.guild_id()).await;
}
