use std::time::Duration;

use thiserror::Error;

use crate::{config::ConfigError, localization::BotLanguage};

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
    pub fn discord_message(&self, language: BotLanguage) -> String {
        match language {
            BotLanguage::PtBr => self.discord_message_pt_br(),
            BotLanguage::EnUs => self.discord_message_en_us(),
        }
    }

    fn discord_message_pt_br(&self) -> String {
        match self {
            Self::InvalidInput { .. } => {
                "⚠️ Não entendi essa entrada. Confira o valor e tente novamente.".to_owned()
            }
            Self::QueueFull { limit } => {
                format!("🚧 A fila está cheia (limite: {limit}). Tente novamente mais tarde.")
            }
            Self::InvalidVoiceChannel(issue) => issue.discord_message_pt_br().to_owned(),
            Self::Timeout { .. } => "⏳ A operação demorou demais. Tente novamente.".to_owned(),
            Self::Resolution { .. } => {
                "🔎 Não encontrei esse conteúdo no YouTube. Tente outro nome ou link.".to_owned()
            }
            Self::YtDlp { .. } => {
                "⚠️ Não consegui preparar esse conteúdo. Tente outra música.".to_owned()
            }
            Self::Voice { .. } => {
                "🔊 Não consegui controlar o canal de voz. Tente novamente.".to_owned()
            }
            Self::Discord(_) => "⚠️ Não consegui concluir a ação no Discord.".to_owned(),
            Self::Configuration(_) | Self::Internal { .. } => {
                "⚠️ Algo deu errado por aqui. Tente novamente.".to_owned()
            }
        }
    }

    fn discord_message_en_us(&self) -> String {
        match self {
            Self::InvalidInput { .. } => {
                "⚠️ I couldn't understand that input. Check the value and try again.".to_owned()
            }
            Self::QueueFull { limit } => {
                format!("🚧 The queue is full (limit: {limit}). Try again later.")
            }
            Self::InvalidVoiceChannel(issue) => issue.discord_message_en_us().to_owned(),
            Self::Timeout { .. } => "⏳ The operation took too long. Try again.".to_owned(),
            Self::Resolution { .. } => {
                "🔎 I couldn't find that content on YouTube. Try another name or link.".to_owned()
            }
            Self::YtDlp { .. } => {
                "⚠️ I couldn't prepare that content. Try another track.".to_owned()
            }
            Self::Voice { .. } => "🔊 I couldn't control the voice channel. Try again.".to_owned(),
            Self::Discord(_) => "⚠️ I couldn't complete the action on Discord.".to_owned(),
            Self::Configuration(_) | Self::Internal { .. } => {
                "⚠️ Something went wrong. Try again.".to_owned()
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
    fn discord_message_pt_br(&self) -> &'static str {
        match self {
            Self::UserNotConnected => "🔊 Entre em um canal de voz para controlar a reprodução.",
            Self::DifferentChannel => {
                "🔊 Entre no mesmo canal de voz que eu para controlar a reprodução."
            }
            Self::Unavailable => "🔊 Não consegui identificar seu canal de voz. Tente novamente.",
        }
    }

    fn discord_message_en_us(&self) -> &'static str {
        match self {
            Self::UserNotConnected => "🔊 Join a voice channel to control playback.",
            Self::DifferentChannel => "🔊 Join the same voice channel as me to control playback.",
            Self::Unavailable => "🔊 I couldn't identify your voice channel. Try again.",
        }
    }
}
