use std::sync::Arc;

use songbird::Songbird;
use tokio::sync::Semaphore;

use crate::{
    config::AppConfig,
    discord::player_panel::PlayerPanelService,
    error::AppError,
    player::{
        auto_leave::AutoLeaveService, manager::PlayerManager, observer::PlayerObserver,
        playback::PlaybackService, session::GuildSessionService,
    },
    sources::{
        TrackResolver,
        youtube::{YoutubeResolver, process::YoutubeProcess},
    },
    voice::VoiceConnection,
};

pub struct AppState {
    pub config: Arc<AppConfig>,
    pub http_client: reqwest::Client,
    pub players: Arc<PlayerManager>,
    pub resolution_slots: Arc<Semaphore>,
    pub track_resolver: Arc<dyn TrackResolver>,
    pub songbird: Arc<Songbird>,
    pub voice: Arc<VoiceConnection>,
    pub playback: Arc<PlaybackService>,
    pub player_panels: Arc<PlayerPanelService>,
    pub sessions: Arc<GuildSessionService>,
    pub auto_leave: Arc<AutoLeaveService>,
}

impl AppState {
    pub fn build(config: Arc<AppConfig>, songbird: Arc<Songbird>) -> Result<Arc<Self>, AppError> {
        let http_client =
            reqwest::Client::builder()
                .build()
                .map_err(|source| AppError::Internal {
                    context: format!("could not build shared HTTP client: {source}"),
                })?;
        let players = Arc::new(PlayerManager::new(config.max_queue_size)?);
        let resolution_slots = Arc::new(Semaphore::new(config.max_concurrent_resolutions));
        let youtube_process = Arc::new(YoutubeProcess::new(
            config.yt_dlp_path.clone(),
            config.yt_dlp_extra_args.clone(),
            config.yt_dlp_timeout,
            Arc::clone(&resolution_slots),
        ));
        let youtube_resolver = YoutubeResolver::new(youtube_process, config.max_queue_size)?;
        let track_resolver: Arc<dyn TrackResolver> = Arc::new(youtube_resolver);
        let voice = Arc::new(VoiceConnection::new(
            Arc::clone(&songbird),
            Arc::clone(&players),
        ));
        let player_panels = PlayerPanelService::new(
            Arc::clone(&players),
            config.bot_language,
            config.player_panel_update_interval,
        );
        let player_observer: Arc<dyn PlayerObserver> = player_panels.clone();
        let playback = PlaybackService::new(
            Arc::clone(&track_resolver),
            http_client.clone(),
            Arc::clone(&songbird),
            Arc::clone(&players),
            player_observer,
        );
        let sessions = GuildSessionService::new(Arc::clone(&voice), Arc::clone(&players));
        let auto_leave = AutoLeaveService::new(
            Arc::clone(&players),
            Arc::clone(&sessions),
            config.auto_leave_timeout,
        );

        Ok(Arc::new(Self {
            config,
            http_client,
            players,
            resolution_slots,
            track_resolver,
            songbird,
            voice,
            playback,
            player_panels,
            sessions,
            auto_leave,
        }))
    }
}
