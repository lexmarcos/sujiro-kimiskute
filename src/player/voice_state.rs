use serenity::model::id::ChannelId;

use crate::error::{AppError, VoiceChannelIssue};

#[derive(Clone, Copy)]
pub(super) enum VoiceConnectionState {
    Disconnected,
    Connecting {
        operation_id: u64,
        channel_id: ChannelId,
    },
    Connected {
        channel_id: ChannelId,
    },
    Disconnecting {
        operation_id: u64,
        channel_id: ChannelId,
    },
}

#[derive(Clone, Copy)]
pub(crate) struct VoiceOperation {
    pub operation_id: u64,
    pub channel_id: ChannelId,
    pub session_epoch: u64,
}

pub(crate) enum VoiceConnectionClaim {
    Ready,
    Wait { operation_id: u64 },
    Start(VoiceOperation),
}

impl VoiceConnectionState {
    pub(super) fn channel_id(self) -> Option<ChannelId> {
        match self {
            Self::Disconnected => None,
            Self::Connecting { channel_id, .. }
            | Self::Connected { channel_id }
            | Self::Disconnecting { channel_id, .. } => Some(channel_id),
        }
    }

    pub(super) fn operation_id(self) -> Option<u64> {
        match self {
            Self::Connecting { operation_id, .. } | Self::Disconnecting { operation_id, .. } => {
                Some(operation_id)
            }
            Self::Disconnected | Self::Connected { .. } => None,
        }
    }

    pub(super) fn matches_connect(self, operation: VoiceOperation) -> bool {
        matches!(
            self,
            Self::Connecting {
                operation_id,
                channel_id,
            } if operation_id == operation.operation_id && channel_id == operation.channel_id
        )
    }

    pub(super) fn matches_disconnect(self, operation: VoiceOperation) -> bool {
        matches!(
            self,
            Self::Disconnecting {
                operation_id,
                channel_id,
            } if operation_id == operation.operation_id && channel_id == operation.channel_id
        )
    }
}

pub(super) fn existing_connection_claim(
    connection: &VoiceConnectionState,
    channel_id: ChannelId,
) -> Result<Option<VoiceConnectionClaim>, AppError> {
    match *connection {
        VoiceConnectionState::Disconnected => Ok(None),
        VoiceConnectionState::Connected {
            channel_id: connected,
        } if connected == channel_id => Ok(Some(VoiceConnectionClaim::Ready)),
        VoiceConnectionState::Connecting {
            operation_id,
            channel_id: connecting,
        }
        | VoiceConnectionState::Disconnecting {
            operation_id,
            channel_id: connecting,
        } if connecting == channel_id => Ok(Some(VoiceConnectionClaim::Wait { operation_id })),
        VoiceConnectionState::Connecting { .. }
        | VoiceConnectionState::Connected { .. }
        | VoiceConnectionState::Disconnecting { .. } => Err(AppError::InvalidVoiceChannel(
            VoiceChannelIssue::DifferentChannel,
        )),
    }
}
