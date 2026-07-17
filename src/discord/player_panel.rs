use std::{
    collections::HashMap,
    sync::{Arc, Weak},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use serenity::{
    all::{ButtonStyle, ChannelId, Colour, GuildId, MessageId, ReactionType},
    builder::{CreateActionRow, CreateButton, CreateEmbed, EditMessage},
    http::Http,
};
use tokio::{
    sync::{Mutex, OnceCell},
    task::AbortHandle,
    time::MissedTickBehavior,
};
use tracing::warn;
use url::Url;

use crate::{
    localization::BotLanguage,
    player::{
        guild_player::GuildPlayerSnapshot, manager::PlayerManager, observer::PlayerObserver,
        playback_state::PlaybackState, track::QueuedTrack,
    },
};

use super::commands::truncate_text;

pub const PREVIOUS_CONTROL_ID: &str = "sujiro:player:previous";
pub const TOGGLE_CONTROL_ID: &str = "sujiro:player:toggle";
pub const SKIP_CONTROL_ID: &str = "sujiro:player:skip";
pub const STOP_CONTROL_ID: &str = "sujiro:player:stop";

const MAX_EMBED_TITLE_CHARS: usize = 256;
const MAX_FIELD_VALUE_CHARS: usize = 1_024;
const MAX_DETAILED_TRACKS: usize = 10;
const MAX_COMPACT_TRACKS: usize = 3;
const MAX_QUEUE_TITLE_CHARS: usize = 72;
const PROGRESS_SEGMENTS: usize = 12;

#[derive(Clone, Copy)]
pub enum PanelView {
    Compact,
    Detailed,
}

#[derive(Clone)]
struct ActivePanel {
    channel_id: ChannelId,
    message_id: MessageId,
    view: PanelView,
    generation: u64,
    refresh_abort: Option<AbortHandle>,
}

pub struct PlayerPanelService {
    weak_self: Weak<PlayerPanelService>,
    http: OnceCell<Arc<Http>>,
    players: Arc<PlayerManager>,
    language: BotLanguage,
    update_interval: Option<Duration>,
    panels: Mutex<HashMap<GuildId, ActivePanel>>,
}

impl PlayerPanelService {
    pub fn new(
        players: Arc<PlayerManager>,
        language: BotLanguage,
        update_interval: Option<Duration>,
    ) -> Arc<Self> {
        Arc::new_cyclic(|weak_self| Self {
            weak_self: weak_self.clone(),
            http: OnceCell::new(),
            players,
            language,
            update_interval,
            panels: Mutex::new(HashMap::new()),
        })
    }

    pub fn initialize(&self, http: Arc<Http>) -> bool {
        self.http.set(http).is_ok()
    }

    pub async fn register(
        self: &Arc<Self>,
        guild_id: GuildId,
        channel_id: ChannelId,
        message_id: MessageId,
        view: PanelView,
    ) {
        let previous = {
            let mut panels = self.panels.lock().await;
            let generation = panels
                .get(&guild_id)
                .map_or(1, |panel| panel.generation.wrapping_add(1));
            panels.insert(
                guild_id,
                ActivePanel {
                    channel_id,
                    message_id,
                    view,
                    generation,
                    refresh_abort: None,
                },
            )
        };
        if let Some(mut previous) = previous {
            abort_refresh(previous.refresh_abort.take());
            if previous.channel_id != channel_id || previous.message_id != message_id {
                self.disable(&previous).await;
            }
        }
        self.refresh(guild_id).await;
    }

    pub async fn channel_id(&self, guild_id: GuildId) -> Option<ChannelId> {
        self.panels
            .lock()
            .await
            .get(&guild_id)
            .map(|panel| panel.channel_id)
    }

    pub async fn refresh(&self, guild_id: GuildId) {
        self.abort_guild_refresh(guild_id).await;
        let Some(http) = self.http.get() else {
            return;
        };
        let Some(panel) = self.panel(guild_id).await else {
            return;
        };
        let Some(player) = self.players.get(guild_id).await else {
            self.disable(&panel).await;
            self.remove_if_same(guild_id, panel).await;
            return;
        };
        let snapshot = player.snapshot().await;
        let builder = panel_message(
            &snapshot,
            panel.view,
            self.language,
            self.update_interval.is_some(),
        );
        if let Err(source) = panel
            .channel_id
            .edit_message(http, panel.message_id, builder)
            .await
        {
            warn!(
                guild_id = %guild_id,
                channel_id = %panel.channel_id,
                message_id = %panel.message_id,
                error = %source,
                "failed to refresh active player panel"
            );
            self.remove_if_same(guild_id, panel).await;
            return;
        }
        if snapshot.playback_state == PlaybackState::Playing
            && snapshot.current.is_some()
            && self.update_interval.is_some()
        {
            self.start_refresh_task(guild_id, panel.generation).await;
        }
    }

    async fn panel(&self, guild_id: GuildId) -> Option<ActivePanel> {
        self.panels
            .lock()
            .await
            .get(&guild_id)
            .map(|panel| ActivePanel {
                channel_id: panel.channel_id,
                message_id: panel.message_id,
                view: panel.view,
                generation: panel.generation,
                refresh_abort: None,
            })
    }

    async fn abort_guild_refresh(&self, guild_id: GuildId) {
        let abort_handle = self
            .panels
            .lock()
            .await
            .get_mut(&guild_id)
            .and_then(|panel| panel.refresh_abort.take());
        abort_refresh(abort_handle);
    }

    async fn start_refresh_task(&self, guild_id: GuildId, generation: u64) {
        let Some(update_interval) = self.update_interval else {
            return;
        };
        let weak_service = self.weak_self.clone();
        let task = tokio::spawn(async move {
            run_refresh_loop(weak_service, guild_id, generation, update_interval).await;
        });
        let abort_handle = task.abort_handle();
        let mut panels = self.panels.lock().await;
        let Some(panel) = panels.get_mut(&guild_id) else {
            abort_handle.abort();
            return;
        };
        if panel.generation != generation {
            abort_handle.abort();
            return;
        }
        panel.refresh_abort = Some(abort_handle);
    }

    async fn refresh_generation(&self, guild_id: GuildId, generation: u64) -> RefreshLoopControl {
        let Some(http) = self.http.get() else {
            return RefreshLoopControl::Stop;
        };
        let Some(panel) = self.panel(guild_id).await else {
            return RefreshLoopControl::Stop;
        };
        if panel.generation != generation {
            return RefreshLoopControl::Stop;
        }
        let Some(player) = self.players.get(guild_id).await else {
            self.remove_if_same(guild_id, panel).await;
            return RefreshLoopControl::Stop;
        };
        let snapshot = player.snapshot().await;
        if snapshot.playback_state != PlaybackState::Playing || snapshot.current.is_none() {
            return RefreshLoopControl::Stop;
        }
        let builder = panel_message(
            &snapshot,
            panel.view,
            self.language,
            self.update_interval.is_some(),
        );
        if let Err(source) = panel
            .channel_id
            .edit_message(http, panel.message_id, builder)
            .await
        {
            warn!(
                guild_id = %guild_id,
                channel_id = %panel.channel_id,
                message_id = %panel.message_id,
                error = %source,
                "failed to refresh player progress"
            );
            self.remove_if_same(guild_id, panel).await;
            return RefreshLoopControl::Stop;
        }
        RefreshLoopControl::Continue
    }

    async fn disable(&self, panel: &ActivePanel) {
        let Some(http) = self.http.get() else {
            return;
        };
        if let Err(source) = panel
            .channel_id
            .edit_message(
                http,
                panel.message_id,
                EditMessage::new().components(Vec::new()),
            )
            .await
        {
            warn!(
                channel_id = %panel.channel_id,
                message_id = %panel.message_id,
                error = %source,
                "failed to disable stale player panel"
            );
        }
    }

    async fn remove_if_same(&self, guild_id: GuildId, panel: ActivePanel) {
        let removed = {
            let mut panels = self.panels.lock().await;
            let matches = panels.get(&guild_id).is_some_and(|current| {
                current.channel_id == panel.channel_id
                    && current.message_id == panel.message_id
                    && current.generation == panel.generation
            });
            matches.then(|| panels.remove(&guild_id)).flatten()
        };
        if let Some(removed) = removed {
            abort_refresh(removed.refresh_abort);
        }
    }
}

enum RefreshLoopControl {
    Continue,
    Stop,
}

async fn run_refresh_loop(
    service: Weak<PlayerPanelService>,
    guild_id: GuildId,
    generation: u64,
    update_interval: Duration,
) {
    let mut interval = tokio::time::interval(update_interval);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    interval.tick().await;
    loop {
        interval.tick().await;
        let Some(service) = service.upgrade() else {
            return;
        };
        if matches!(
            service.refresh_generation(guild_id, generation).await,
            RefreshLoopControl::Stop
        ) {
            return;
        }
    }
}

fn abort_refresh(abort_handle: Option<AbortHandle>) {
    if let Some(abort_handle) = abort_handle {
        abort_handle.abort();
    }
}

#[async_trait]
impl PlayerObserver for PlayerPanelService {
    async fn player_changed(&self, guild_id: GuildId) {
        self.refresh(guild_id).await;
    }

    async fn track_failed(&self, guild_id: GuildId, track: &QueuedTrack) {
        let Some(http) = self.http.get() else {
            return;
        };
        let Some(channel_id) = self.channel_id(guild_id).await else {
            return;
        };
        let title = truncate_text(&track.track.title, MAX_QUEUE_TITLE_CHARS);
        let message = match self.language {
            BotLanguage::PtBr => {
                format!("⚠️ Não consegui tocar **{title}** e avancei para a próxima música.")
            }
            BotLanguage::EnUs => {
                format!("⚠️ I couldn't play **{title}** and moved to the next track.")
            }
        };
        if let Err(source) = channel_id.say(http, message).await {
            warn!(
                guild_id = %guild_id,
                channel_id = %channel_id,
                track_id = %track.track.id,
                error = %source,
                "failed to report skipped playback failure"
            );
        }
    }
}

pub fn now_playing_embed(
    snapshot: &GuildPlayerSnapshot,
    language: BotLanguage,
    progress_enabled: bool,
) -> Option<CreateEmbed> {
    now_playing_embed_for_view(snapshot, PanelView::Detailed, language, progress_enabled)
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

pub fn upcoming_tracks(queued: &[QueuedTrack]) -> Option<String> {
    upcoming_tracks_with_limit(queued, MAX_DETAILED_TRACKS, true)
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

pub fn now_playing_message(language: BotLanguage) -> &'static str {
    match language {
        BotLanguage::PtBr => "🎵 **Tocando agora**",
        BotLanguage::EnUs => "🎵 **Now playing**",
    }
}

fn panel_message(
    snapshot: &GuildPlayerSnapshot,
    view: PanelView,
    language: BotLanguage,
    progress_enabled: bool,
) -> EditMessage {
    match now_playing_embed_for_view(snapshot, view, language, progress_enabled) {
        Some(embed) => EditMessage::new()
            .content(now_playing_message(language))
            .embed(embed)
            .components(vec![control_row(snapshot, language)]),
        None => EditMessage::new()
            .content(idle_panel_message(language))
            .embeds(Vec::new())
            .components(vec![disabled_control_row(snapshot, language)]),
    }
}

fn now_playing_embed_for_view(
    snapshot: &GuildPlayerSnapshot,
    view: PanelView,
    language: BotLanguage,
    progress_enabled: bool,
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
    if let Some(thumbnail_url) = valid_thumbnail_url(track.thumbnail_url.as_deref()) {
        embed = embed.thumbnail(thumbnail_url);
    }
    if progress_enabled && let Some(progress) = progress_label(snapshot, language) {
        embed = embed.field(progress_field_name(language), progress, false);
    }
    let (limit, include_requester) = match view {
        PanelView::Compact => (MAX_COMPACT_TRACKS, false),
        PanelView::Detailed => (MAX_DETAILED_TRACKS, true),
    };
    if let Some(upcoming) = upcoming_tracks_with_limit(&snapshot.queued, limit, include_requester) {
        embed = embed.field(upcoming_field_name(language), upcoming, false);
    }
    Some(embed)
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
    let no_active_track = !matches!(
        snapshot.playback_state,
        PlaybackState::Playing | PlaybackState::Paused
    );
    CreateActionRow::Buttons(vec![
        control_button(
            PREVIOUS_CONTROL_ID,
            previous,
            "⏮️",
            ButtonStyle::Secondary,
            panel_inactive || !snapshot.has_previous,
        ),
        toggle_button(
            snapshot.playback_state,
            language,
            panel_inactive || no_active_track,
        ),
        control_button(
            SKIP_CONTROL_ID,
            next,
            "⏭️",
            ButtonStyle::Secondary,
            panel_inactive || no_active_track || snapshot.queued.is_empty(),
        ),
        control_button(
            STOP_CONTROL_ID,
            stop,
            "⏹️",
            ButtonStyle::Danger,
            panel_inactive || (snapshot.current.is_none() && snapshot.queued.is_empty()),
        ),
    ])
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

fn upcoming_tracks_with_limit(
    queued: &[QueuedTrack],
    limit: usize,
    include_requester: bool,
) -> Option<String> {
    if queued.is_empty() {
        return None;
    }
    let mut summary = String::new();
    for (index, track) in queued.iter().take(limit).enumerate() {
        let line = upcoming_line(index + 1, track, include_requester);
        if !append_line(&mut summary, &line, MAX_FIELD_VALUE_CHARS) {
            break;
        }
    }
    Some(summary)
}

fn upcoming_line(position: usize, track: &QueuedTrack, include_requester: bool) -> String {
    let title = truncate_text(&track.track.title, MAX_QUEUE_TITLE_CHARS);
    let duration = track
        .track
        .duration_seconds
        .map(format_duration)
        .map(|value| format!(" · `{value}`"))
        .unwrap_or_default();
    let requester = if include_requester {
        format!(" · <@{}>", track.requested_by)
    } else {
        String::new()
    };
    format!("`{position}.` {title}{duration}{requester}\n")
}

fn progress_label(snapshot: &GuildPlayerSnapshot, language: BotLanguage) -> Option<String> {
    let current = snapshot.current.as_ref()?;
    let position = snapshot.position_seconds?;
    let Some(duration) = current.track.duration_seconds else {
        return Some(match language {
            BotLanguage::PtBr => format!("Decorrido: `{}`", format_duration(position)),
            BotLanguage::EnUs => format!("Elapsed: `{}`", format_duration(position)),
        });
    };
    let position = position.min(duration);
    let progress_bar = progress_bar(position, duration);
    let timing = format!(
        "{progress_bar} `{}` / `{}`",
        format_duration(position),
        format_duration(duration)
    );
    if snapshot.playback_state != PlaybackState::Playing || position >= duration {
        return Some(timing);
    }
    let end_timestamp = unix_timestamp_after(duration - position)?;
    let end_label = match language {
        BotLanguage::PtBr => "Termina",
        BotLanguage::EnUs => "Ends",
    };
    Some(format!("{timing}\n{end_label} <t:{end_timestamp}:R>"))
}

fn progress_bar(position: u64, duration: u64) -> String {
    if duration == 0 {
        return format!("●{}", "─".repeat(PROGRESS_SEGMENTS));
    }
    let marker = ((position as u128 * PROGRESS_SEGMENTS as u128) / duration as u128)
        .min(PROGRESS_SEGMENTS as u128) as usize;
    let mut bar = String::with_capacity(PROGRESS_SEGMENTS * 3);
    for index in 0..=PROGRESS_SEGMENTS {
        bar.push(if index == marker { '●' } else { '─' });
    }
    bar
}

fn unix_timestamp_after(seconds: u64) -> Option<u64> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    now.checked_add(seconds)
}

fn valid_thumbnail_url(value: Option<&str>) -> Option<&str> {
    let value = value?;
    let parsed = Url::parse(value).ok()?;
    matches!(parsed.scheme(), "http" | "https")
        .then_some(value)
        .filter(|_| parsed.host_str().is_some())
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

fn idle_panel_message(language: BotLanguage) -> &'static str {
    match language {
        BotLanguage::PtBr => "🎵 A reprodução terminou. Use `/play` para adicionar uma música.",
        BotLanguage::EnUs => "🎵 Playback finished. Use `/play` to add a track.",
    }
}

fn state_field_name(language: BotLanguage) -> &'static str {
    match language {
        BotLanguage::PtBr => "Estado",
        BotLanguage::EnUs => "Status",
    }
}

fn progress_field_name(language: BotLanguage) -> &'static str {
    match language {
        BotLanguage::PtBr => "Progresso",
        BotLanguage::EnUs => "Progress",
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
