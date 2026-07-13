use std::time::Duration;

use thiserror::Error;

use crate::config::ConfigError;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("configuration error: {0}")]
    Configuration(#[from] ConfigError),

    #[error("Discord error: {0}")]
    Discord(#[source] Box<serenity::Error>),

    #[error("voice operation failed: {context}")]
    Voice { context: String },

    #[error("yt-dlp operation failed: {context}")]
    YtDlp { context: String },

    #[error("content resolution failed: {context}")]
    Resolution { context: String },

    #[error("invalid input: {reason}")]
    InvalidInput { reason: String },

    #[error("queue is full (limit: {limit})")]
    QueueFull { limit: usize },

    #[error("invalid voice channel: {0}")]
    InvalidVoiceChannel(VoiceChannelIssue),

    #[error("{operation} timed out after {duration:?}")]
    Timeout {
        operation: &'static str,
        duration: Duration,
    },

    #[error("internal error: {context}")]
    Internal { context: String },
}

impl From<serenity::Error> for AppError {
    fn from(source: serenity::Error) -> Self {
        Self::Discord(Box::new(source))
    }
}

impl AppError {
    pub fn discord_message(&self) -> String {
        match self {
            Self::InvalidInput { .. } => "A entrada informada é inválida.".to_owned(),
            Self::QueueFull { limit } => format!("A fila está cheia (limite: {limit})."),
            Self::InvalidVoiceChannel(issue) => issue.discord_message().to_owned(),
            Self::Timeout { .. } => "A operação demorou demais. Tente novamente.".to_owned(),
            Self::Resolution { .. } => "Não foi possível encontrar esse conteúdo.".to_owned(),
            Self::YtDlp { .. } => "Não foi possível preparar esse conteúdo.".to_owned(),
            Self::Voice { .. } => "Não foi possível controlar o canal de voz.".to_owned(),
            Self::Discord(_) => "Não foi possível concluir a operação no Discord.".to_owned(),
            Self::Configuration(_) | Self::Internal { .. } => {
                "Ocorreu um erro interno. Tente novamente.".to_owned()
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum VoiceChannelIssue {
    #[error("the user is not connected to a voice channel")]
    UserNotConnected,

    #[error("the user is not in the bot's voice channel")]
    DifferentChannel,

    #[error("the voice channel is unavailable")]
    Unavailable,
}

impl VoiceChannelIssue {
    fn discord_message(&self) -> &'static str {
        match self {
            Self::UserNotConnected => "Você precisa estar em um canal de voz.",
            Self::DifferentChannel => "Você precisa estar no mesmo canal de voz que o bot.",
            Self::Unavailable => "Não foi possível identificar o canal de voz.",
        }
    }
}
