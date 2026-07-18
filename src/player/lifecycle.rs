use serenity::model::id::GuildId;
use songbird::tracks::TrackHandle;
use tokio::task::AbortHandle;

use crate::{error::AppError, player::track::QueuedTrack};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum PlayerLifecycle {
    Active,
    Closing,
}

pub(crate) enum LeaveClaim {
    AlreadyClosing,
    Ready(LeaveOperation),
}

pub(crate) struct LeaveOperation {
    pub track: Option<QueuedTrack>,
    pub handle: Option<TrackHandle>,
    pub removed_from_queue: usize,
    pub session_epoch: u64,
    pub auto_leave_abort: Option<AbortHandle>,
    pub idle_leave_abort: Option<AbortHandle>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct AutoLeaveToken {
    pub generation: u64,
    pub channel_id: serenity::model::id::ChannelId,
}

pub(super) struct AutoLeaveTimer {
    pub token: AutoLeaveToken,
    pub abort_handle: Option<AbortHandle>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct IdleLeaveToken {
    pub generation: u64,
}

pub(super) struct IdleLeaveTimer {
    pub token: IdleLeaveToken,
    pub abort_handle: Option<AbortHandle>,
}

pub(crate) struct IdleLeaveCancellation {
    pub abort_handle: Option<AbortHandle>,
    pub canceled: bool,
}

pub(crate) struct AutoLeaveCancellation {
    pub abort_handle: Option<AbortHandle>,
    pub canceled: bool,
}

pub(super) fn closing_error(guild_id: GuildId) -> AppError {
    AppError::Voice {
        context: format!("guild {guild_id} player session is closing"),
    }
}
