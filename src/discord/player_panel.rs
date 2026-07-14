use serenity::{
    all::{ButtonStyle, Colour, ReactionType},
    builder::{CreateActionRow, CreateButton, CreateEmbed},
};

use crate::{
    localization::BotLanguage,
    player::{
        guild_player::GuildPlayerSnapshot, playback_state::PlaybackState, track::QueuedTrack,
    },
};

use super::commands::truncate_text;

pub const PREVIOUS_CONTROL_ID: &str = "sujiro:player:previous";
pub const TOGGLE_CONTROL_ID: &str = "sujiro:player:toggle";
pub const SKIP_CONTROL_ID: &str = "sujiro:player:skip";
pub const STOP_CONTROL_ID: &str = "sujiro:player:stop";

const MAX_EMBED_TITLE_CHARS: usize = 256;
const MAX_FIELD_VALUE_CHARS: usize = 1_024;
const MAX_NEXT_TRACKS: usize = 10;
const MAX_QUEUE_TITLE_CHARS: usize = 72;

pub fn now_playing_embed(
    snapshot: &GuildPlayerSnapshot,
    language: BotLanguage,
) -> Option<CreateEmbed> {
    let current = snapshot.current.as_ref()?;
    let track = &current.track;
    let mut embed = CreateEmbed::new()
        .title(truncate_text(&track.title, MAX_EMBED_TITLE_CHARS))
        .url(&track.webpage_url)
        .colour(panel_colour(snapshot.playback_state))
        .field(
            state_field_name(language),
            state_label(snapshot.playback_state, language),
            true,
        )
        .field(
            duration_field_name(language),
            duration_label(track.duration_seconds, language),
            true,
        )
        .field(
            requester_field_name(language),
            format!("<@{}>", current.requested_by),
            true,
        )
        .field(
            channel_field_name(language),
            track
                .channel_name
                .as_deref()
                .unwrap_or_else(|| unavailable_channel(language)),
            false,
        );
    if let Some(thumbnail_url) = &track.thumbnail_url {
        embed = embed.thumbnail(thumbnail_url);
    }
    if let Some(upcoming) = upcoming_tracks(&snapshot.queued) {
        embed = embed.field(upcoming_field_name(language), upcoming, false);
    }
    Some(embed)
}

pub fn control_row(snapshot: &GuildPlayerSnapshot, language: BotLanguage) -> CreateActionRow {
    build_control_row(snapshot, language, false)
}

pub fn disabled_control_row(
    snapshot: &GuildPlayerSnapshot,
    language: BotLanguage,
) -> CreateActionRow {
    build_control_row(snapshot, language, true)
}

fn build_control_row(
    snapshot: &GuildPlayerSnapshot,
    language: BotLanguage,
    panel_inactive: bool,
) -> CreateActionRow {
    let (previous, next, stop) = match language {
        BotLanguage::PtBr => ("Anterior", "Próxima", "Parar"),
        BotLanguage::EnUs => ("Previous", "Next", "Stop"),
    };
    CreateActionRow::Buttons(vec![
        control_button(
            PREVIOUS_CONTROL_ID,
            previous,
            "⏮️",
            ButtonStyle::Secondary,
            panel_inactive,
        ),
        toggle_button(snapshot.playback_state, language, panel_inactive),
        control_button(
            SKIP_CONTROL_ID,
            next,
            "⏭️",
            ButtonStyle::Secondary,
            panel_inactive || snapshot.queued.is_empty(),
        ),
        control_button(
            STOP_CONTROL_ID,
            stop,
            "⏹️",
            ButtonStyle::Danger,
            panel_inactive,
        ),
    ])
}

pub fn upcoming_tracks(queued: &[QueuedTrack]) -> Option<String> {
    if queued.is_empty() {
        return None;
    }
    let mut summary = String::new();
    for (index, track) in queued.iter().take(MAX_NEXT_TRACKS).enumerate() {
        let line = upcoming_line(index + 1, track);
        if !append_line(&mut summary, &line, MAX_FIELD_VALUE_CHARS) {
            break;
        }
    }
    Some(summary)
}

pub fn format_duration(total_seconds: u64) -> String {
    let hours = total_seconds / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        return format!("{hours}:{minutes:02}:{seconds:02}");
    }
    format!("{minutes}:{seconds:02}")
}

pub fn stopped_message(removed_tracks: usize, language: BotLanguage) -> String {
    match (language, removed_tracks) {
        (BotLanguage::PtBr, 0) => "⏹️ Reprodução encerrada. A fila já estava vazia.".to_owned(),
        (BotLanguage::PtBr, 1) => {
            "⏹️ Reprodução encerrada e fila limpa. 1 música removida.".to_owned()
        }
        (BotLanguage::PtBr, count) => {
            format!("⏹️ Reprodução encerrada e fila limpa. {count} músicas removidas.")
        }
        (BotLanguage::EnUs, 0) => "⏹️ Playback stopped. The queue was already empty.".to_owned(),
        (BotLanguage::EnUs, 1) => {
            "⏹️ Playback stopped and queue cleared. 1 track removed.".to_owned()
        }
        (BotLanguage::EnUs, count) => {
            format!("⏹️ Playback stopped and queue cleared. {count} tracks removed.")
        }
    }
}

fn toggle_button(
    playback_state: PlaybackState,
    language: BotLanguage,
    disabled: bool,
) -> CreateButton {
    let (label, emoji) = match (language, playback_state == PlaybackState::Paused) {
        (BotLanguage::PtBr, true) => ("Retomar", "▶️"),
        (BotLanguage::PtBr, false) => ("Pausar", "⏸️"),
        (BotLanguage::EnUs, true) => ("Resume", "▶️"),
        (BotLanguage::EnUs, false) => ("Pause", "⏸️"),
    };
    control_button(
        TOGGLE_CONTROL_ID,
        label,
        emoji,
        ButtonStyle::Primary,
        disabled,
    )
}

fn control_button(
    custom_id: &'static str,
    label: &'static str,
    emoji: &'static str,
    style: ButtonStyle,
    disabled: bool,
) -> CreateButton {
    CreateButton::new(custom_id)
        .label(label)
        .emoji(ReactionType::Unicode(emoji.to_owned()))
        .style(style)
        .disabled(disabled)
}

fn upcoming_line(position: usize, track: &QueuedTrack) -> String {
    let title = truncate_text(&track.track.title, MAX_QUEUE_TITLE_CHARS);
    let duration = track
        .track
        .duration_seconds
        .map(format_duration)
        .map(|value| format!(" · `{value}`"))
        .unwrap_or_default();
    format!(
        "`{position}.` {title}{duration} · <@{}>\n",
        track.requested_by
    )
}

fn append_line(summary: &mut String, line: &str, max_chars: usize) -> bool {
    if summary.chars().count() + line.chars().count() > max_chars {
        return false;
    }
    summary.push_str(line);
    true
}

fn duration_label(duration_seconds: Option<u64>, language: BotLanguage) -> String {
    duration_seconds
        .map(format_duration)
        .unwrap_or_else(|| match language {
            BotLanguage::PtBr => "Não informada".to_owned(),
            BotLanguage::EnUs => "Not provided".to_owned(),
        })
}

fn state_label(playback_state: PlaybackState, language: BotLanguage) -> &'static str {
    match (language, playback_state) {
        (BotLanguage::PtBr, PlaybackState::Idle) => "⏹️ Parada",
        (BotLanguage::PtBr, PlaybackState::Starting) => "⏳ Preparando",
        (BotLanguage::PtBr, PlaybackState::Playing) => "▶️ Tocando",
        (BotLanguage::PtBr, PlaybackState::Paused) => "⏸️ Pausada",
        (BotLanguage::EnUs, PlaybackState::Idle) => "⏹️ Stopped",
        (BotLanguage::EnUs, PlaybackState::Starting) => "⏳ Preparing",
        (BotLanguage::EnUs, PlaybackState::Playing) => "▶️ Playing",
        (BotLanguage::EnUs, PlaybackState::Paused) => "⏸️ Paused",
    }
}

pub fn now_playing_message(language: BotLanguage) -> &'static str {
    match language {
        BotLanguage::PtBr => "🎵 **Tocando agora**",
        BotLanguage::EnUs => "🎵 **Now playing**",
    }
}

fn state_field_name(language: BotLanguage) -> &'static str {
    match language {
        BotLanguage::PtBr => "Estado",
        BotLanguage::EnUs => "Status",
    }
}

fn duration_field_name(language: BotLanguage) -> &'static str {
    match language {
        BotLanguage::PtBr => "Duração",
        BotLanguage::EnUs => "Duration",
    }
}

fn requester_field_name(language: BotLanguage) -> &'static str {
    match language {
        BotLanguage::PtBr => "Solicitada por",
        BotLanguage::EnUs => "Requested by",
    }
}

fn channel_field_name(language: BotLanguage) -> &'static str {
    match language {
        BotLanguage::PtBr => "Canal",
        BotLanguage::EnUs => "Channel",
    }
}

fn unavailable_channel(language: BotLanguage) -> &'static str {
    match language {
        BotLanguage::PtBr => "Não informado",
        BotLanguage::EnUs => "Not provided",
    }
}

fn upcoming_field_name(language: BotLanguage) -> &'static str {
    match language {
        BotLanguage::PtBr => "Próximas",
        BotLanguage::EnUs => "Up next",
    }
}

fn panel_colour(playback_state: PlaybackState) -> Colour {
    match playback_state {
        PlaybackState::Paused => Colour::ORANGE,
        PlaybackState::Idle => Colour::DARK_GREY,
        PlaybackState::Starting | PlaybackState::Playing => Colour::BLURPLE,
    }
}
