use anyhow::{bail, Context, Result};
use auric_audio::AudioEngine;
use auric_core::{
    AppCommand, AppEvent, FeatureId, FeatureRegistry, FeatureState, PlaybackQueueEntry,
    PlaybackState, PlaybackStatus, RepeatMode, TrackId,
};
use auric_library::db::{Database, DatabaseOptions, JournalMode, PragmaSnapshot, SynchronousMode};
use auric_library::scan::{DirectoryScanner, ScanOptions, ScanSummary};
use auric_library::watch::{WatchOptions, WatchSessionSummary, WatchedFolderService, WatchedRoot};
use auric_library::{LibraryRoot, TrackRecord};
use auric_ui::ThemeStore;
use auric_ui::{
    render_once_to_text, run_interactive_full, FsThemeStore, IconMode, Palette,
    PaletteCommandResult, PlaybackAction, PlayerEventUpdate, RunOptions, ScanProgress,
    ShellListItem, ShellSnapshot, ShellState, ShellTrackItem,
};
use serde::Deserialize;
use serde_json::{json, Value as JsonValue};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct AppConfig {
    pub features: FeaturesConfig,
    pub library: LibraryConfig,
    pub ui: UiConfig,
    pub database: DatabaseConfig,
}

impl AppConfig {
    pub fn load_from_path(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read config {}", path.display()))?;
        let cfg: Self = toml::from_str(&raw)
            .with_context(|| format!("failed to parse TOML config {}", path.display()))?;
        Ok(cfg)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct FeaturesConfig {
    pub metadata: bool,
    pub artwork: bool,
    pub remote_metadata: bool,
    pub watched_folders: bool,
    pub equalizer: bool,
    pub visualizer: bool,
    pub analytics: bool,
    pub p2p_sync: bool,
    pub p2p_stream: bool,
    pub mouse: bool,
    pub image_artwork: bool,
}

impl Default for FeaturesConfig {
    fn default() -> Self {
        Self {
            metadata: true,
            artwork: true,
            remote_metadata: false,
            watched_folders: true,
            equalizer: false,
            visualizer: false,
            analytics: false,
            p2p_sync: false,
            p2p_stream: false,
            mouse: true,
            image_artwork: true,
        }
    }
}

impl FeaturesConfig {
    pub fn enabled_for(&self, feature: FeatureId) -> bool {
        match feature {
            FeatureId::Metadata => self.metadata,
            FeatureId::Artwork => self.artwork,
            FeatureId::RemoteMetadata => self.remote_metadata,
            FeatureId::WatchedFolders => self.watched_folders,
            FeatureId::Equalizer => self.equalizer,
            FeatureId::Visualizer => self.visualizer,
            FeatureId::Analytics => self.analytics,
            FeatureId::P2PSync => self.p2p_sync,
            FeatureId::P2PStream => self.p2p_stream,
            FeatureId::Mouse => self.mouse,
            FeatureId::ImageArtwork => self.image_artwork,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LibraryConfig {
    pub auto_scan_on_start: bool,
    pub watch_debounce_ms: u64,
    pub read_embedded_artwork: bool,
    pub write_tags: bool,
    pub scan_batch_size: usize,
    pub prune_missing_on_scan: bool,
}

impl Default for LibraryConfig {
    fn default() -> Self {
        Self {
            auto_scan_on_start: true,
            watch_debounce_ms: 750,
            read_embedded_artwork: true,
            write_tags: true,
            scan_batch_size: 2_000,
            prune_missing_on_scan: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub theme: String,
    pub color_scheme: String,
    pub artwork_display_filter: String,
    pub pixel_art_artwork: bool,
    pub pixel_art_cell_size: u16,
    pub pixel_art_redraw_policy: String,
    pub icon_pack: String,
    pub icon_fallback: String,
    pub preferred_terminal_font: String,
    pub use_theme_background: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: "auric-dark".to_string(),
            color_scheme: "dark".to_string(),
            artwork_display_filter: "none".to_string(),
            pixel_art_artwork: false,
            pixel_art_cell_size: 2,
            pixel_art_redraw_policy: "on-change".to_string(),
            icon_pack: "nerd-font".to_string(),
            icon_fallback: "ascii".to_string(),
            preferred_terminal_font: "FiraCode Nerd Font Mono".to_string(),
            use_theme_background: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DatabaseConfig {
    pub path: String,
    pub journal_mode: String,
    pub synchronous: String,
    pub busy_timeout_ms: u64,
    pub cache_size_kib: i64,
    pub mmap_size_mb: u64,
    pub wal_autocheckpoint_pages: u32,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: "var/auric.db".to_string(),
            journal_mode: "wal".to_string(),
            synchronous: "normal".to_string(),
            busy_timeout_ms: 5_000,
            cache_size_kib: 8 * 1024,
            mmap_size_mb: 64,
            wal_autocheckpoint_pages: 1000,
        }
    }
}

impl DatabaseConfig {
    pub fn to_options(&self, cwd: &Path) -> Result<DatabaseOptions> {
        let db_path = {
            let p = PathBuf::from(&self.path);
            if p.is_absolute() {
                p
            } else {
                cwd.join(p)
            }
        };

        Ok(DatabaseOptions {
            path: db_path,
            journal_mode: parse_journal_mode(&self.journal_mode)?,
            synchronous: parse_synchronous_mode(&self.synchronous)?,
            busy_timeout_ms: self.busy_timeout_ms,
            cache_size_kib: self.cache_size_kib.max(256),
            mmap_size_bytes: self.mmap_size_mb.saturating_mul(1024 * 1024),
            wal_autocheckpoint_pages: self.wal_autocheckpoint_pages.max(100),
        })
    }
}

#[derive(Debug)]
pub struct BootstrapReport {
    pub config_path: PathBuf,
    pub db_path: Option<PathBuf>,
    pub schema_version: i64,
    pub pragmas: PragmaSnapshot,
    pub stats: auric_library::db::DatabaseStats,
    pub feature_enabled_count: usize,
    pub feature_total_count: usize,
    pub ui_theme: String,
    pub ui_color_scheme: String,
    pub ui_icon_pack: String,
}

#[derive(Debug)]
pub struct BootstrappedApp {
    pub config: AppConfig,
    pub db: Database,
    pub feature_registry: FeatureRegistry,
    pub playback_state: PlaybackState,
    pub report: BootstrapReport,
    pub player: auric_audio::player::PlayerHandle,
}

pub fn bootstrap_from_config_path(config_path: &Path) -> Result<BootstrappedApp> {
    let config = AppConfig::load_from_path(config_path)?;
    let cwd = env::current_dir().context("failed to resolve current working directory")?;
    let options = config.database.to_options(&cwd)?;

    let db = Database::open(&options)?;
    seed_initial_settings(&db, &config)?;
    let feature_registry = load_feature_registry(&db, &config.features)?;
    let playback_state = load_playback_state(&db)?;
    db.quick_check().context("sqlite quick_check failed")?;
    db.optimize().context("sqlite optimize failed")?;

    let feature_enabled_count = FeatureId::ALL
        .into_iter()
        .filter(|feature| feature_registry.is_enabled(*feature))
        .count();

    let report = BootstrapReport {
        config_path: config_path.to_path_buf(),
        db_path: db.path().map(|p| p.to_path_buf()),
        schema_version: db.schema_version()?,
        pragmas: db.pragma_snapshot()?,
        stats: db.stats()?,
        feature_enabled_count,
        feature_total_count: FeatureId::ALL.len(),
        ui_theme: config.ui.theme.clone(),
        ui_color_scheme: config.ui.color_scheme.clone(),
        ui_icon_pack: config.ui.icon_pack.clone(),
    };

    let player = auric_audio::player::PlayerHandle::spawn();

    Ok(BootstrappedApp {
        config,
        db,
        feature_registry,
        playback_state,
        report,
        player,
    })
}

fn seed_initial_settings(db: &Database, config: &AppConfig) -> Result<()> {
    seed_setting_if_missing(db, "ui.theme", json!(config.ui.theme))?;
    seed_setting_if_missing(db, "ui.color_scheme", json!(config.ui.color_scheme))?;
    seed_setting_if_missing(
        db,
        "ui.artwork_display_filter",
        json!(config.ui.artwork_display_filter),
    )?;
    seed_setting_if_missing(
        db,
        "ui.pixel_art_artwork",
        json!(config.ui.pixel_art_artwork),
    )?;
    seed_setting_if_missing(
        db,
        "ui.pixel_art_cell_size",
        json!(config.ui.pixel_art_cell_size),
    )?;
    seed_setting_if_missing(
        db,
        "ui.pixel_art_redraw_policy",
        json!(config.ui.pixel_art_redraw_policy),
    )?;
    seed_setting_if_missing(db, "ui.icon_pack", json!(config.ui.icon_pack))?;
    seed_setting_if_missing(db, "ui.icon_fallback", json!(config.ui.icon_fallback))?;
    seed_setting_if_missing(
        db,
        "ui.preferred_terminal_font",
        json!(config.ui.preferred_terminal_font),
    )?;

    for feature in FeatureId::ALL {
        let key = feature_setting_key(feature);
        seed_setting_if_missing(db, &key, json!(config.features.enabled_for(feature)))?;
    }

    seed_setting_if_missing(
        db,
        PLAYBACK_STATE_SETTING_KEY,
        json!(PlaybackState::default()),
    )?;

    Ok(())
}

fn seed_setting_if_missing(db: &Database, key: &str, value: JsonValue) -> Result<()> {
    if db.get_setting_json(key)?.is_none() {
        db.set_setting_json(key, &value)?;
    }
    Ok(())
}

fn load_feature_registry(db: &Database, defaults: &FeaturesConfig) -> Result<FeatureRegistry> {
    let mut registry = FeatureRegistry::default();
    for feature in FeatureId::ALL {
        let key = feature_setting_key(feature);
        let enabled = match db.get_setting_json(&key)? {
            Some(JsonValue::Bool(v)) => v,
            _ => defaults.enabled_for(feature),
        };
        registry.set_enabled(feature, enabled);
    }
    Ok(registry)
}

fn feature_setting_key(feature: FeatureId) -> String {
    format!("feature.{}.enabled", feature.as_key())
}

const PLAYBACK_STATE_SETTING_KEY: &str = "playback.state";

fn load_playback_state(db: &Database) -> Result<PlaybackState> {
    let raw = db.get_setting_json(PLAYBACK_STATE_SETTING_KEY)?;
    let mut state = match raw {
        Some(value) => serde_json::from_value::<PlaybackState>(value).unwrap_or_else(|err| {
            eprintln!("warning: failed to deserialize playback state, resetting: {err}");
            PlaybackState::default()
        }),
        None => PlaybackState::default(),
    };
    normalize_playback_state(&mut state);
    save_playback_state(db, &state)?;
    Ok(state)
}

fn save_playback_state(db: &Database, state: &PlaybackState) -> Result<()> {
    db.set_setting_json(PLAYBACK_STATE_SETTING_KEY, &serde_json::to_value(state)?)?;
    Ok(())
}

fn normalize_playback_state(state: &mut PlaybackState) {
    if state.queue.is_empty() {
        state.session.current_index = None;
        state.session.position_ms = 0;
        if !matches!(state.session.status, PlaybackStatus::Stopped) {
            state.session.status = PlaybackStatus::Stopped;
        }
    } else if let Some(idx) = state.session.current_index {
        if idx >= state.queue.len() {
            state.session.current_index = Some(state.queue.len().saturating_sub(1));
            state.session.position_ms = 0;
        }
    }

    if !state.session.volume.is_finite() {
        state.session.volume = 1.0;
    }
    state.session.volume = state.session.volume.clamp(0.0, 1.0);
}

fn persist_playback_state(app: &mut BootstrappedApp) -> Result<()> {
    normalize_playback_state(&mut app.playback_state);
    save_playback_state(&app.db, &app.playback_state)
}

fn playback_queue_entry_from_track_row(row: auric_library::db::TrackRow) -> PlaybackQueueEntry {
    PlaybackQueueEntry {
        track_id: row.id,
        path: row.path,
        title: row.title,
        artist: row.artist,
        album: row.album,
        duration_ms: row.duration_ms,
        sample_rate: row.sample_rate,
        channels: row.channels,
        bit_depth: row.bit_depth,
    }
}

fn emit_playback_state_changed(events: &mut Vec<AppEvent>, state: &PlaybackState) {
    events.push(AppEvent::PlaybackStateChanged {
        status: state.session.status,
        current_index: state.session.current_index,
        queue_len: state.queue.len(),
    });
}

fn current_track_id(state: &PlaybackState) -> Option<TrackId> {
    state.current_entry().map(|entry| entry.track_id)
}

pub fn run_cli() -> Result<()> {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "ui".to_string());
    let config_path = resolve_config_path();

    match command.as_str() {
        "init" => {
            let app = bootstrap_from_config_path(&config_path)?;
            print_bootstrap_report(&app.report);
        }
        "doctor" => {
            let app = bootstrap_from_config_path(&config_path)?;
            print_bootstrap_report(&app.report);
            println!("doctor: quick_check=ok optimize=ok");
        }
        "db-stress" => {
            let count = match args.next() {
                Some(raw) => raw
                    .parse::<usize>()
                    .with_context(|| format!("invalid track count: {raw}"))?,
                None => 20_000,
            };
            let mut app = bootstrap_from_config_path(&config_path)?;
            run_db_stress(&mut app.db, count)?;
        }
        "feature" => {
            let mut app = bootstrap_from_config_path(&config_path)?;
            let subargs: Vec<String> = args.collect();
            handle_feature_command(&mut app, &subargs)?;
        }
        "root" => {
            let app = bootstrap_from_config_path(&config_path)?;
            let subargs: Vec<String> = args.collect();
            handle_root_command(&app, &subargs)?;
        }
        "playlist" => {
            let app = bootstrap_from_config_path(&config_path)?;
            let subargs: Vec<String> = args.collect();
            handle_playlist_command(&app, &subargs)?;
        }
        "scan" => {
            let mut app = bootstrap_from_config_path(&config_path)?;
            let subargs: Vec<String> = args.collect();
            handle_scan_command(&mut app, &subargs)?;
        }
        "watch" => {
            let mut app = bootstrap_from_config_path(&config_path)?;
            let subargs: Vec<String> = args.collect();
            handle_watch_command(&mut app, &subargs)?;
        }
        "artwork" => {
            let app = bootstrap_from_config_path(&config_path)?;
            let subargs: Vec<String> = args.collect();
            handle_artwork_command(&app, &subargs)?;
        }
        "track" => {
            let app = bootstrap_from_config_path(&config_path)?;
            let subargs: Vec<String> = args.collect();
            handle_track_command(&app, &subargs)?;
        }
        "audio" => {
            let app = bootstrap_from_config_path(&config_path)?;
            let subargs: Vec<String> = args.collect();
            handle_audio_command(&app, &subargs)?;
        }
        "playback" => {
            let mut app = bootstrap_from_config_path(&config_path)?;
            let subargs: Vec<String> = args.collect();
            handle_playback_command(&mut app, &subargs)?;
        }
        "ui" => {
            let mut app = bootstrap_from_config_path(&config_path)?;
            let subargs: Vec<String> = args.collect();
            handle_ui_command(&mut app, &subargs)?;
        }
        other => {
            bail!(
                "unknown command: {other}. expected one of: init, doctor, db-stress [count], feature, root, playlist, scan, watch, artwork, track, audio, playback, ui"
            );
        }
    }

    Ok(())
}

fn resolve_config_path() -> PathBuf {
    if let Ok(path) = env::var("AURIC_CONFIG") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    PathBuf::from("config/default.toml")
}

fn dispatch_app_command(app: &mut BootstrappedApp, command: AppCommand) -> Result<Vec<AppEvent>> {
    let mut events = Vec::new();

    match command {
        AppCommand::ToggleFeature { feature, enabled } => {
            let transition = if enabled {
                FeatureState::Starting
            } else {
                FeatureState::Stopping
            };
            app.feature_registry.set_state(feature, transition.clone());
            events.push(AppEvent::FeatureStateChanged {
                feature,
                state: transition,
            });

            app.db
                .set_setting_json(&feature_setting_key(feature), &json!(enabled))?;

            let final_state = if enabled {
                FeatureState::Enabled
            } else {
                FeatureState::Disabled
            };
            app.feature_registry.set_state(feature, final_state.clone());
            events.push(AppEvent::FeatureStateChanged {
                feature,
                state: final_state,
            });

            app.report.feature_enabled_count = FeatureId::ALL
                .into_iter()
                .filter(|f| app.feature_registry.is_enabled(*f))
                .count();
        }
        cmd @ (AppCommand::Play
        | AppCommand::Pause
        | AppCommand::Stop
        | AppCommand::Next
        | AppCommand::Previous
        | AppCommand::SeekMillis(_)
        | AppCommand::SetVolume(_)) => {
            handle_playback_transport_command(app, cmd, &mut events)?;
        }
    }

    Ok(events)
}

fn handle_tui_playback_action(
    app: &mut BootstrappedApp,
    action: PlaybackAction,
) -> Result<PaletteCommandResult> {
    match action {
        PlaybackAction::PlayTrack { track_index } => {
            let total = app.db.stats().map(|s| s.track_count).unwrap_or(250) as usize;
            let tracks = app.db.list_tracks(total).unwrap_or_default();
            let queue: Vec<PlaybackQueueEntry> = tracks
                .into_iter()
                .map(|t| PlaybackQueueEntry {
                    track_id: t.id,
                    path: t.path,
                    title: t.title,
                    artist: t.artist,
                    album: t.album,
                    duration_ms: t.duration_ms,
                    sample_rate: t.sample_rate,
                    channels: t.channels,
                    bit_depth: t.bit_depth,
                })
                .collect();

            if track_index >= queue.len() {
                return Ok(PaletteCommandResult::new("No track at that index", false));
            }

            app.playback_state.queue = queue;
            app.playback_state.session.current_index = Some(track_index);
            app.playback_state.session.status = PlaybackStatus::Playing;
            app.playback_state.session.position_ms = 0;

            let entry = &app.playback_state.queue[track_index];
            app.player.load(&entry.path);

            let title = entry.title.clone().unwrap_or_default();
            Ok(PaletteCommandResult::new(
                format!("Playing: {title}"),
                true,
            ))
        }
        PlaybackAction::TogglePause => match app.playback_state.session.status {
            PlaybackStatus::Playing => {
                app.player.pause();
                app.playback_state.session.status = PlaybackStatus::Paused;
                Ok(PaletteCommandResult::new("Paused", true))
            }
            PlaybackStatus::Paused => {
                app.player.resume();
                app.playback_state.session.status = PlaybackStatus::Playing;
                Ok(PaletteCommandResult::new("Resumed", true))
            }
            PlaybackStatus::Stopped => {
                if let Some(idx) = app.playback_state.session.current_index {
                    let entry_path = app.playback_state.queue.get(idx).map(|e| e.path.clone());
                    let entry_title = app
                        .playback_state
                        .queue
                        .get(idx)
                        .and_then(|e| e.title.clone());
                    if let Some(path) = entry_path {
                        app.player.load(&path);
                        app.playback_state.session.status = PlaybackStatus::Playing;
                        let title = entry_title.unwrap_or_default();
                        return Ok(PaletteCommandResult::new(
                            format!("Playing: {title}"),
                            true,
                        ));
                    }
                }
                Ok(PaletteCommandResult::new("No track to play", false))
            }
        },
        PlaybackAction::Stop => {
            app.player.stop();
            app.playback_state.session.status = PlaybackStatus::Stopped;
            Ok(PaletteCommandResult::new("Stopped", true))
        }
        PlaybackAction::Next => {
            let mut events = Vec::new();
            handle_playback_transport_command(app, AppCommand::Next, &mut events)?;
            let status = app.playback_state.session.status;
            let entry_info = app.playback_state.current_entry().map(|e| {
                (e.path.clone(), e.title.clone().unwrap_or_default())
            });
            if status == PlaybackStatus::Playing || status == PlaybackStatus::Paused {
                if let Some((path, title)) = entry_info {
                    app.player.load(&path);
                    app.playback_state.session.status = PlaybackStatus::Playing;
                    return Ok(PaletteCommandResult::new(
                        format!("Playing: {title}"),
                        true,
                    ));
                }
            }
            app.player.stop();
            Ok(PaletteCommandResult::new("End of queue", true))
        }
        PlaybackAction::Previous => {
            let mut events = Vec::new();
            handle_playback_transport_command(app, AppCommand::Previous, &mut events)?;
            let status = app.playback_state.session.status;
            let entry_info = app.playback_state.current_entry().map(|e| {
                (e.path.clone(), e.title.clone().unwrap_or_default())
            });
            if let Some((path, title)) = entry_info {
                if status == PlaybackStatus::Playing {
                    app.player.load(&path);
                }
                return Ok(PaletteCommandResult::new(
                    format!("Track: {title}"),
                    true,
                ));
            }
            Ok(PaletteCommandResult::new("Start of queue", true))
        }
        PlaybackAction::VolumeUp => {
            let new_vol = (app.playback_state.session.volume + 0.05).min(1.0);
            app.playback_state.session.volume = new_vol;
            app.player.set_volume(new_vol);
            Ok(PaletteCommandResult::new(
                format!("Volume: {}%", (new_vol * 100.0).round() as u32),
                true,
            ))
        }
        PlaybackAction::VolumeDown => {
            let new_vol = (app.playback_state.session.volume - 0.05).max(0.0);
            app.playback_state.session.volume = new_vol;
            app.player.set_volume(new_vol);
            Ok(PaletteCommandResult::new(
                format!("Volume: {}%", (new_vol * 100.0).round() as u32),
                true,
            ))
        }
        PlaybackAction::ToggleShuffle => {
            app.playback_state.session.shuffle = !app.playback_state.session.shuffle;
            let label = if app.playback_state.session.shuffle {
                "Shuffle: on"
            } else {
                "Shuffle: off"
            };
            Ok(PaletteCommandResult::new(label, true))
        }
    }
}

fn handle_playback_transport_command(
    app: &mut BootstrappedApp,
    command: AppCommand,
    events: &mut Vec<AppEvent>,
) -> Result<()> {
    let prev_track_id = current_track_id(&app.playback_state);
    let mut track_changed = false;

    match command {
        AppCommand::Play => {
            if app.playback_state.queue.is_empty() {
                events.push(AppEvent::Warning("playback queue is empty".to_string()));
                return Ok(());
            }
            if app.playback_state.session.current_index.is_none() {
                app.playback_state.session.current_index = Some(0);
                app.playback_state.session.position_ms = 0;
                track_changed = true;
            }
            app.playback_state.session.status = PlaybackStatus::Playing;
        }
        AppCommand::Pause => {
            app.playback_state.session.status = PlaybackStatus::Paused;
        }
        AppCommand::Stop => {
            app.playback_state.session.status = PlaybackStatus::Stopped;
            app.playback_state.session.position_ms = 0;
        }
        AppCommand::SeekMillis(position_ms) => {
            let clamped = if let Some(entry) = app.playback_state.current_entry() {
                match entry.duration_ms {
                    Some(ms) if ms > 0 => position_ms.min(ms as u64),
                    _ => position_ms,
                }
            } else {
                position_ms
            };
            app.playback_state.session.position_ms = clamped;
            events.push(AppEvent::PlaybackPositionMillis(clamped));
        }
        AppCommand::SetVolume(volume) => {
            let normalized = if volume.is_finite() { volume } else { 1.0 };
            app.playback_state.session.volume = normalized.clamp(0.0, 1.0);
        }
        AppCommand::Next => {
            if app.playback_state.queue.is_empty() {
                events.push(AppEvent::Warning("playback queue is empty".to_string()));
                return Ok(());
            }
            let len = app.playback_state.queue.len();
            let current = app.playback_state.session.current_index.unwrap_or(0);
            let next_index = match app.playback_state.session.repeat {
                RepeatMode::One => Some(current.min(len.saturating_sub(1))),
                RepeatMode::All => Some((current + 1) % len),
                RepeatMode::Off => {
                    if current + 1 < len {
                        Some(current + 1)
                    } else {
                        None
                    }
                }
            };

            match next_index {
                Some(idx) => {
                    let was_none = app.playback_state.session.current_index.is_none();
                    app.playback_state.session.current_index = Some(idx);
                    app.playback_state.session.position_ms = 0;
                    if app.playback_state.session.status == PlaybackStatus::Stopped {
                        app.playback_state.session.status = PlaybackStatus::Paused;
                    }
                    track_changed =
                        was_none || prev_track_id != current_track_id(&app.playback_state);
                }
                None => {
                    app.playback_state.session.status = PlaybackStatus::Stopped;
                    app.playback_state.session.position_ms = 0;
                    events.push(AppEvent::Warning("end of queue".to_string()));
                }
            }
        }
        AppCommand::Previous => {
            if app.playback_state.queue.is_empty() {
                events.push(AppEvent::Warning("playback queue is empty".to_string()));
                return Ok(());
            }
            if app.playback_state.session.position_ms > 3_000 {
                app.playback_state.session.position_ms = 0;
                events.push(AppEvent::PlaybackPositionMillis(0));
            } else {
                let len = app.playback_state.queue.len();
                let current = app.playback_state.session.current_index.unwrap_or(0);
                let prev_index = match app.playback_state.session.repeat {
                    RepeatMode::One => current.min(len.saturating_sub(1)),
                    RepeatMode::All => {
                        if current == 0 {
                            len.saturating_sub(1)
                        } else {
                            current - 1
                        }
                    }
                    RepeatMode::Off => current.saturating_sub(1),
                };
                app.playback_state.session.current_index = Some(prev_index);
                app.playback_state.session.position_ms = 0;
                track_changed = prev_track_id != current_track_id(&app.playback_state);
            }
        }
        AppCommand::ToggleFeature { .. } => unreachable!("handled above"),
    }

    let new_track_id = current_track_id(&app.playback_state);
    if track_changed || prev_track_id != new_track_id {
        events.push(AppEvent::TrackChanged {
            track_id: new_track_id,
        });
    }
    emit_playback_state_changed(events, &app.playback_state);
    persist_playback_state(app)?;
    Ok(())
}

fn handle_feature_command(app: &mut BootstrappedApp, args: &[String]) -> Result<()> {
    let sub = args.first().map(String::as_str).unwrap_or("list");
    match sub {
        "list" => {
            print_feature_list(app);
        }
        "enable" | "disable" => {
            let name = args
                .get(1)
                .map(String::as_str)
                .ok_or_else(|| anyhow::anyhow!("usage: auric feature {sub} <feature-name>"))?;
            let feature = FeatureId::from_key(name).ok_or_else(|| {
                anyhow::anyhow!(
                    "unknown feature: {name}. valid values: {}",
                    FeatureId::ALL
                        .into_iter()
                        .map(|f| f.as_key())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
            let enabled = sub == "enable";
            let events = dispatch_app_command(app, AppCommand::ToggleFeature { feature, enabled })?;
            for event in events {
                println!("event: {event:?}");
            }
            println!(
                "feature {} => {}",
                feature,
                if app.feature_registry.is_enabled(feature) {
                    "enabled"
                } else {
                    "disabled"
                }
            );
        }
        _ => bail!("usage: auric feature [list|enable <name>|disable <name>]"),
    }
    Ok(())
}

fn print_feature_list(app: &BootstrappedApp) {
    println!(
        "features (enabled {}/{}):",
        app.report.feature_enabled_count, app.report.feature_total_count
    );
    for feature in FeatureId::ALL {
        let state = app.feature_registry.state(feature);
        let state_label = match state {
            FeatureState::Disabled => "disabled",
            FeatureState::Starting => "starting",
            FeatureState::Enabled => "enabled",
            FeatureState::Degraded { .. } => "degraded",
            FeatureState::Stopping => "stopping",
        };
        println!("  {:<18} {}", feature.as_key(), state_label);
    }
}

fn handle_root_command(app: &BootstrappedApp, args: &[String]) -> Result<()> {
    let sub = args.first().map(String::as_str).unwrap_or("list");
    match sub {
        "list" => {
            let rows = app.db.list_library_roots()?;
            if rows.is_empty() {
                println!("no library roots");
            } else {
                for row in rows {
                    println!("{} | watched={} | {}", row.id, row.watched, row.path);
                }
            }
        }
        "add" => {
            let path = args
                .get(1)
                .map(String::as_str)
                .ok_or_else(|| anyhow::anyhow!("usage: auric root add <path> [--watched]"))?;
            let resolved = std::path::Path::new(path);
            if !resolved.exists() {
                bail!("path does not exist: {path}");
            }
            if !resolved.is_dir() {
                bail!("path is not a directory: {path}");
            }
            let watched = args
                .iter()
                .skip(2)
                .any(|a| a == "--watched" || a == "watched");
            let row = app.db.upsert_library_root(&LibraryRoot {
                path: path.to_string(),
                watched,
            })?;
            println!(
                "root saved: {} | watched={} | {}",
                row.id, row.watched, row.path
            );
        }
        _ => bail!("usage: auric root [list|add <path> [--watched]]"),
    }
    Ok(())
}

fn handle_playlist_command(app: &BootstrappedApp, args: &[String]) -> Result<()> {
    let sub = args.first().map(String::as_str).unwrap_or("list");
    match sub {
        "list" => {
            let rows = app.db.list_playlists()?;
            if rows.is_empty() {
                println!("no playlists");
            } else {
                for row in rows {
                    println!("{} | {}", row.id, row.name);
                }
            }
        }
        "create" => {
            let name = join_args(args, 1)
                .ok_or_else(|| anyhow::anyhow!("usage: auric playlist create <name>"))?;
            let id = app.db.create_playlist(&name)?;
            println!("playlist created: {} | {}", id, name);
        }
        "rename" => {
            let id = args
                .get(1)
                .map(String::as_str)
                .ok_or_else(|| anyhow::anyhow!("usage: auric playlist rename <id> <name>"))?;
            let name = join_args(args, 2)
                .ok_or_else(|| anyhow::anyhow!("usage: auric playlist rename <id> <name>"))?;
            app.db.rename_playlist(id, &name)?;
            println!("playlist renamed: {} | {}", id, name);
        }
        "delete" => {
            let id = args
                .get(1)
                .map(String::as_str)
                .ok_or_else(|| anyhow::anyhow!("usage: auric playlist delete <id>"))?;
            app.db.delete_playlist(id)?;
            println!("playlist deleted: {}", id);
        }
        "list-tracks" => {
            let id = args
                .get(1)
                .map(String::as_str)
                .ok_or_else(|| anyhow::anyhow!("usage: auric playlist list-tracks <id> [--limit N]"))?;
            let mut limit = 100usize;
            let mut i = 2usize;
            while i < args.len() {
                match args[i].as_str() {
                    "--limit" => {
                        let raw = args.get(i + 1).ok_or_else(|| {
                            anyhow::anyhow!(
                                "usage: auric playlist list-tracks <id> [--limit N]"
                            )
                        })?;
                        limit = raw
                            .parse::<usize>()
                            .with_context(|| format!("invalid --limit value: {raw}"))?;
                        i += 2;
                    }
                    other => bail!(
                        "unknown argument for playlist list-tracks: {other}. usage: auric playlist list-tracks <id> [--limit N]"
                    ),
                }
            }
            let rows = app.db.list_playlist_tracks(id, limit)?;
            if rows.is_empty() {
                println!("playlist has no tracks: {id}");
            } else {
                for row in rows {
                    println!(
                        "{:>4} | {} | {} | {} | {}",
                        row.position,
                        row.track.artist.as_deref().unwrap_or("-"),
                        row.track.album.as_deref().unwrap_or("-"),
                        row.track.title.as_deref().unwrap_or("-"),
                        row.track.path
                    );
                }
            }
        }
        "add-track" => {
            let playlist_id = args
                .get(1)
                .map(String::as_str)
                .ok_or_else(|| anyhow::anyhow!("usage: auric playlist add-track <playlist-id> <track-path> | auric playlist add-track <playlist-id> --track-id <track-id>"))?;

            let track_row = if args.get(2).map(String::as_str) == Some("--track-id") {
                let raw = args.get(3).ok_or_else(|| {
                    anyhow::anyhow!(
                        "usage: auric playlist add-track <playlist-id> --track-id <track-id>"
                    )
                })?;
                let track_id = TrackId(Uuid::parse_str(raw).with_context(|| {
                    format!("invalid track id (expected UUID): {raw}")
                })?);
                app.db
                    .get_track_by_id(track_id)?
                    .ok_or_else(|| anyhow::anyhow!("track not found by id: {raw}"))?
            } else {
                let path = join_args(args, 2).ok_or_else(|| {
                    anyhow::anyhow!("usage: auric playlist add-track <playlist-id> <track-path>")
                })?;
                app.db
                    .get_track_by_path(&path)?
                    .ok_or_else(|| anyhow::anyhow!("track not found by path: {path}"))?
            };

            let position = app
                .db
                .append_track_to_playlist(playlist_id, track_row.id)?;
            println!(
                "playlist track added: {} @ {} | {}",
                playlist_id, position, track_row.path
            );
        }
        "remove-track" => {
            let playlist_id = args.get(1).map(String::as_str).ok_or_else(|| {
                anyhow::anyhow!("usage: auric playlist remove-track <playlist-id> <position>")
            })?;
            let raw = args.get(2).ok_or_else(|| {
                anyhow::anyhow!("usage: auric playlist remove-track <playlist-id> <position>")
            })?;
            let position = raw
                .parse::<i64>()
                .with_context(|| format!("invalid playlist position: {raw}"))?;
            app.db.remove_playlist_track_at(playlist_id, position)?;
            println!("playlist track removed: {} @ {}", playlist_id, position);
        }
        "clear-tracks" => {
            let playlist_id = args.get(1).map(String::as_str).ok_or_else(|| {
                anyhow::anyhow!("usage: auric playlist clear-tracks <playlist-id>")
            })?;
            let removed = app.db.clear_playlist_tracks(playlist_id)?;
            println!("playlist tracks cleared: {} (removed {})", playlist_id, removed);
        }
        _ => bail!("usage: auric playlist [list|create <name>|rename <id> <name>|delete <id>|list-tracks <id> [--limit N]|add-track <playlist-id> <track-path>|add-track <playlist-id> --track-id <track-id>|remove-track <playlist-id> <position>|clear-tracks <playlist-id>]"),
    }
    Ok(())
}

fn join_args(args: &[String], start: usize) -> Option<String> {
    if args.len() <= start {
        return None;
    }
    let joined = args[start..].join(" ").trim().to_string();
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

fn handle_scan_command(app: &mut BootstrappedApp, args: &[String]) -> Result<()> {
    let sub = args.first().map(String::as_str).unwrap_or("roots");
    match sub {
        "roots" => {
            let prune = has_flag(args, "--prune");
            let scanner = scanner_from_config(&app.config.library, prune);
            let roots = app.db.list_library_roots()?;
            if roots.is_empty() {
                println!("no library roots configured");
            } else {
                let mut summaries = Vec::new();
                let mut skipped_invalid_roots = 0usize;
                for root in roots {
                    match scanner.scan_path(&mut app.db, Path::new(&root.path)) {
                        Ok(summary) => summaries.push(summary),
                        Err(auric_library::scan::ScanError::InvalidRoot(reason)) => {
                            skipped_invalid_roots += 1;
                            eprintln!("auric scan warning: skipping root: {reason}");
                        }
                        Err(err) => return Err(err.into()),
                    }
                }
                for summary in summaries {
                    print_scan_summary(&summary);
                }
                if skipped_invalid_roots > 0 {
                    println!("skipped_invalid_roots: {}", skipped_invalid_roots);
                }
            }
        }
        "path" => {
            let path = args
                .get(1)
                .map(String::as_str)
                .ok_or_else(|| anyhow::anyhow!("usage: auric scan path <dir> [--prune]"))?;
            let prune = has_flag(args, "--prune");
            let scanner = scanner_from_config(&app.config.library, prune);
            let summary = scanner.scan_path(&mut app.db, Path::new(path))?;
            print_scan_summary(&summary);
        }
        _ => bail!("usage: auric scan [roots [--prune] | path <dir> [--prune]]"),
    }
    Ok(())
}

fn scanner_from_config(cfg: &LibraryConfig, prune_override: bool) -> DirectoryScanner {
    DirectoryScanner::new(ScanOptions {
        batch_size: cfg.scan_batch_size.max(1),
        prune_missing: cfg.prune_missing_on_scan || prune_override,
        follow_symlinks: false,
        read_embedded_artwork: cfg.read_embedded_artwork,
        max_embedded_artwork_bytes: 8 * 1024 * 1024,
    })
}

fn handle_watch_command(app: &mut BootstrappedApp, args: &[String]) -> Result<()> {
    let sub = args.first().map(String::as_str).unwrap_or("roots");
    let watched_feature_enabled = app.feature_registry.is_enabled(FeatureId::WatchedFolders);
    if !watched_feature_enabled {
        eprintln!(
            "auric watch warning: watched_folders feature is disabled; watcher commands still run for testing"
        );
    }

    match sub {
        "roots" => {
            let run_for_ms = parse_optional_u64_flag(args, "--run-for-ms")?;
            let prune = has_flag(args, "--prune");
            let watched_only = !has_flag(args, "--all-roots");
            let scan_on_start = has_flag(args, "--scan-on-start");
            let service = watcher_from_config(&app.config.library, WatchOptionsOverrides {
                prune_override: prune,
                watched_only,
                scan_on_start,
                run_for_ms,
            });
            println!(
                "watching {} roots (mode={})",
                if watched_only { "watched" } else { "all" },
                if let Some(ms) = run_for_ms {
                    format!("bounded {ms}ms")
                } else {
                    "until interrupted".to_string()
                }
            );
            let summary = service.watch_saved_roots(&mut app.db)?;
            print_watch_summary(&summary);
        }
        "path" => {
            let path = args.get(1).ok_or_else(|| {
                anyhow::anyhow!(
                    "usage: auric watch path <dir> [--prune] [--scan-on-start] [--run-for-ms N]"
                )
            })?;
            let run_for_ms = parse_optional_u64_flag(args, "--run-for-ms")?;
            let prune = has_flag(args, "--prune");
            let scan_on_start = has_flag(args, "--scan-on-start");
            let service = watcher_from_config(&app.config.library, WatchOptionsOverrides {
                prune_override: prune,
                watched_only: false,
                scan_on_start,
                run_for_ms,
            });
            let summary = service.watch_roots(
                &mut app.db,
                vec![WatchedRoot {
                    path_string: path.clone(),
                    path: PathBuf::from(path),
                }],
            )?;
            print_watch_summary(&summary);
        }
        _ => bail!(
            "usage: auric watch [roots [--all-roots] [--prune] [--scan-on-start] [--run-for-ms N] | path <dir> [--prune] [--scan-on-start] [--run-for-ms N]]"
        ),
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct WatchOptionsOverrides {
    prune_override: bool,
    watched_only: bool,
    scan_on_start: bool,
    run_for_ms: Option<u64>,
}

fn watcher_from_config(
    cfg: &LibraryConfig,
    overrides: WatchOptionsOverrides,
) -> WatchedFolderService {
    WatchedFolderService::new(WatchOptions {
        debounce_ms: cfg.watch_debounce_ms.max(50),
        poll_timeout_ms: 250,
        watched_only: overrides.watched_only,
        prune_missing: cfg.prune_missing_on_scan || overrides.prune_override,
        scan_batch_size: cfg.scan_batch_size.max(1),
        follow_symlinks: false,
        read_embedded_artwork: cfg.read_embedded_artwork,
        max_embedded_artwork_bytes: 8 * 1024 * 1024,
        scan_on_start: overrides.scan_on_start,
        max_runtime: overrides.run_for_ms.map(Duration::from_millis),
    })
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

fn parse_optional_u64_flag(args: &[String], flag: &str) -> Result<Option<u64>> {
    let mut i = 0usize;
    while i < args.len() {
        if args[i] == flag {
            let raw = args
                .get(i + 1)
                .ok_or_else(|| anyhow::anyhow!("missing value for {flag}"))?;
            let value = raw
                .parse::<u64>()
                .with_context(|| format!("invalid value for {flag}: {raw}"))?;
            return Ok(Some(value));
        }
        i += 1;
    }
    Ok(None)
}

fn print_scan_summary(summary: &ScanSummary) {
    println!("scan complete");
    println!("  root: {}", summary.root_path);
    println!(
        "  discovered_audio_files: {}",
        summary.discovered_audio_files
    );
    println!("  imported_tracks: {}", summary.imported_tracks);
    println!(
        "  embedded_artwork_candidates: {}",
        summary.embedded_artwork_candidates
    );
    println!(
        "  embedded_artwork_linked_tracks: {}",
        summary.embedded_artwork_linked_tracks
    );
    println!(
        "  embedded_artwork_inserted_assets: {}",
        summary.embedded_artwork_inserted_assets
    );
    println!(
        "  embedded_artwork_reused_assets: {}",
        summary.embedded_artwork_reused_assets
    );
    println!(
        "  embedded_artwork_skipped_oversize: {}",
        summary.embedded_artwork_skipped_oversize
    );
    println!(
        "  skipped_non_audio_files: {}",
        summary.skipped_non_audio_files
    );
    println!(
        "  skipped_unreadable_entries: {}",
        summary.skipped_unreadable_entries
    );
    println!("  pruned_missing_tracks: {}", summary.pruned_missing_tracks);
    println!(
        "  purged_orphan_artwork_assets: {}",
        summary.purged_orphan_artwork_assets
    );
    println!("  elapsed_ms: {}", summary.elapsed_ms);
}

fn print_watch_summary(summary: &WatchSessionSummary) {
    println!("watch session complete");
    println!("  watched_root_count: {}", summary.watched_root_count);
    println!("  skipped_root_count: {}", summary.skipped_root_count);
    println!(
        "  observed_notify_events: {}",
        summary.observed_notify_events
    );
    println!("  ignored_notify_events: {}", summary.ignored_notify_events);
    println!("  rescans: {}", summary.rescans.len());
    for rescan in summary.rescans.iter().take(8) {
        println!(
            "    root={} reason={} events={} imported={} pruned={} elapsed_ms={}",
            rescan.root_path,
            rescan.reason,
            rescan.event_count,
            rescan.summary.imported_tracks,
            rescan.summary.pruned_missing_tracks,
            rescan.summary.elapsed_ms
        );
    }
    if summary.rescans.len() > 8 {
        println!("    ... {} additional rescans", summary.rescans.len() - 8);
    }
    println!("  elapsed_ms: {}", summary.elapsed_ms);
}

fn handle_track_command(app: &BootstrappedApp, args: &[String]) -> Result<()> {
    let sub = args.first().map(String::as_str).unwrap_or("list");
    match sub {
        "list" => {
            let mut limit = 20usize;
            let mut prefix: Option<String> = None;

            let mut i = 1usize;
            while i < args.len() {
                match args[i].as_str() {
                    "--limit" => {
                        let raw = args.get(i + 1).ok_or_else(|| {
                            anyhow::anyhow!("usage: auric track list [--limit N] [--prefix PATH]")
                        })?;
                        limit = raw
                            .parse::<usize>()
                            .with_context(|| format!("invalid --limit value: {raw}"))?;
                        i += 2;
                    }
                    "--prefix" => {
                        let raw = args.get(i + 1).ok_or_else(|| {
                            anyhow::anyhow!("usage: auric track list [--limit N] [--prefix PATH]")
                        })?;
                        prefix = Some(raw.clone());
                        i += 2;
                    }
                    other => {
                        bail!(
                            "unknown argument for track list: {other}. usage: auric track list [--limit N] [--prefix PATH]"
                        );
                    }
                }
            }

            let rows = if let Some(prefix) = prefix {
                app.db.list_tracks_by_prefix(&prefix, limit)?
            } else {
                app.db.list_tracks(limit)?
            };

            if rows.is_empty() {
                println!("no tracks");
            } else {
                for row in rows {
                    println!(
                        "{} | {} | {} | {} | {} | {}Hz {}ch {}bit | {}ms",
                        row.id.0,
                        row.artist.as_deref().unwrap_or("-"),
                        row.album.as_deref().unwrap_or("-"),
                        row.title.as_deref().unwrap_or("-"),
                        row.path,
                        row.sample_rate.unwrap_or_default(),
                        row.channels.unwrap_or_default(),
                        row.bit_depth.unwrap_or_default(),
                        row.duration_ms.unwrap_or_default()
                    );
                }
            }
        }
        _ => bail!("usage: auric track [list [--limit N] [--prefix PATH]]"),
    }
    Ok(())
}

fn handle_audio_command(app: &BootstrappedApp, args: &[String]) -> Result<()> {
    let sub = args.first().map(String::as_str).unwrap_or("devices");
    let engine = AudioEngine::new();

    match sub {
        "devices" => {
            let devices = engine.list_output_devices()?;
            if devices.is_empty() {
                println!("no output devices reported by backend");
            } else {
                println!("audio output devices");
                for dev in devices {
                    println!(
                        "  {} | default={} | {}",
                        dev.id, dev.default_output, dev.name
                    );
                }
            }
        }
        "inspect" | "inspect-path" => {
            let path = join_args(args, 1).ok_or_else(|| {
                anyhow::anyhow!("usage: auric audio inspect <path> | auric audio inspect-current")
            })?;
            let inspection = engine.inspect_source_uri(&path)?;
            print_audio_inspection(&inspection);
        }
        "inspect-current" => {
            let current = app
                .playback_state
                .current_entry()
                .ok_or_else(|| anyhow::anyhow!("playback queue has no current track selected"))?;
            let inspection = engine.inspect_source_uri(&current.path)?;
            print_audio_inspection(&inspection);
        }
        "inspect-track-id" => {
            let raw = args.get(1).ok_or_else(|| {
                anyhow::anyhow!("usage: auric audio inspect-track-id <track-id>")
            })?;
            let track_id = TrackId(
                Uuid::parse_str(raw)
                    .with_context(|| format!("invalid track id (expected UUID): {raw}"))?,
            );
            let row = app
                .db
                .get_track_by_id(track_id)?
                .ok_or_else(|| anyhow::anyhow!("track not found by id: {raw}"))?;
            let inspection = engine.inspect_source_uri(&row.path)?;
            print_audio_inspection(&inspection);
        }
        _ => bail!(
            "usage: auric audio [devices | inspect <path> | inspect-current | inspect-track-id <track-id>]"
        ),
    }
    Ok(())
}

fn print_audio_inspection(inspection: &auric_audio::AudioInspection) {
    println!("audio inspection");
    println!("  source_uri: {}", inspection.source_uri);
    println!("  resolved_path: {}", inspection.resolved_path);
    println!("  sample_rate: {}", inspection.format.sample_rate);
    println!("  channels: {}", inspection.format.channels);
    println!("  bit_depth: {}", inspection.format.bit_depth);
}

fn handle_playback_command(app: &mut BootstrappedApp, args: &[String]) -> Result<()> {
    let sub = args.first().map(String::as_str).unwrap_or("status");
    match sub {
        "status" => {
            print_playback_status(app);
        }
        "play" => {
            print_playback_events(dispatch_app_command(app, AppCommand::Play)?);
            print_playback_status(app);
        }
        "pause" => {
            print_playback_events(dispatch_app_command(app, AppCommand::Pause)?);
            print_playback_status(app);
        }
        "stop" => {
            print_playback_events(dispatch_app_command(app, AppCommand::Stop)?);
            print_playback_status(app);
        }
        "next" => {
            print_playback_events(dispatch_app_command(app, AppCommand::Next)?);
            print_playback_status(app);
        }
        "previous" | "prev" => {
            print_playback_events(dispatch_app_command(app, AppCommand::Previous)?);
            print_playback_status(app);
        }
        "seek" => {
            let raw = args
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("usage: auric playback seek <millis>"))?;
            let millis = raw
                .parse::<u64>()
                .with_context(|| format!("invalid seek millis: {raw}"))?;
            print_playback_events(dispatch_app_command(app, AppCommand::SeekMillis(millis))?);
            print_playback_status(app);
        }
        "volume" => {
            let raw = args
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("usage: auric playback volume <0..1>"))?;
            let volume = raw
                .parse::<f32>()
                .with_context(|| format!("invalid volume value: {raw}"))?;
            print_playback_events(dispatch_app_command(app, AppCommand::SetVolume(volume))?);
            print_playback_status(app);
        }
        "repeat" => {
            let raw = args
                .get(1)
                .map(String::as_str)
                .ok_or_else(|| anyhow::anyhow!("usage: auric playback repeat <off|one|all>"))?;
            app.playback_state.session.repeat = parse_repeat_mode(raw)?;
            persist_playback_state(app)?;
            println!("repeat => {}", format_repeat_mode(app.playback_state.session.repeat));
            print_playback_status(app);
        }
        "shuffle" => {
            let raw = args
                .get(1)
                .map(String::as_str)
                .ok_or_else(|| anyhow::anyhow!("usage: auric playback shuffle <on|off>"))?;
            app.playback_state.session.shuffle = parse_bool_toggle(raw)
                .ok_or_else(|| anyhow::anyhow!("usage: auric playback shuffle <on|off>"))?;
            persist_playback_state(app)?;
            println!(
                "shuffle => {}",
                if app.playback_state.session.shuffle {
                    "on"
                } else {
                    "off"
                }
            );
            print_playback_status(app);
        }
        "queue" => {
            handle_playback_queue_command(app, args)?;
        }
        _ => bail!(
            "usage: auric playback [status|play|pause|stop|next|previous|seek <ms>|volume <0..1>|repeat <off|one|all>|shuffle <on|off>|queue ...]"
        ),
    }
    Ok(())
}

fn handle_playback_queue_command(app: &mut BootstrappedApp, args: &[String]) -> Result<()> {
    let sub = args.get(1).map(String::as_str).unwrap_or("list");
    match sub {
        "list" => {
            let mut limit = 50usize;
            let mut i = 2usize;
            while i < args.len() {
                match args[i].as_str() {
                    "--limit" => {
                        let raw = args.get(i + 1).ok_or_else(|| {
                            anyhow::anyhow!("usage: auric playback queue list [--limit N]")
                        })?;
                        limit = raw
                            .parse::<usize>()
                            .with_context(|| format!("invalid --limit value: {raw}"))?;
                        i += 2;
                    }
                    other => bail!(
                        "unknown argument for playback queue list: {other}. usage: auric playback queue list [--limit N]"
                    ),
                }
            }
            print_playback_queue(app, limit);
        }
        "clear" => {
            app.playback_state.queue.clear();
            app.playback_state.session.current_index = None;
            app.playback_state.session.position_ms = 0;
            app.playback_state.session.status = PlaybackStatus::Stopped;
            persist_playback_state(app)?;
            println!("playback queue cleared");
            print_playback_status(app);
        }
        "add-path" => {
            let path = join_args(args, 2).ok_or_else(|| {
                anyhow::anyhow!("usage: auric playback queue add-path <track-path>")
            })?;
            let row = app
                .db
                .get_track_by_path(&path)?
                .ok_or_else(|| anyhow::anyhow!("track not found by path: {path}"))?;
            app.playback_state
                .queue
                .push(playback_queue_entry_from_track_row(row));
            persist_playback_state(app)?;
            println!("queued track path: {path}");
            print_playback_status(app);
        }
        "add-id" => {
            let raw = args.get(2).ok_or_else(|| {
                anyhow::anyhow!("usage: auric playback queue add-id <track-id>")
            })?;
            let track_id = TrackId(Uuid::parse_str(raw).with_context(|| {
                format!("invalid track id (expected UUID): {raw}")
            })?);
            let row = app
                .db
                .get_track_by_id(track_id)?
                .ok_or_else(|| anyhow::anyhow!("track not found by id: {raw}"))?;
            app.playback_state
                .queue
                .push(playback_queue_entry_from_track_row(row));
            persist_playback_state(app)?;
            println!("queued track id: {raw}");
            print_playback_status(app);
        }
        "add-prefix" => {
            let path_prefix = args.get(2).map(String::as_str).ok_or_else(|| {
                anyhow::anyhow!(
                    "usage: auric playback queue add-prefix <path-prefix> [--limit N]"
                )
            })?;
            let mut limit = 200usize;
            let mut i = 3usize;
            while i < args.len() {
                match args[i].as_str() {
                    "--limit" => {
                        let raw = args.get(i + 1).ok_or_else(|| {
                            anyhow::anyhow!(
                                "usage: auric playback queue add-prefix <path-prefix> [--limit N]"
                            )
                        })?;
                        limit = raw
                            .parse::<usize>()
                            .with_context(|| format!("invalid --limit value: {raw}"))?;
                        i += 2;
                    }
                    other => bail!(
                        "unknown argument for playback queue add-prefix: {other}. usage: auric playback queue add-prefix <path-prefix> [--limit N]"
                    ),
                }
            }

            let rows = app.db.list_tracks_by_prefix(path_prefix, limit)?;
            if rows.is_empty() {
                println!("no tracks found under prefix: {path_prefix}");
            } else {
                let before = app.playback_state.queue.len();
                app.playback_state.queue.extend(
                    rows.into_iter()
                        .map(playback_queue_entry_from_track_row),
                );
                persist_playback_state(app)?;
                println!(
                    "queued {} tracks from prefix: {}",
                    app.playback_state.queue.len().saturating_sub(before),
                    path_prefix
                );
                print_playback_status(app);
            }
        }
        "add-playlist" | "load-playlist" => {
            let playlist_id = args.get(2).map(String::as_str).ok_or_else(|| {
                anyhow::anyhow!("usage: auric playback queue {sub} <playlist-id> [--limit N]")
            })?;
            let mut limit = 1_000usize;
            let mut i = 3usize;
            while i < args.len() {
                match args[i].as_str() {
                    "--limit" => {
                        let raw = args.get(i + 1).ok_or_else(|| {
                            anyhow::anyhow!(
                                "usage: auric playback queue {sub} <playlist-id> [--limit N]"
                            )
                        })?;
                        limit = raw
                            .parse::<usize>()
                            .with_context(|| format!("invalid --limit value: {raw}"))?;
                        i += 2;
                    }
                    other => bail!(
                        "unknown argument for playback queue {sub}: {other}. usage: auric playback queue {sub} <playlist-id> [--limit N]"
                    ),
                }
            }
            let rows = app.db.list_playlist_tracks(playlist_id, limit)?;
            if rows.is_empty() {
                println!("playlist has no tracks: {playlist_id}");
            } else {
                let entries = rows
                    .into_iter()
                    .map(|row| playback_queue_entry_from_track_row(row.track))
                    .collect::<Vec<_>>();
                if sub == "load-playlist" {
                    app.playback_state.queue = entries;
                    app.playback_state.session.current_index = None;
                    app.playback_state.session.position_ms = 0;
                    app.playback_state.session.status = PlaybackStatus::Stopped;
                } else {
                    app.playback_state.queue.extend(entries);
                }
                persist_playback_state(app)?;
                println!(
                    "{} queue from playlist: {} (queue_len={})",
                    if sub == "load-playlist" {
                        "loaded"
                    } else {
                        "appended to"
                    },
                    playlist_id,
                    app.playback_state.queue.len()
                );
                print_playback_status(app);
            }
        }
        "remove" => {
            let raw = args.get(2).ok_or_else(|| {
                anyhow::anyhow!("usage: auric playback queue remove <index>")
            })?;
            let index = raw
                .parse::<usize>()
                .with_context(|| format!("invalid queue index: {raw}"))?;
            if index >= app.playback_state.queue.len() {
                bail!(
                    "queue index out of range: {index} (len={})",
                    app.playback_state.queue.len()
                );
            }
            app.playback_state.queue.remove(index);
            adjust_playback_selection_after_queue_removal(&mut app.playback_state, index);
            persist_playback_state(app)?;
            println!("removed queue item: {index}");
            print_playback_status(app);
        }
        "select" | "play" => {
            let raw = args.get(2).ok_or_else(|| {
                anyhow::anyhow!("usage: auric playback queue {sub} <index>")
            })?;
            let index = raw
                .parse::<usize>()
                .with_context(|| format!("invalid queue index: {raw}"))?;
            if index >= app.playback_state.queue.len() {
                bail!(
                    "queue index out of range: {index} (len={})",
                    app.playback_state.queue.len()
                );
            }
            app.playback_state.session.current_index = Some(index);
            app.playback_state.session.position_ms = 0;
            if sub == "play" {
                app.playback_state.session.status = PlaybackStatus::Playing;
            }
            persist_playback_state(app)?;
            println!(
                "queue {} index {}",
                if sub == "play" { "playing" } else { "selected" },
                index
            );
            print_playback_status(app);
        }
        _ => bail!(
            "usage: auric playback queue [list [--limit N] | clear | add-path <track-path> | add-id <track-id> | add-prefix <path-prefix> [--limit N] | add-playlist <playlist-id> [--limit N] | load-playlist <playlist-id> [--limit N] | remove <index> | select <index> | play <index>]"
        ),
    }
    Ok(())
}

fn adjust_playback_selection_after_queue_removal(state: &mut PlaybackState, removed_index: usize) {
    if state.queue.is_empty() {
        state.session.current_index = None;
        state.session.position_ms = 0;
        state.session.status = PlaybackStatus::Stopped;
        return;
    }

    match state.session.current_index {
        Some(idx) if idx > removed_index => state.session.current_index = Some(idx - 1),
        Some(idx) if idx == removed_index => {
            let new_idx = removed_index.min(state.queue.len().saturating_sub(1));
            state.session.current_index = Some(new_idx);
            state.session.position_ms = 0;
        }
        _ => {}
    }
}

fn print_playback_events(events: Vec<AppEvent>) {
    for event in events {
        println!("event: {event:?}");
    }
}

fn print_playback_status(app: &BootstrappedApp) {
    let session = &app.playback_state.session;
    println!("playback");
    println!("  status: {}", format_playback_status(session.status));
    println!("  queue_len: {}", app.playback_state.queue.len());
    println!(
        "  current_index: {}",
        session
            .current_index
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".to_string())
    );
    println!("  position_ms: {}", session.position_ms);
    println!("  volume: {:.3}", session.volume);
    println!("  shuffle: {}", if session.shuffle { "on" } else { "off" });
    println!("  repeat: {}", format_repeat_mode(session.repeat));
    if let Some(entry) = app.playback_state.current_entry() {
        println!(
            "  current_track: {} | {} | {} | {}",
            entry.artist.as_deref().unwrap_or("-"),
            entry.album.as_deref().unwrap_or("-"),
            entry.title.as_deref().unwrap_or("-"),
            entry.path
        );
    } else {
        println!("  current_track: -");
    }
}

fn print_playback_queue(app: &BootstrappedApp, limit: usize) {
    let queue = &app.playback_state.queue;
    if queue.is_empty() {
        println!("playback queue is empty");
        return;
    }

    let capped = limit.max(1);
    for (idx, entry) in queue.iter().take(capped).enumerate() {
        let current_marker = if Some(idx) == app.playback_state.session.current_index {
            ">"
        } else {
            " "
        };
        println!(
            "{} {:>4} | {} | {} | {} | {} | {}Hz {}ch {}bit | {}ms",
            current_marker,
            idx,
            entry.artist.as_deref().unwrap_or("-"),
            entry.album.as_deref().unwrap_or("-"),
            entry.title.as_deref().unwrap_or("-"),
            entry.path,
            entry.sample_rate.unwrap_or_default(),
            entry.channels.unwrap_or_default(),
            entry.bit_depth.unwrap_or_default(),
            entry.duration_ms.unwrap_or_default()
        );
    }
    if queue.len() > capped {
        println!("... {} more queue items", queue.len() - capped);
    }
}

fn parse_repeat_mode(raw: &str) -> Result<RepeatMode> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "off" => Ok(RepeatMode::Off),
        "one" => Ok(RepeatMode::One),
        "all" => Ok(RepeatMode::All),
        other => bail!("invalid repeat mode: {other} (expected off|one|all)"),
    }
}

fn parse_bool_toggle(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "on" | "true" | "1" | "yes" => Some(true),
        "off" | "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

fn format_repeat_mode(mode: RepeatMode) -> &'static str {
    match mode {
        RepeatMode::Off => "off",
        RepeatMode::One => "one",
        RepeatMode::All => "all",
    }
}

fn format_playback_status(status: PlaybackStatus) -> &'static str {
    match status {
        PlaybackStatus::Stopped => "stopped",
        PlaybackStatus::Playing => "playing",
        PlaybackStatus::Paused => "paused",
    }
}

fn handle_artwork_command(app: &BootstrappedApp, args: &[String]) -> Result<()> {
    let sub = args.first().map(String::as_str).unwrap_or("stats");
    match sub {
        "stats" => {
            let stats = app.db.stats()?;
            let total_bytes = app.db.artwork_total_bytes()?;
            println!("artwork cache");
            println!("  assets: {}", stats.artwork_asset_count);
            println!("  track_links: {}", stats.track_artwork_count);
            println!("  total_bytes: {}", total_bytes);
        }
        "list" => {
            let mut limit = 20usize;
            let mut i = 1usize;
            while i < args.len() {
                match args[i].as_str() {
                    "--limit" => {
                        let raw = args.get(i + 1).ok_or_else(|| {
                            anyhow::anyhow!("usage: auric artwork list [--limit N]")
                        })?;
                        limit = raw
                            .parse::<usize>()
                            .with_context(|| format!("invalid --limit value: {raw}"))?;
                        i += 2;
                    }
                    other => {
                        bail!(
                            "unknown argument for artwork list: {other}. usage: auric artwork list [--limit N]"
                        );
                    }
                }
            }
            let rows = app.db.list_artwork_assets(limit)?;
            if rows.is_empty() {
                println!("no cached artwork");
            } else {
                for row in rows {
                    println!(
                        "{} | {} bytes | {} | {} | {}",
                        row.id,
                        row.byte_len,
                        row.mime_type.as_deref().unwrap_or("-"),
                        row.picture_type.as_deref().unwrap_or("-"),
                        row.sha256_hex
                    );
                }
            }
        }
        "track" => {
            let path = join_args(args, 1)
                .ok_or_else(|| anyhow::anyhow!("usage: auric artwork track <track-path>"))?;
            match app.db.get_track_artwork_by_path(&path)? {
                Some(row) => {
                    println!("track artwork");
                    println!("  track_id: {}", row.track_id.0);
                    println!("  path: {}", row.track_path);
                    println!("  artwork_id: {}", row.artwork_id);
                    println!("  source: {}", row.source);
                    println!("  mime_type: {}", row.mime_type.as_deref().unwrap_or("-"));
                    println!(
                        "  picture_type: {}",
                        row.picture_type.as_deref().unwrap_or("-")
                    );
                    println!("  byte_len: {}", row.byte_len);
                    println!("  sha256: {}", row.sha256_hex);
                }
                None => println!("no artwork mapped for track: {path}"),
            }
        }
        "purge-orphans" => {
            let removed = app.db.purge_orphan_artwork_assets()?;
            println!("purged orphan artwork assets: {}", removed);
        }
        _ => bail!(
            "usage: auric artwork [stats | list [--limit N] | track <track-path> | purge-orphans]"
        ),
    }
    Ok(())
}

fn handle_ui_command(app: &mut BootstrappedApp, args: &[String]) -> Result<()> {
    let sub = args.first().map(String::as_str).unwrap_or("preview");
    match sub {
        "render-once" => {
            let mut width = 120u16;
            let mut height = 34u16;
            let mut i = 1usize;
            while i < args.len() {
                match args[i].as_str() {
                    "--width" => {
                        let raw = args.get(i + 1).ok_or_else(|| {
                            anyhow::anyhow!("usage: auric ui render-once [--width N] [--height N]")
                        })?;
                        width = parse_u16_arg(raw, "--width")?;
                        i += 2;
                    }
                    "--height" => {
                        let raw = args.get(i + 1).ok_or_else(|| {
                            anyhow::anyhow!(
                                "usage: auric ui render-once [--width N] [--height N]"
                            )
                        })?;
                        height = parse_u16_arg(raw, "--height")?;
                        i += 2;
                    }
                    other => bail!(
                        "unknown argument for ui render-once: {other}. usage: auric ui render-once [--width N] [--height N]"
                    ),
                }
            }

            let (palette, snapshot) = load_ui_palette_and_snapshot(app);
            let mut state = ShellState::new(snapshot);
            let rendered = render_once_to_text(&mut state, &palette, width, height)?;
            println!("{rendered}");
        }
        "preview" => {
            let mouse = !has_flag(args, "--no-mouse");
            let (palette, snapshot) = load_ui_palette_and_snapshot(app);
            let mut state = ShellState::new(snapshot);
            let app_cell = std::cell::RefCell::new(app);
            let lib_config = {
                let app_ref = app_cell.borrow();
                app_ref.config.library.clone()
            };
            let db_options = {
                let app_ref = app_cell.borrow();
                let cwd = env::current_dir().unwrap_or_default();
                app_ref.config.database.to_options(&cwd).unwrap_or_default()
            };
            run_interactive_full(
                &mut state,
                &palette,
                RunOptions {
                    mouse,
                    ..RunOptions::default()
                },
                || {
                    let app_ref = app_cell.borrow();
                    Ok(build_shell_snapshot(&app_ref))
                },
                |input| {
                    let mut app_ref = app_cell.borrow_mut();
                    execute_ui_palette_command(&mut app_ref, input).map_err(|e| {
                        auric_ui::UiError::Terminal(format!("palette command failed: {e}"))
                    })
                },
                {
                    let lib_config = lib_config.clone();
                    let db_options = db_options.clone();
                    move |scan_path: String| {
                        let (tx, rx) = std::sync::mpsc::channel();
                        let lib_config = lib_config.clone();
                        let db_options = db_options.clone();
                        std::thread::spawn(move || {
                            let done = std::sync::Arc::new(
                                std::sync::atomic::AtomicBool::new(false),
                            );

                            // Progress poller: check DB track count periodically
                            let progress_tx = tx.clone();
                            let progress_db_opts = db_options.clone();
                            let progress_path = scan_path.clone();
                            let progress_done = std::sync::Arc::clone(&done);
                            std::thread::spawn(move || {
                                let db = Database::open(&progress_db_opts).ok();
                                while !progress_done.load(
                                    std::sync::atomic::Ordering::Relaxed,
                                ) {
                                    std::thread::sleep(std::time::Duration::from_millis(750));
                                    if let Some(ref db) = db {
                                        let count = db.stats().map(|s| s.track_count).unwrap_or(0);
                                        let _ = progress_tx.send(ScanProgress::Progress {
                                            discovered: count as usize,
                                            path: progress_path.clone(),
                                        });
                                    }
                                }
                            });

                            let scan_result = (|| -> anyhow::Result<ScanSummary> {
                                let mut db = Database::open(&db_options)?;
                                let scanner = scanner_from_config(&lib_config, false);
                                let summary = scanner.scan_path(
                                    &mut db,
                                    std::path::Path::new(&scan_path),
                                )?;
                                Ok(summary)
                            })();

                            done.store(true, std::sync::atomic::Ordering::Relaxed);

                            match scan_result {
                                Ok(summary) => {
                                    let _ = tx.send(ScanProgress::Done {
                                        message: format!(
                                            "Scan complete: {} ({} tracks imported in {:.1}s)",
                                            summary.root_path,
                                            summary.imported_tracks,
                                            summary.elapsed_ms as f64 / 1000.0,
                                        ),
                                    });
                                }
                                Err(err) => {
                                    let _ = tx.send(ScanProgress::Error {
                                        message: format!("{err:#}"),
                                    });
                                }
                            }
                        });
                        rx
                    }
                },
                |action: PlaybackAction| {
                    let mut app_ref = app_cell.borrow_mut();
                    handle_tui_playback_action(&mut app_ref, action).map_err(|e| {
                        auric_ui::UiError::Terminal(format!("playback error: {e}"))
                    })
                },
                || {
                    let app_ref = app_cell.borrow();
                    let events = app_ref.player.poll_events();
                    events
                        .into_iter()
                        .filter_map(|evt| match evt {
                            auric_audio::player::PlayerEvent::Position {
                                position_ms,
                                duration_ms,
                            } => Some(PlayerEventUpdate {
                                position_ms,
                                duration_ms,
                                status: "playing".to_string(),
                                track_finished: false,
                            }),
                            auric_audio::player::PlayerEvent::TrackFinished => {
                                Some(PlayerEventUpdate {
                                    position_ms: 0,
                                    duration_ms: 0,
                                    status: "stopped".to_string(),
                                    track_finished: true,
                                })
                            }
                            auric_audio::player::PlayerEvent::Paused => {
                                Some(PlayerEventUpdate {
                                    position_ms: 0,
                                    duration_ms: 0,
                                    status: "paused".to_string(),
                                    track_finished: false,
                                })
                            }
                            auric_audio::player::PlayerEvent::Stopped => {
                                Some(PlayerEventUpdate {
                                    position_ms: 0,
                                    duration_ms: 0,
                                    status: "stopped".to_string(),
                                    track_finished: false,
                                })
                            }
                            _ => None,
                        })
                        .collect()
                },
            )?;
        }
        "themes" => {
            let store = FsThemeStore::new(default_theme_dir());
            for theme in store.list().unwrap_or_default() {
                println!("{theme}");
            }
        }
        _ => bail!("usage: auric ui [render-once [--width N] [--height N] | preview [--no-mouse] | themes]"),
    }
    Ok(())
}

fn load_ui_palette_and_snapshot(app: &BootstrappedApp) -> (Palette, ShellSnapshot) {
    let store = FsThemeStore::new(default_theme_dir());
    let mut palette = match store.load_palette(&app.config.ui.theme) {
        Ok(p) => p,
        Err(err) => {
            eprintln!(
                "auric ui warning: failed to load theme '{}': {err}. using default palette",
                app.config.ui.theme
            );
            Palette::default()
        }
    };
    palette.use_terminal_bg = !app.config.ui.use_theme_background;
    let snapshot = build_shell_snapshot(app);
    (palette, snapshot)
}

fn default_theme_dir() -> PathBuf {
    env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("themes")
}

fn execute_ui_palette_command(
    app: &mut BootstrappedApp,
    input: &str,
) -> Result<PaletteCommandResult> {
    let command = input.trim();
    if command.is_empty() {
        return Ok(PaletteCommandResult::new("No command entered", false));
    }

    let words = command.split_whitespace().collect::<Vec<_>>();
    let head = words.first().copied().unwrap_or_default();

    match head {
        "help" | "?" => Ok(PaletteCommandResult::new(
            "Palette commands: help, refresh, feature [list|enable|disable], scan [roots|path], root [list|add], playlist [list|create|rename|delete]",
            false,
        )),
        "refresh" | "reload" => Ok(PaletteCommandResult::new(
            "Refresh requested",
            true,
        )),
        "feature" => execute_palette_feature_command(app, &words),
        "scan" => execute_palette_scan_command(app, command, &words),
        "root" => execute_palette_root_command(app, command, &words),
        "playlist" => execute_palette_playlist_command(app, command, &words),
        "watch" => Ok(PaletteCommandResult::new(
            "watch commands are not supported in the interactive shell (run from CLI)",
            false,
        )),
        "__add_root" => {
            let path = strip_n_words(command, 1)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("internal error: __add_root with no path"))?;
            app.db.upsert_library_root(&LibraryRoot {
                path: path.clone(),
                watched: true,
            })?;
            Ok(PaletteCommandResult::with_background_scan(
                format!("Added {path}, scanning..."),
                path,
            ))
        }
        other => Ok(PaletteCommandResult::new(
            format!("Unknown command: {other} (use 'help')"),
            false,
        )),
    }
}

fn execute_palette_feature_command(
    app: &mut BootstrappedApp,
    words: &[&str],
) -> Result<PaletteCommandResult> {
    let sub = words.get(1).copied().unwrap_or("list");
    match sub {
        "list" => {
            let enabled = FeatureId::ALL
                .into_iter()
                .filter(|f| app.feature_registry.is_enabled(*f))
                .count();
            Ok(PaletteCommandResult::new(
                format!(
                    "Features enabled: {enabled}/{} (use 'feature enable <name>' or 'feature disable <name>')",
                    FeatureId::ALL.len()
                ),
                true,
            ))
        }
        "enable" | "disable" => {
            let raw = words
                .get(2)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("usage: feature {sub} <feature-name>"))?;
            let feature = FeatureId::from_key(raw).ok_or_else(|| {
                anyhow::anyhow!(
                    "unknown feature: {raw} (valid: {})",
                    FeatureId::ALL
                        .into_iter()
                        .map(|f| f.as_key())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
            let enabled = sub == "enable";
            let _events =
                dispatch_app_command(app, AppCommand::ToggleFeature { feature, enabled })?;
            Ok(PaletteCommandResult::new(
                format!(
                    "feature {} {}",
                    feature,
                    if enabled { "enabled" } else { "disabled" }
                ),
                true,
            ))
        }
        _ => Ok(PaletteCommandResult::new(
            "usage: feature [list|enable <name>|disable <name>]",
            false,
        )),
    }
}

fn execute_palette_scan_command(
    app: &mut BootstrappedApp,
    command: &str,
    words: &[&str],
) -> Result<PaletteCommandResult> {
    let sub = words.get(1).copied().unwrap_or("roots");
    let prune = words.contains(&"--prune");
    let scanner = scanner_from_config(&app.config.library, prune);
    match sub {
        "roots" => {
            let summaries = scanner.scan_saved_roots(&mut app.db)?;
            if summaries.is_empty() {
                Ok(PaletteCommandResult::new(
                    "No library roots configured",
                    false,
                ))
            } else {
                let imported = summaries.iter().map(|s| s.imported_tracks).sum::<usize>();
                let rescanned = summaries.len();
                Ok(PaletteCommandResult::new(
                    format!("Scanned {rescanned} roots (imported {imported} tracks)"),
                    true,
                ))
            }
        }
        "path" => {
            let path = strip_n_words(command, 2)
                .and_then(|s| s.split(" --").next().map(str::trim).map(str::to_string))
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("usage: scan path <dir> [--prune]"))?;
            let summary = scanner.scan_path(&mut app.db, Path::new(&path))?;
            Ok(PaletteCommandResult::new(
                format!(
                    "Scanned {} (imported {}, pruned {})",
                    summary.root_path, summary.imported_tracks, summary.pruned_missing_tracks
                ),
                true,
            ))
        }
        _ => Ok(PaletteCommandResult::new(
            "usage: scan [roots [--prune] | path <dir> [--prune]]",
            false,
        )),
    }
}

fn execute_palette_root_command(
    app: &mut BootstrappedApp,
    command: &str,
    words: &[&str],
) -> Result<PaletteCommandResult> {
    let sub = words.get(1).copied().unwrap_or("list");
    match sub {
        "list" => {
            let count = app.db.list_library_roots()?.len();
            Ok(PaletteCommandResult::new(
                format!("Library roots: {count}"),
                true,
            ))
        }
        "add" => {
            let watched = words.contains(&"--watched") || words.contains(&"watched");
            let path = strip_n_words(command, 2)
                .map(|s| s.replace("--watched", "").replace(" watched", ""))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("usage: root add <path> [--watched]"))?;
            let row = app.db.upsert_library_root(&LibraryRoot { path, watched })?;
            Ok(PaletteCommandResult::new(
                format!("Root saved: {} (watched={})", row.path, row.watched),
                true,
            ))
        }
        _ => Ok(PaletteCommandResult::new(
            "usage: root [list | add <path> [--watched]]",
            false,
        )),
    }
}

fn execute_palette_playlist_command(
    app: &mut BootstrappedApp,
    command: &str,
    words: &[&str],
) -> Result<PaletteCommandResult> {
    let sub = words.get(1).copied().unwrap_or("list");
    match sub {
        "list" => {
            let count = app.db.list_playlists()?.len();
            Ok(PaletteCommandResult::new(
                format!("Playlists: {count}"),
                true,
            ))
        }
        "create" => {
            let name = strip_n_words(command, 2)
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("usage: playlist create <name>"))?;
            let id = app.db.create_playlist(name.trim())?;
            Ok(PaletteCommandResult::new(
                format!("Playlist created: {} | {}", id, name.trim()),
                true,
            ))
        }
        "rename" => {
            let id = words
                .get(2)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("usage: playlist rename <id> <name>"))?;
            let name = strip_n_words(command, 3)
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("usage: playlist rename <id> <name>"))?;
            app.db.rename_playlist(id, name.trim())?;
            Ok(PaletteCommandResult::new(
                format!("Playlist renamed: {} | {}", id, name.trim()),
                true,
            ))
        }
        "delete" => {
            let id = words
                .get(2)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("usage: playlist delete <id>"))?;
            app.db.delete_playlist(id)?;
            Ok(PaletteCommandResult::new(
                format!("Playlist deleted: {id}"),
                true,
            ))
        }
        _ => Ok(PaletteCommandResult::new(
            "usage: playlist [list|create <name>|rename <id> <name>|delete <id>]",
            false,
        )),
    }
}

fn strip_n_words(input: &str, n: usize) -> Option<String> {
    let mut in_word = false;
    let mut words_seen = 0usize;
    for (idx, ch) in input.char_indices() {
        if ch.is_whitespace() {
            in_word = false;
            continue;
        }
        if !in_word {
            in_word = true;
            if words_seen == n {
                return Some(input[idx..].to_string());
            }
            words_seen += 1;
        }
    }
    None
}

fn build_shell_snapshot(app: &BootstrappedApp) -> ShellSnapshot {
    let stats = app.db.stats().unwrap_or_else(|err| {
        eprintln!("warning: failed to load database stats: {err}");
        app.report.stats.clone()
    });

    let roots = app
        .db
        .list_library_roots()
        .unwrap_or_default()
        .into_iter()
        .map(|row| ShellListItem {
            id: row.id,
            label: row.path,
            detail: Some(if row.watched {
                "watched".to_string()
            } else {
                "manual".to_string()
            }),
        })
        .collect::<Vec<_>>();

    let playlists = app
        .db
        .list_playlists()
        .unwrap_or_default()
        .into_iter()
        .map(|row| ShellListItem {
            id: row.id,
            label: row.name,
            detail: None,
        })
        .collect::<Vec<_>>();

    let tracks = app
        .db
        .list_tracks(stats.track_count.max(250) as usize)
        .unwrap_or_default()
        .into_iter()
        .map(|row| ShellTrackItem {
            id: row.id.0.to_string(),
            title: row.title.unwrap_or_else(|| "-".to_string()),
            artist: row.artist.unwrap_or_else(|| "-".to_string()),
            album: row.album.unwrap_or_else(|| "-".to_string()),
            path: row.path,
            duration_ms: row.duration_ms,
            sample_rate: row.sample_rate,
            channels: row.channels,
            bit_depth: row.bit_depth,
        })
        .collect::<Vec<_>>();

    let feature_summary = FeatureId::ALL
        .into_iter()
        .map(|feature| {
            (
                feature.as_key().to_string(),
                app.feature_registry.is_enabled(feature),
            )
        })
        .collect::<Vec<_>>();

    let mouse_enabled =
        app.feature_registry.is_enabled(FeatureId::Mouse) && app.config.features.mouse;
    let feature_enabled_count = FeatureId::ALL
        .into_iter()
        .filter(|feature| app.feature_registry.is_enabled(*feature))
        .count();

    ShellSnapshot {
        app_title: "auric".to_string(),
        theme_name: app.config.ui.theme.clone(),
        color_scheme: app.config.ui.color_scheme.clone(),
        icon_mode: IconMode::from_config(&app.config.ui.icon_pack),
        icon_fallback: app.config.ui.icon_fallback.clone(),
        preferred_terminal_font: app.config.ui.preferred_terminal_font.clone(),
        mouse_enabled,
        artwork_filter: if app.config.ui.pixel_art_artwork {
            "pixel-art".to_string()
        } else {
            app.config.ui.artwork_display_filter.clone()
        },
        pixel_art_enabled: app.config.ui.pixel_art_artwork,
        pixel_art_cell_size: app.config.ui.pixel_art_cell_size,
        roots,
        playlists,
        tracks,
        feature_summary,
        status_lines: vec![
            format!(
                "db={} tracks={} artwork={} playlists={} roots={}",
                app.report
                    .db_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "<memory>".to_string()),
                stats.track_count,
                stats.artwork_asset_count,
                stats.playlist_count,
                stats.library_root_count
            ),
            format!(
                "features={}/{} mouse={} icons={}",
                feature_enabled_count,
                app.report.feature_total_count,
                if mouse_enabled { "on" } else { "off" },
                app.config.ui.icon_pack
            ),
        ],
        playback_status: match app.playback_state.session.status {
            PlaybackStatus::Playing => "playing",
            PlaybackStatus::Paused => "paused",
            PlaybackStatus::Stopped => "stopped",
        }
        .to_string(),
        now_playing_title: app
            .playback_state
            .current_entry()
            .and_then(|e| e.title.clone())
            .unwrap_or_default(),
        now_playing_artist: app
            .playback_state
            .current_entry()
            .and_then(|e| e.artist.clone())
            .unwrap_or_default(),
        now_playing_album: app
            .playback_state
            .current_entry()
            .and_then(|e| e.album.clone())
            .unwrap_or_default(),
        now_playing_duration_ms: app
            .playback_state
            .current_entry()
            .and_then(|e| e.duration_ms)
            .unwrap_or(0) as u64,
        now_playing_position_ms: app.playback_state.session.position_ms,
        volume: app.playback_state.session.volume,
        shuffle: app.playback_state.session.shuffle,
        repeat_mode: match app.playback_state.session.repeat {
            RepeatMode::Off => "off",
            RepeatMode::One => "one",
            RepeatMode::All => "all",
        }
        .to_string(),
        queue_length: app.playback_state.queue.len(),
        queue_position: app
            .playback_state
            .session
            .current_index
            .map(|i| i + 1)
            .unwrap_or(0),
    }
}

fn parse_u16_arg(raw: &str, flag: &str) -> Result<u16> {
    let value = raw
        .parse::<u16>()
        .with_context(|| format!("invalid value for {flag}: {raw}"))?;
    if value == 0 {
        bail!("{flag} must be > 0");
    }
    Ok(value)
}

fn print_bootstrap_report(report: &BootstrapReport) {
    println!("auric bootstrap ready");
    println!("  config: {}", report.config_path.display());
    if let Some(path) = &report.db_path {
        println!("  db: {}", path.display());
    }
    println!("  schema_version: {}", report.schema_version);
    println!(
        "  features: enabled={}/{}",
        report.feature_enabled_count, report.feature_total_count
    );
    println!(
        "  ui: theme={} color_scheme={} icon_pack={}",
        report.ui_theme, report.ui_color_scheme, report.ui_icon_pack
    );
    println!(
        "  sqlite: journal_mode={} synchronous={} foreign_keys={} cache_size={} mmap_size={}",
        report.pragmas.journal_mode,
        report.pragmas.synchronous,
        report.pragmas.foreign_keys,
        report.pragmas.cache_size,
        report.pragmas.mmap_size,
    );
    println!(
        "  stats: settings={} roots={} tracks={} artwork_assets={} track_artwork={} playlists={} entries={} db_size_bytes={}",
        report.stats.settings_count,
        report.stats.library_root_count,
        report.stats.track_count,
        report.stats.artwork_asset_count,
        report.stats.track_artwork_count,
        report.stats.playlist_count,
        report.stats.playlist_entry_count,
        report.stats.db_size_bytes
    );
}

fn run_db_stress(db: &mut Database, total: usize) -> Result<()> {
    const CHUNK_SIZE: usize = 2_000;

    let prefix = Uuid::new_v4().to_string();
    let started = Instant::now();
    let mut inserted = 0usize;
    let mut batch = Vec::with_capacity(CHUNK_SIZE);

    for start in (0..total).step_by(CHUNK_SIZE) {
        batch.clear();
        let end = (start + CHUNK_SIZE).min(total);
        for i in start..end {
            batch.push(TrackRecord {
                id: TrackId(Uuid::new_v4()),
                path: format!("/stress/{prefix}/{i:06}.flac"),
                title: Some(format!("Stress Track {i}")),
                artist: Some("Auric Stress".to_string()),
                album: Some("DB Stress".to_string()),
                duration_ms: None,
                sample_rate: None,
                channels: None,
                bit_depth: None,
                file_mtime_ms: None,
            });
        }
        db.upsert_tracks_batch(&batch)
            .map_err(anyhow::Error::from)
            .with_context(|| format!("failed upserting stress batch {start}..{end}"))?;
        inserted += batch.len();
    }

    let elapsed = started.elapsed();
    let stats = db.stats()?;
    let tps = if elapsed.as_secs_f64() > 0.0 {
        inserted as f64 / elapsed.as_secs_f64()
    } else {
        inserted as f64
    };

    println!("db stress complete");
    println!("  inserted_tracks: {inserted}");
    println!("  elapsed_ms: {}", elapsed.as_millis());
    println!("  throughput_tracks_per_sec: {:.1}", tps);
    println!(
        "  totals: tracks={} playlists={} db_size_bytes={}",
        stats.track_count, stats.playlist_count, stats.db_size_bytes
    );

    Ok(())
}

fn parse_journal_mode(raw: &str) -> Result<JournalMode> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "wal" => Ok(JournalMode::Wal),
        "delete" => Ok(JournalMode::Delete),
        "memory" => Ok(JournalMode::Memory),
        other => bail!("unsupported database.journal_mode: {other}"),
    }
}

fn parse_synchronous_mode(raw: &str) -> Result<SynchronousMode> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "off" => Ok(SynchronousMode::Off),
        "normal" => Ok(SynchronousMode::Normal),
        "full" => Ok(SynchronousMode::Full),
        other => bail!("unsupported database.synchronous: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn parses_default_config() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/default.toml");
        let cfg = AppConfig::load_from_path(&path).unwrap();
        assert_eq!(cfg.ui.theme, "auric-dark");
        assert_eq!(cfg.ui.color_scheme, "dark");
        assert_eq!(cfg.ui.artwork_display_filter, "none");
        assert!(!cfg.ui.pixel_art_artwork);
        assert_eq!(cfg.ui.pixel_art_cell_size, 2);
        assert_eq!(cfg.ui.icon_pack, "nerd-font");
        assert!(cfg.features.metadata);
        assert!(!cfg.features.visualizer);
        assert_eq!(cfg.database.journal_mode, "wal");
    }

    #[test]
    fn bootstrap_creates_database_and_seeds_settings() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("auric-test.db");
        let cfg_path = dir.path().join("auric-test.toml");

        fs::write(
            &cfg_path,
            format!(
                "[ui]\ntheme = \"auric-light\"\ncolor_scheme = \"light\"\nicon_pack = \"ascii\"\nicon_fallback = \"ascii\"\npreferred_terminal_font = \"FiraCode Nerd Font Mono\"\n\n[database]\npath = \"{}\"\njournal_mode = \"wal\"\nsynchronous = \"normal\"\n",
                db_path.display()
            ),
        )
        .unwrap();

        let app = bootstrap_from_config_path(&cfg_path).unwrap();
        assert!(db_path.exists());
        assert_eq!(app.report.schema_version, 2);
        assert_eq!(
            app.db.get_setting_json("ui.theme").unwrap(),
            Some(json!("auric-light"))
        );
        assert_eq!(
            app.db.get_setting_json("ui.icon_pack").unwrap(),
            Some(json!("ascii"))
        );
        assert_eq!(
            app.db.get_setting_json("ui.pixel_art_artwork").unwrap(),
            Some(json!(false))
        );
        assert_eq!(
            app.db
                .get_setting_json("feature.visualizer.enabled")
                .unwrap(),
            Some(json!(false))
        );
        assert!(!app.feature_registry.is_enabled(FeatureId::Visualizer));
    }

    #[test]
    fn feature_toggle_dispatch_persists_state() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("auric-test.db");
        let cfg_path = dir.path().join("auric-test.toml");

        fs::write(
            &cfg_path,
            format!("[database]\npath = \"{}\"\n", db_path.display()),
        )
        .unwrap();

        let mut app = bootstrap_from_config_path(&cfg_path).unwrap();
        let events = dispatch_app_command(
            &mut app,
            AppCommand::ToggleFeature {
                feature: FeatureId::Visualizer,
                enabled: true,
            },
        )
        .unwrap();

        assert_eq!(events.len(), 2);
        assert!(app.feature_registry.is_enabled(FeatureId::Visualizer));
        assert_eq!(
            app.db
                .get_setting_json("feature.visualizer.enabled")
                .unwrap(),
            Some(json!(true))
        );
    }

    #[test]
    fn root_and_playlist_cli_commands_use_db() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("auric-test.db");
        let cfg_path = dir.path().join("auric-test.toml");

        fs::write(
            &cfg_path,
            format!("[database]\npath = \"{}\"\n", db_path.display()),
        )
        .unwrap();

        let app = bootstrap_from_config_path(&cfg_path).unwrap();
        let music_dir = dir.path().join("music");
        fs::create_dir(&music_dir).unwrap();
        handle_root_command(
            &app,
            &[
                String::from("add"),
                music_dir.to_string_lossy().to_string(),
                String::from("--watched"),
            ],
        )
        .unwrap();
        handle_playlist_command(
            &app,
            &[
                String::from("create"),
                String::from("Road"),
                String::from("Trip"),
            ],
        )
        .unwrap();

        assert_eq!(app.db.list_library_roots().unwrap().len(), 1);
        let playlists = app.db.list_playlists().unwrap();
        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0].name, "Road Trip");
    }

    #[test]
    fn playback_queue_and_session_persist_across_bootstrap() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("auric-test.db");
        let cfg_path = dir.path().join("auric-test.toml");

        fs::write(
            &cfg_path,
            format!("[database]\npath = \"{}\"\n", db_path.display()),
        )
        .unwrap();

        let mut app = bootstrap_from_config_path(&cfg_path).unwrap();
        let track_a = TrackRecord {
            id: TrackId(Uuid::new_v4()),
            path: "/tmp/test-a.flac".to_string(),
            title: Some("Track A".to_string()),
            artist: Some("Artist".to_string()),
            album: Some("Album".to_string()),
            duration_ms: Some(120_000),
            sample_rate: Some(96_000),
            channels: Some(2),
            bit_depth: Some(24),
            file_mtime_ms: Some(1),
        };
        let track_b = TrackRecord {
            id: TrackId(Uuid::new_v4()),
            path: "/tmp/test-b.flac".to_string(),
            title: Some("Track B".to_string()),
            artist: Some("Artist".to_string()),
            album: Some("Album".to_string()),
            duration_ms: Some(150_000),
            sample_rate: Some(44_100),
            channels: Some(2),
            bit_depth: Some(16),
            file_mtime_ms: Some(2),
        };
        app.db.upsert_track(&track_a).unwrap();
        app.db.upsert_track(&track_b).unwrap();

        handle_playback_command(
            &mut app,
            &[
                String::from("queue"),
                String::from("add-path"),
                track_a.path.clone(),
            ],
        )
        .unwrap();
        handle_playback_command(
            &mut app,
            &[
                String::from("queue"),
                String::from("add-path"),
                track_b.path.clone(),
            ],
        )
        .unwrap();
        handle_playback_command(
            &mut app,
            &[
                String::from("queue"),
                String::from("play"),
                String::from("1"),
            ],
        )
        .unwrap();
        let _ = dispatch_app_command(&mut app, AppCommand::SeekMillis(42_000)).unwrap();
        let _ = dispatch_app_command(&mut app, AppCommand::SetVolume(0.65)).unwrap();
        app.playback_state.session.repeat = RepeatMode::All;
        persist_playback_state(&mut app).unwrap();

        let app2 = bootstrap_from_config_path(&cfg_path).unwrap();
        assert_eq!(app2.playback_state.queue.len(), 2);
        assert_eq!(app2.playback_state.session.current_index, Some(1));
        assert_eq!(app2.playback_state.session.status, PlaybackStatus::Playing);
        assert_eq!(app2.playback_state.session.position_ms, 42_000);
        assert!((app2.playback_state.session.volume - 0.65).abs() < 0.0001);
        assert_eq!(app2.playback_state.session.repeat, RepeatMode::All);
        assert_eq!(
            app2.playback_state
                .current_entry()
                .and_then(|e| e.title.as_deref()),
            Some("Track B")
        );
    }

    #[test]
    fn playback_transport_next_previous_updates_selection() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("auric-test.db");
        let cfg_path = dir.path().join("auric-test.toml");

        fs::write(
            &cfg_path,
            format!("[database]\npath = \"{}\"\n", db_path.display()),
        )
        .unwrap();

        let mut app = bootstrap_from_config_path(&cfg_path).unwrap();
        for i in 0..3 {
            app.db
                .upsert_track(&TrackRecord {
                    id: TrackId(Uuid::new_v4()),
                    path: format!("/tmp/t-{i}.flac"),
                    title: Some(format!("T{i}")),
                    artist: Some("A".to_string()),
                    album: Some("B".to_string()),
                    duration_ms: Some(1000),
                    sample_rate: Some(44_100),
                    channels: Some(2),
                    bit_depth: Some(16),
                    file_mtime_ms: Some(i as i64),
                })
                .unwrap();
        }
        let rows = app.db.list_tracks_by_prefix("/tmp", 10).unwrap();
        app.playback_state.queue = rows
            .into_iter()
            .map(playback_queue_entry_from_track_row)
            .collect();
        app.playback_state.session.current_index = Some(0);
        app.playback_state.session.status = PlaybackStatus::Playing;
        persist_playback_state(&mut app).unwrap();

        dispatch_app_command(&mut app, AppCommand::Next).unwrap();
        assert_eq!(app.playback_state.session.current_index, Some(1));
        assert_eq!(app.playback_state.session.position_ms, 0);

        app.playback_state.session.position_ms = 4_000;
        persist_playback_state(&mut app).unwrap();
        dispatch_app_command(&mut app, AppCommand::Previous).unwrap();
        assert_eq!(app.playback_state.session.current_index, Some(1));
        assert_eq!(app.playback_state.session.position_ms, 0);

        dispatch_app_command(&mut app, AppCommand::Previous).unwrap();
        assert_eq!(app.playback_state.session.current_index, Some(0));

        dispatch_app_command(&mut app, AppCommand::Previous).unwrap();
        assert_eq!(app.playback_state.session.current_index, Some(0));
    }

    #[test]
    fn playlist_tracks_can_be_edited_and_loaded_into_playback_queue() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("auric-test.db");
        let cfg_path = dir.path().join("auric-test.toml");

        fs::write(
            &cfg_path,
            format!("[database]\npath = \"{}\"\n", db_path.display()),
        )
        .unwrap();

        let mut app = bootstrap_from_config_path(&cfg_path).unwrap();
        let playlist_id = app.db.create_playlist("Queue Source").unwrap();
        let tracks = [
            TrackRecord {
                id: TrackId(Uuid::new_v4()),
                path: "/tmp/pl-a.flac".to_string(),
                title: Some("PLA".to_string()),
                artist: Some("Artist".to_string()),
                album: Some("Album".to_string()),
                duration_ms: Some(1000),
                sample_rate: Some(44_100),
                channels: Some(2),
                bit_depth: Some(16),
                file_mtime_ms: Some(1),
            },
            TrackRecord {
                id: TrackId(Uuid::new_v4()),
                path: "/tmp/pl-b.flac".to_string(),
                title: Some("PLB".to_string()),
                artist: Some("Artist".to_string()),
                album: Some("Album".to_string()),
                duration_ms: Some(2000),
                sample_rate: Some(48_000),
                channels: Some(2),
                bit_depth: Some(24),
                file_mtime_ms: Some(2),
            },
        ];
        for track in &tracks {
            app.db.upsert_track(track).unwrap();
            handle_playlist_command(
                &app,
                &[
                    String::from("add-track"),
                    playlist_id.clone(),
                    track.path.clone(),
                ],
            )
            .unwrap();
        }

        let playlist_rows = app.db.list_playlist_tracks(&playlist_id, 10).unwrap();
        assert_eq!(playlist_rows.len(), 2);
        assert_eq!(playlist_rows[0].position, 0);
        assert_eq!(playlist_rows[1].position, 1);

        handle_playlist_command(
            &app,
            &[
                String::from("remove-track"),
                playlist_id.clone(),
                String::from("0"),
            ],
        )
        .unwrap();
        let playlist_rows = app.db.list_playlist_tracks(&playlist_id, 10).unwrap();
        assert_eq!(playlist_rows.len(), 1);
        assert_eq!(playlist_rows[0].position, 0);
        assert_eq!(playlist_rows[0].track.title.as_deref(), Some("PLB"));

        handle_playback_command(
            &mut app,
            &[
                String::from("queue"),
                String::from("load-playlist"),
                playlist_id.clone(),
            ],
        )
        .unwrap();
        assert_eq!(app.playback_state.queue.len(), 1);
        assert_eq!(app.playback_state.queue[0].title.as_deref(), Some("PLB"));
        assert_eq!(app.playback_state.session.current_index, None);
        assert_eq!(app.playback_state.session.status, PlaybackStatus::Stopped);
    }

    #[test]
    fn ui_palette_commands_mutate_state_and_return_refresh_hints() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("auric-test.db");
        let cfg_path = dir.path().join("auric-test.toml");

        fs::write(
            &cfg_path,
            format!("[database]\npath = \"{}\"\n", db_path.display()),
        )
        .unwrap();

        let mut app = bootstrap_from_config_path(&cfg_path).unwrap();

        let result = execute_ui_palette_command(&mut app, "feature enable visualizer").unwrap();
        assert!(result.refresh_requested);
        assert!(app.feature_registry.is_enabled(FeatureId::Visualizer));

        let result =
            execute_ui_palette_command(&mut app, "root add /tmp/auric-palette --watched").unwrap();
        assert!(result.refresh_requested);
        assert_eq!(app.db.list_library_roots().unwrap().len(), 1);
        assert!(result.status_message.contains("Root saved"));

        let result =
            execute_ui_palette_command(&mut app, "playlist create Late Night Mix").unwrap();
        assert!(result.refresh_requested);
        let playlists = app.db.list_playlists().unwrap();
        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0].name, "Late Night Mix");
    }

    #[test]
    fn strip_n_words_returns_remaining_input() {
        assert_eq!(
            strip_n_words("playlist create Late Night Mix", 2),
            Some("Late Night Mix".to_string())
        );
        assert_eq!(strip_n_words("feature list", 2), None);
    }
}
