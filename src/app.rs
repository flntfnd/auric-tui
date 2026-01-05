use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use ratatui::layout::Rect;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::audio::{AudioPlayer, SpectrumAnalyzer};
use crate::config::{AppConfig, Database, RepeatMode, SortMode, Theme, ThemePreset};
use crate::events::{Action, MultiWatcher, WatchEvent};
use crate::library::{
    fetch_missing_artwork, ArtworkEvent, LoadedFolder, Playlist, ScanEvent, Scanner, Track,
};
use crate::ui::{AlbumArtRenderer, FileBrowser};

/// Cached layout areas for mouse hit testing
#[derive(Default, Clone, Copy)]
pub struct LayoutAreas {
    pub watched_section: Rect,
    pub library_section: Rect,
    pub playlists_section: Rect,
    pub track_list: Rect,
    pub now_playing: Rect,
    // Now playing sub-areas for click detection
    pub np_prev_btn: Rect,
    pub np_play_btn: Rect,
    pub np_next_btn: Rect,
    pub np_progress_bar: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    WatchedFolders,
    Library,
    Playlists,
    Tracks,
    NowPlaying,
}

impl Panel {
    pub fn next(self) -> Self {
        match self {
            Panel::WatchedFolders => Panel::Library,
            Panel::Library => Panel::Playlists,
            Panel::Playlists => Panel::Tracks,
            Panel::Tracks => Panel::NowPlaying,
            Panel::NowPlaying => Panel::WatchedFolders,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Panel::WatchedFolders => Panel::NowPlaying,
            Panel::Library => Panel::WatchedFolders,
            Panel::Playlists => Panel::Library,
            Panel::Tracks => Panel::Playlists,
            Panel::NowPlaying => Panel::Tracks,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    FileBrowser,
    AddWatchedFolder,
    Search,
    NewPlaylist,
    Help,
    Settings,
    Confirm(ConfirmAction),
}

/// Which setting is currently selected in the settings screen
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsSelection {
    #[default]
    Theme,
    SpectrumAnalyzer,
    AlbumArt,
}

impl SettingsSelection {
    pub fn next(self) -> Self {
        match self {
            SettingsSelection::Theme => SettingsSelection::SpectrumAnalyzer,
            SettingsSelection::SpectrumAnalyzer => SettingsSelection::AlbumArt,
            SettingsSelection::AlbumArt => SettingsSelection::Theme,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            SettingsSelection::Theme => SettingsSelection::AlbumArt,
            SettingsSelection::SpectrumAnalyzer => SettingsSelection::Theme,
            SettingsSelection::AlbumArt => SettingsSelection::SpectrumAnalyzer,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmAction {
    DeletePlaylist,
    #[allow(dead_code)]
    RemoveFromPlaylist,
    DeleteFolder,
}

pub struct App {
    // State
    pub running: bool,
    pub active_panel: Panel,
    pub input_mode: InputMode,
    pub input_buffer: String,

    // Library
    pub folders: Vec<LoadedFolder>,
    pub tracks: Vec<Track>,
    pub playlists: Vec<Playlist>,

    // Selection state
    pub folder_cursor: usize,                      // Current cursor position in folder list
    pub folders_selected: HashSet<PathBuf>,        // Selected folder paths (for filtering)
    pub watched_folder_cursor: usize,              // Cursor in watched folders
    pub watched_folders_selected: HashSet<PathBuf>, // Selected watched folder paths
    pub playlist_selected: usize,
    pub track_selected: usize,
    pub track_list_offset: usize, // Scroll offset for track list

    // Playback
    pub player: AudioPlayer,
    pub current_track_id: Option<Uuid>,
    pub queue: Vec<Uuid>,
    pub queue_position: usize,

    // Settings
    pub config: AppConfig,
    pub db: Database,

    // View state
    pub sorted_track_ids: Vec<Uuid>,
    pub filtered_track_ids: Vec<Uuid>,
    pub search_query: String,
    pub status_message: Option<String>,

    // Async communication
    pub scan_rx: Option<mpsc::Receiver<ScanEvent>>,

    // Album art renderer
    pub album_art_renderer: AlbumArtRenderer,

    // File browser
    pub file_browser: Option<FileBrowser>,

    // Multi-folder watcher for watched folders
    multi_watcher: Option<MultiWatcher>,
    watch_rx: Option<mpsc::Receiver<WatchEvent>>,

    // Album art fetching
    artwork_rx: Option<mpsc::Receiver<ArtworkEvent>>,
    artwork_fetch_in_progress: bool,

    // Spectrum analyzer for visualization
    spectrum_analyzer: SpectrumAnalyzer,

    // Layout areas for mouse hit testing
    pub layout_areas: LayoutAreas,

    // Settings screen
    pub settings_selection: SettingsSelection,
}

impl App {
    pub fn new() -> Result<Self> {
        let db = Database::new()?;
        let config = db.load_config()?;
        let folders = db.load_folders()?;
        let tracks = db.load_all_tracks()?;
        let playlists = db.load_playlists()?;
        let (current_track_id, queue) = db.load_session()?;

        let mut player = AudioPlayer::new()?;
        player.set_volume(config.volume);

        // Create spectrum analyzer using the player's shared sample buffer
        let spectrum_analyzer = SpectrumAnalyzer::new(player.spectrum().sample_buffer());

        let sorted_track_ids: Vec<Uuid> = tracks.iter().map(|t| t.id).collect();

        let mut app = Self {
            running: true,
            active_panel: Panel::Library,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),

            folders,
            tracks,
            playlists,

            folder_cursor: 0,
            folders_selected: HashSet::new(),
            watched_folder_cursor: 0,
            watched_folders_selected: HashSet::new(),
            playlist_selected: 0,
            track_selected: 0,
            track_list_offset: 0,

            player,
            current_track_id,
            queue,
            queue_position: 0,

            config,
            db,

            sorted_track_ids: sorted_track_ids.clone(),
            filtered_track_ids: sorted_track_ids,
            search_query: String::new(),
            status_message: None,

            scan_rx: None,

            album_art_renderer: AlbumArtRenderer::new(),

            file_browser: None,

            multi_watcher: None,
            watch_rx: None,

            artwork_rx: None,
            artwork_fetch_in_progress: false,

            spectrum_analyzer,

            layout_areas: LayoutAreas::default(),

            settings_selection: SettingsSelection::default(),
        };

        app.sort_tracks();

        // Start watching all folders marked as watched
        app.start_watched_folders();

        Ok(app)
    }

    /// Initialize the multi-watcher and start watching all folders marked as watched
    pub fn start_watched_folders(&mut self) {
        let (tx, rx) = mpsc::channel(100);

        match MultiWatcher::new(tx) {
            Ok(mut watcher) => {
                // Start watching all folders that are marked as watched
                let watched_count = self
                    .folders
                    .iter()
                    .filter(|f| f.is_watched)
                    .filter_map(|f| {
                        if f.path.exists() {
                            watcher.watch(f.path.clone()).ok()
                        } else {
                            None
                        }
                    })
                    .count();

                self.multi_watcher = Some(watcher);
                self.watch_rx = Some(rx);

                if watched_count > 0 {
                    self.status_message = Some(format!("Watching {} folders", watched_count));
                }
            }
            Err(e) => {
                self.status_message = Some(format!("Watch error: {}", e));
            }
        }
    }

    /// Process watch events for file changes in watched folders
    pub fn process_watch_events(&mut self) {
        // Collect events first to avoid borrow conflicts
        let events: Vec<WatchEvent> = if let Some(rx) = &mut self.watch_rx {
            let mut events = Vec::new();
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
            events
        } else {
            return;
        };

        // Now process each event
        for event in events {
            match event {
                WatchEvent::FileCreated(path) => {
                    self.handle_file_created(path);
                }
                WatchEvent::FileModified(path) => {
                    self.handle_file_modified(path);
                }
                WatchEvent::FileDeleted(path) => {
                    self.handle_file_deleted(path);
                }
                WatchEvent::FolderDeleted(path) => {
                    self.handle_watched_folder_deleted(path);
                }
                WatchEvent::Error(e) => {
                    self.status_message = Some(format!("Watch error: {}", e));
                }
            }
        }
    }

    /// Handle a new file being created in a watched folder
    fn handle_file_created(&mut self, path: PathBuf) {
        // Find which watched folder this belongs to
        let folder_path = self
            .folders
            .iter()
            .filter(|f| f.is_watched)
            .find(|f| path.starts_with(&f.path))
            .map(|f| f.path.clone());

        if let Some(folder_path) = folder_path {
            // Read metadata and add the track
            match Scanner::read_track_metadata(&path) {
                Ok(track) => {
                    let _ = self.db.add_track(&track, &folder_path);
                    self.sorted_track_ids.push(track.id);
                    self.filtered_track_ids.push(track.id);
                    self.tracks.push(track);

                    // Update folder track count
                    if let Some(folder) = self.folders.iter_mut().find(|f| f.path == folder_path) {
                        folder.track_count += 1;
                        let _ = self.db.update_folder_track_count(&folder.path, folder.track_count);
                    }

                    self.sort_tracks();
                    self.status_message = Some(format!("Added: {}", path.display()));
                }
                Err(e) => {
                    self.status_message = Some(format!("Error reading {}: {}", path.display(), e));
                }
            }
        }
    }

    /// Handle a file being modified in a watched folder
    fn handle_file_modified(&mut self, path: PathBuf) {
        // Find the existing track
        if let Some(track_idx) = self.tracks.iter().position(|t| t.path == path) {
            let track_id = self.tracks[track_idx].id;

            // Re-read metadata
            match Scanner::read_track_metadata(&path) {
                Ok(mut new_track) => {
                    // Keep the same ID
                    new_track.id = track_id;

                    // Find folder path for database update
                    let folder_path = self
                        .folders
                        .iter()
                        .filter(|f| f.is_watched)
                        .find(|f| path.starts_with(&f.path))
                        .map(|f| f.path.clone());

                    if let Some(fp) = folder_path {
                        let _ = self.db.add_track(&new_track, &fp);
                    }

                    self.tracks[track_idx] = new_track;
                    self.status_message = Some(format!("Updated: {}", path.display()));
                }
                Err(e) => {
                    self.status_message = Some(format!("Error updating {}: {}", path.display(), e));
                }
            }
        }
    }

    /// Handle a file being deleted from a watched folder
    fn handle_file_deleted(&mut self, path: PathBuf) {
        // Find and remove the track
        if let Some(track_idx) = self.tracks.iter().position(|t| t.path == path) {
            let track_id = self.tracks[track_idx].id;

            // Handle if this track is playing or in queue
            self.handle_track_removal(track_id);

            // Remove from database
            let _ = self.db.remove_track_by_path(&path);

            // Remove from memory
            self.tracks.remove(track_idx);
            self.sorted_track_ids.retain(|&id| id != track_id);
            self.filtered_track_ids.retain(|&id| id != track_id);

            // Update folder track count
            if let Some(folder) = self
                .folders
                .iter_mut()
                .filter(|f| f.is_watched)
                .find(|f| path.starts_with(&f.path))
            {
                folder.track_count = folder.track_count.saturating_sub(1);
                let _ = self.db.update_folder_track_count(&folder.path, folder.track_count);
            }

            // Adjust track selection if needed
            if self.track_selected >= self.filtered_track_ids.len() {
                self.track_selected = self.filtered_track_ids.len().saturating_sub(1);
            }

            self.status_message = Some(format!("Removed: {}", path.display()));
        }
    }

    /// Handle a watched folder being deleted
    fn handle_watched_folder_deleted(&mut self, path: PathBuf) {
        // Find the folder
        if let Some(folder_idx) = self.folders.iter().position(|f| f.path == path && f.is_watched)
        {
            // Get all track IDs from this folder
            let track_ids: Vec<Uuid> = self
                .tracks
                .iter()
                .filter(|t| t.path.starts_with(&path))
                .map(|t| t.id)
                .collect();

            // Handle each track's removal (queue, playback)
            for track_id in &track_ids {
                self.handle_track_removal(*track_id);
            }

            // Remove from database
            let _ = self.db.remove_folder(&path);

            // Remove tracks from memory
            self.tracks.retain(|t| !track_ids.contains(&t.id));
            self.sorted_track_ids.retain(|id| !track_ids.contains(id));
            self.filtered_track_ids.retain(|id| !track_ids.contains(id));

            // Stop watching
            if let Some(ref mut watcher) = self.multi_watcher {
                let _ = watcher.unwatch(&path);
            }

            // Remove folder from memory
            let folder_name = self.folders[folder_idx].name.clone();
            self.folders.remove(folder_idx);

            // Adjust selection
            if self.watched_folder_cursor > 0
                && self.watched_folder_cursor >= self.watched_folders_count()
            {
                self.watched_folder_cursor -= 1;
            }

            self.status_message = Some(format!("Watched folder removed: {}", folder_name));
        }
    }

    /// Handle removal of a track from queue/playback
    fn handle_track_removal(&mut self, track_id: Uuid) {
        // Check if currently playing
        if self.current_track_id == Some(track_id) {
            // Try to play next track
            if self.next_track().is_err() || self.queue.is_empty() {
                self.stop_playback();
            }
            self.status_message = Some("Playing track was deleted".to_string());
        }

        // Remove from queue
        if let Some(pos) = self.queue.iter().position(|&id| id == track_id) {
            self.queue.remove(pos);
            // Adjust queue position if needed
            if pos < self.queue_position {
                self.queue_position = self.queue_position.saturating_sub(1);
            }
        }
    }

    /// Get count of watched folders
    pub fn watched_folders_count(&self) -> usize {
        self.folders.iter().filter(|f| f.is_watched).count()
    }

    /// Get count of non-watched (regular) folders
    pub fn regular_folders_count(&self) -> usize {
        self.folders.iter().filter(|f| !f.is_watched).count()
    }

    pub fn handle_action(&mut self, action: Action) -> Result<()> {
        match self.input_mode {
            InputMode::Normal => self.handle_normal_action(action)?,
            InputMode::FileBrowser => self.handle_file_browser_action(action, false)?,
            InputMode::AddWatchedFolder => self.handle_file_browser_action(action, true)?,
            InputMode::Search => self.handle_search_action(action)?,
            InputMode::NewPlaylist => self.handle_new_playlist_action(action)?,
            InputMode::Help => self.handle_help_action(action)?,
            InputMode::Settings => self.handle_settings_action(action)?,
            InputMode::Confirm(confirm_action) => {
                self.handle_confirm_action(action, confirm_action)?
            }
        }
        Ok(())
    }

    fn handle_normal_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Quit => self.quit()?,
            Action::NextPanel => self.active_panel = self.active_panel.next(),
            Action::PrevPanel => self.active_panel = self.active_panel.prev(),

            Action::Up => self.move_selection_up(),
            Action::Down => self.move_selection_down(),
            Action::Enter => self.handle_enter()?,
            Action::Escape => {
                // Clear folder selection to show all tracks
                if !self.folders_selected.is_empty() || !self.watched_folders_selected.is_empty() {
                    self.clear_folder_selection();
                    self.status_message = Some("Showing all tracks".to_string());
                }
            }

            Action::PlayPause => self.toggle_playback(),
            Action::Stop => self.stop_playback(),
            Action::NextTrack => self.next_track()?,
            Action::PrevTrack => self.prev_track()?,
            Action::SeekForward => self.player.seek_forward(Duration::from_secs(10))?,
            Action::SeekBackward => self.player.seek_backward(Duration::from_secs(10))?,
            Action::VolumeUp => {
                self.player.volume_up();
                self.config.volume = self.player.volume();
            }
            Action::VolumeDown => {
                self.player.volume_down();
                self.config.volume = self.player.volume();
            }

            Action::ToggleShuffle => {
                self.config.shuffle = !self.config.shuffle;
                self.status_message = Some(format!(
                    "Shuffle: {}",
                    if self.config.shuffle { "ON" } else { "OFF" }
                ));
            }
            Action::ToggleRepeat => {
                self.config.repeat = self.config.repeat.next();
                self.status_message = Some(format!("Repeat: {:?}", self.config.repeat));
            }
            Action::CycleSortMode => {
                self.config.sort_mode = self.config.sort_mode.next();
                self.sort_tracks();
                self.status_message = Some(format!("Sort: {}", self.config.sort_mode.label()));
            }

            Action::LoadFolder => {
                self.file_browser = Some(FileBrowser::new());
                self.input_mode = InputMode::FileBrowser;
            }
            Action::SetWatchFolder => {
                // Now used to add a watched folder
                self.file_browser = Some(FileBrowser::new());
                self.input_mode = InputMode::AddWatchedFolder;
            }
            Action::NewPlaylist => {
                self.input_mode = InputMode::NewPlaylist;
                self.input_buffer.clear();
            }
            Action::AddToPlaylist => self.add_selected_to_playlist(),
            Action::DeletePlaylist => {
                if !self.playlists.is_empty() {
                    self.input_mode = InputMode::Confirm(ConfirmAction::DeletePlaylist);
                }
            }
            Action::RemoveFromPlaylist | Action::Delete => {
                // 'd' key - context-dependent delete
                match self.active_panel {
                    Panel::WatchedFolders => {
                        if self.watched_folders_count() > 0 {
                            self.remove_selected_watched_folder();
                        }
                    }
                    Panel::Library => {
                        if self.regular_folders_count() > 0 {
                            self.input_mode = InputMode::Confirm(ConfirmAction::DeleteFolder);
                        }
                    }
                    Panel::Playlists => {
                        // Could remove track from playlist in future
                    }
                    Panel::Tracks | Panel::NowPlaying => {
                        // Could remove from queue in future
                    }
                }
            }

            Action::Help => self.input_mode = InputMode::Help,
            Action::Search => {
                self.input_mode = InputMode::Search;
                self.input_buffer.clear();
            }
            Action::Settings => {
                self.input_mode = InputMode::Settings;
                self.settings_selection = SettingsSelection::default();
            }
            Action::FetchArtwork => {
                self.fetch_all_missing_artwork();
            }

            // Mouse actions
            Action::MouseClick { x, y } => {
                self.handle_mouse_click(x, y)?;
            }
            Action::MouseDrag { x, y } => {
                self.handle_mouse_drag(x, y)?;
            }
            Action::MouseScrollUp { x, y } => {
                self.handle_mouse_scroll(x, y, true);
            }
            Action::MouseScrollDown { x, y } => {
                self.handle_mouse_scroll(x, y, false);
            }

            _ => {}
        }
        Ok(())
    }

    fn handle_file_browser_action(&mut self, action: Action, set_watch: bool) -> Result<()> {
        if let Some(ref mut browser) = self.file_browser {
            match action {
                Action::Escape => {
                    self.file_browser = None;
                    self.input_mode = InputMode::Normal;
                }
                Action::Up => {
                    browser.move_up();
                }
                Action::Down => {
                    browser.move_down(20); // Approximate visible height
                }
                Action::Backspace | Action::Left => {
                    browser.go_up();
                }
                Action::Enter | Action::Right => {
                    // Navigate into selected directory
                    browser.enter();
                }
                Action::NextPanel | Action::Char('l') => {
                    // Tab or 'l' = load the selected/current folder
                    let path = browser.get_load_path();
                    self.file_browser = None;
                    self.input_mode = InputMode::Normal;
                    if set_watch {
                        self.add_watched_folder(path)?;
                    } else {
                        self.load_folder(path)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Add a folder as a watched folder (auto-syncs with file changes)
    fn add_watched_folder(&mut self, path: PathBuf) -> Result<()> {
        if !path.exists() || !path.is_dir() {
            self.status_message = Some("Invalid folder path".to_string());
            return Ok(());
        }

        // Check if folder already exists
        if let Some(existing) = self.folders.iter_mut().find(|f| f.path == path) {
            if existing.is_watched {
                self.status_message = Some("Folder is already watched".to_string());
                return Ok(());
            }
            // Convert existing folder to watched
            existing.is_watched = true;
            let _ = self.db.set_folder_watched(&path, true);

            // Start watching
            if let Some(ref mut watcher) = self.multi_watcher {
                if let Err(e) = watcher.watch(path.clone()) {
                    self.status_message = Some(format!("Watch error: {}", e));
                    return Ok(());
                }
            }

            self.status_message = Some(format!("Now watching: {}", path.display()));
            return Ok(());
        }

        // New folder - add as watched
        let folder = LoadedFolder::new_watched(path.clone());

        // Save folder to database
        if let Err(e) = self.db.add_folder(&folder) {
            self.status_message = Some(format!("Database error: {}", e));
            return Ok(());
        }

        // Start watching
        if let Some(ref mut watcher) = self.multi_watcher {
            if let Err(e) = watcher.watch(path.clone()) {
                self.status_message = Some(format!("Watch error: {}", e));
                return Ok(());
            }
        }

        self.folders.push(folder);
        self.status_message = Some(format!("Scanning watched folder: {}", path.display()));

        // Start async scan
        let (tx, rx) = mpsc::channel(100);
        self.scan_rx = Some(rx);

        let path_clone = path.clone();
        tokio::spawn(async move {
            let _ = Scanner::scan_folder(&path_clone, tx).await;
        });

        Ok(())
    }

    /// Remove a watched folder (stops watching but keeps folder data)
    fn remove_selected_watched_folder(&mut self) {
        // Get the nth watched folder
        let watched_folders: Vec<usize> = self
            .folders
            .iter()
            .enumerate()
            .filter(|(_, f)| f.is_watched)
            .map(|(i, _)| i)
            .collect();

        if self.watched_folder_cursor >= watched_folders.len() {
            return;
        }

        let folder_idx = watched_folders[self.watched_folder_cursor];
        let folder = &self.folders[folder_idx];
        let folder_path = folder.path.clone();
        let folder_name = folder.name.clone();

        // Stop watching
        if let Some(ref mut watcher) = self.multi_watcher {
            let _ = watcher.unwatch(&folder_path);
        }

        // Mark as not watched in database
        let _ = self.db.set_folder_watched(&folder_path, false);

        // Update in memory
        self.folders[folder_idx].is_watched = false;

        // Adjust selection
        if self.watched_folder_cursor > 0
            && self.watched_folder_cursor >= self.watched_folders_count()
        {
            self.watched_folder_cursor -= 1;
        }

        self.status_message = Some(format!("Stopped watching: {}", folder_name));
    }

    fn handle_search_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Escape | Action::Cancel => {
                self.input_mode = InputMode::Normal;
                self.search_query.clear();
                self.input_buffer.clear();
                self.filter_tracks();
            }
            Action::Enter | Action::Confirm => {
                self.search_query = self.input_buffer.clone();
                self.input_mode = InputMode::Normal;
                self.filter_tracks();
            }
            Action::Backspace => {
                self.input_buffer.pop();
                self.search_query = self.input_buffer.clone();
                self.filter_tracks();
            }
            Action::Char(c) => {
                self.input_buffer.push(c);
                self.search_query = self.input_buffer.clone();
                self.filter_tracks();
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_new_playlist_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Escape | Action::Cancel => {
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
            }
            Action::Enter | Action::Confirm => {
                if !self.input_buffer.is_empty() {
                    let playlist = Playlist::new(&self.input_buffer);
                    let _ = self.db.save_playlist(&playlist);
                    self.playlists.push(playlist);
                    self.status_message = Some(format!("Created playlist: {}", self.input_buffer));
                }
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
            }
            Action::Backspace => {
                self.input_buffer.pop();
            }
            Action::Char(c) => {
                self.input_buffer.push(c);
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_help_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Escape | Action::Cancel | Action::Help | Action::Quit => {
                self.input_mode = InputMode::Normal;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_settings_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Escape | Action::Cancel | Action::Settings => {
                self.input_mode = InputMode::Normal;
            }
            Action::Up => {
                self.settings_selection = self.settings_selection.prev();
            }
            Action::Down => {
                self.settings_selection = self.settings_selection.next();
            }
            Action::Enter | Action::Right => {
                // Cycle to next value for the selected setting
                match self.settings_selection {
                    SettingsSelection::Theme => {
                        self.config.theme = self.config.theme.next();
                        self.status_message = Some(format!("Theme: {}", self.config.theme.label()));
                    }
                    SettingsSelection::SpectrumAnalyzer => {
                        self.config.spectrum_enabled = !self.config.spectrum_enabled;
                        self.status_message = Some(format!(
                            "Spectrum Analyzer: {}",
                            if self.config.spectrum_enabled { "ON" } else { "OFF" }
                        ));
                    }
                    SettingsSelection::AlbumArt => {
                        self.config.show_album_art = !self.config.show_album_art;
                        self.status_message = Some(format!(
                            "Album Art: {}",
                            if self.config.show_album_art { "ON" } else { "OFF" }
                        ));
                    }
                }
            }
            Action::Left => {
                // Cycle to previous value for the selected setting
                match self.settings_selection {
                    SettingsSelection::Theme => {
                        // Cycle backwards through themes
                        self.config.theme = match self.config.theme {
                            ThemePreset::Default => ThemePreset::Gruvbox,
                            ThemePreset::Dracula => ThemePreset::Default,
                            ThemePreset::Gruvbox => ThemePreset::Dracula,
                        };
                        self.status_message = Some(format!("Theme: {}", self.config.theme.label()));
                    }
                    SettingsSelection::SpectrumAnalyzer => {
                        self.config.spectrum_enabled = !self.config.spectrum_enabled;
                        self.status_message = Some(format!(
                            "Spectrum Analyzer: {}",
                            if self.config.spectrum_enabled { "ON" } else { "OFF" }
                        ));
                    }
                    SettingsSelection::AlbumArt => {
                        self.config.show_album_art = !self.config.show_album_art;
                        self.status_message = Some(format!(
                            "Album Art: {}",
                            if self.config.show_album_art { "ON" } else { "OFF" }
                        ));
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_confirm_action(&mut self, action: Action, confirm: ConfirmAction) -> Result<()> {
        match action {
            Action::Confirm | Action::Char('y') | Action::Char('Y') => {
                match confirm {
                    ConfirmAction::DeletePlaylist => {
                        if self.playlist_selected < self.playlists.len() {
                            let playlist = &self.playlists[self.playlist_selected];
                            let name = playlist.name.clone();
                            let _ = self.db.delete_playlist(&playlist.id);
                            self.playlists.remove(self.playlist_selected);
                            if self.playlist_selected > 0 {
                                self.playlist_selected -= 1;
                            }
                            self.status_message = Some(format!("Deleted playlist: {}", name));
                        }
                    }
                    ConfirmAction::RemoveFromPlaylist => {
                        // Handle remove from playlist
                    }
                    ConfirmAction::DeleteFolder => {
                        self.remove_selected_folder();
                    }
                }
                self.input_mode = InputMode::Normal;
            }
            Action::Escape | Action::Cancel | Action::Char('n') | Action::Char('N') => {
                self.input_mode = InputMode::Normal;
            }
            _ => {}
        }
        Ok(())
    }

    fn move_selection_up(&mut self) {
        match self.active_panel {
            Panel::WatchedFolders => {
                if self.watched_folder_cursor > 0 {
                    self.watched_folder_cursor -= 1;
                }
            }
            Panel::Library => {
                if self.folder_cursor > 0 {
                    self.folder_cursor -= 1;
                }
            }
            Panel::Playlists => {
                if self.playlist_selected > 0 {
                    self.playlist_selected -= 1;
                }
            }
            Panel::Tracks | Panel::NowPlaying => {
                if self.track_selected > 0 {
                    self.track_selected -= 1;
                    // Scroll up if selection goes above visible area
                    if self.track_selected < self.track_list_offset {
                        self.track_list_offset = self.track_selected;
                    }
                }
            }
        }
    }

    fn move_selection_down(&mut self) {
        match self.active_panel {
            Panel::WatchedFolders => {
                if self.watched_folder_cursor + 1 < self.watched_folders_count() {
                    self.watched_folder_cursor += 1;
                }
            }
            Panel::Library => {
                if self.folder_cursor + 1 < self.regular_folders_count() {
                    self.folder_cursor += 1;
                }
            }
            Panel::Playlists => {
                if self.playlist_selected + 1 < self.playlists.len() {
                    self.playlist_selected += 1;
                }
            }
            Panel::Tracks | Panel::NowPlaying => {
                if self.track_selected + 1 < self.filtered_track_ids.len() {
                    self.track_selected += 1;
                    // Scroll down if selection goes below visible area
                    self.ensure_track_visible();
                }
            }
        }
    }

    fn handle_enter(&mut self) -> Result<()> {
        match self.active_panel {
            Panel::WatchedFolders => {
                // Toggle selection of watched folder (for multi-select)
                self.toggle_watched_folder_at_cursor();
            }
            Panel::Library => {
                // Toggle selection of folder (for multi-select)
                self.toggle_folder_at_cursor();
            }
            Panel::Playlists => {
                // Load playlist tracks
            }
            Panel::Tracks | Panel::NowPlaying => {
                self.play_selected_track()?;
            }
        }
        Ok(())
    }

    fn play_selected_track(&mut self) -> Result<()> {
        if let Some(track_id) = self.filtered_track_ids.get(self.track_selected).copied() {
            self.play_track(track_id)?;
            // Build queue from current view
            self.queue = self.filtered_track_ids.clone();
            self.queue_position = self.track_selected;
        }
        Ok(())
    }

    fn play_track(&mut self, track_id: Uuid) -> Result<()> {
        if let Some(track) = self.tracks.iter().find(|t| t.id == track_id) {
            self.player.play(&track.path, track.duration)?;
            self.current_track_id = Some(track_id);
            // Update album art renderer
            self.album_art_renderer.set_track(Some(track));
        }
        Ok(())
    }

    fn toggle_playback(&mut self) {
        self.player.toggle_pause();
    }

    fn stop_playback(&mut self) {
        self.player.stop();
        self.current_track_id = None;
        self.album_art_renderer.set_track(None);
    }

    fn next_track(&mut self) -> Result<()> {
        if self.queue.is_empty() {
            return Ok(());
        }

        if self.config.shuffle {
            // Random next
            use std::collections::hash_map::RandomState;
            use std::hash::{BuildHasher, Hasher};
            let idx = RandomState::new().build_hasher().finish() as usize % self.queue.len();
            self.queue_position = idx;
        } else {
            self.queue_position = (self.queue_position + 1) % self.queue.len();
        }

        if let Some(&track_id) = self.queue.get(self.queue_position) {
            self.play_track(track_id)?;
        }
        Ok(())
    }

    fn prev_track(&mut self) -> Result<()> {
        if self.queue.is_empty() {
            return Ok(());
        }

        if self.queue_position > 0 {
            self.queue_position -= 1;
        } else {
            self.queue_position = self.queue.len().saturating_sub(1);
        }

        if let Some(&track_id) = self.queue.get(self.queue_position) {
            self.play_track(track_id)?;
        }
        Ok(())
    }

    fn add_selected_to_playlist(&mut self) {
        if self.playlists.is_empty() || self.filtered_track_ids.is_empty() {
            return;
        }

        if let Some(&track_id) = self.filtered_track_ids.get(self.track_selected) {
            self.playlists[self.playlist_selected].add_track(track_id);
            let _ = self.db.save_playlist(&self.playlists[self.playlist_selected]);
            self.status_message = Some("Added to playlist".to_string());
        }
    }

    pub fn load_folder(&mut self, path: PathBuf) -> Result<()> {
        if !path.exists() || !path.is_dir() {
            self.status_message = Some("Invalid folder path".to_string());
            return Ok(());
        }

        // Check if folder already loaded
        if self.folders.iter().any(|f| f.path == path) {
            self.status_message = Some("Folder already loaded".to_string());
            return Ok(());
        }

        let folder = LoadedFolder::new(path.clone());

        // Save folder to database
        if let Err(e) = self.db.add_folder(&folder) {
            self.status_message = Some(format!("Database error: {}", e));
            return Ok(());
        }

        self.folders.push(folder);
        self.status_message = Some(format!("Scanning: {}", path.display()));

        // Start async scan
        let (tx, rx) = mpsc::channel(100);
        self.scan_rx = Some(rx);

        let path_clone = path.clone();
        tokio::spawn(async move {
            let _ = Scanner::scan_folder(&path_clone, tx).await;
        });

        Ok(())
    }

    fn remove_selected_folder(&mut self) {
        // Get the nth regular (non-watched) folder
        let regular_folders: Vec<usize> = self
            .folders
            .iter()
            .enumerate()
            .filter(|(_, f)| !f.is_watched)
            .map(|(i, _)| i)
            .collect();

        if self.folder_cursor >= regular_folders.len() {
            return;
        }

        let folder_idx = regular_folders[self.folder_cursor];
        let folder = &self.folders[folder_idx];
        let folder_path = folder.path.clone();
        let folder_name = folder.name.clone();

        // Remove from database (this also removes tracks via cascade)
        if let Err(e) = self.db.remove_folder(&folder_path) {
            self.status_message = Some(format!("Database error: {}", e));
            return;
        }

        // Remove all tracks from this folder in memory
        let tracks_to_remove: Vec<Uuid> = self
            .tracks
            .iter()
            .filter(|t| t.path.starts_with(&folder_path))
            .map(|t| t.id)
            .collect();

        // Stop playback if current track is from this folder
        if let Some(current_id) = self.current_track_id {
            if tracks_to_remove.contains(&current_id) {
                self.stop_playback();
            }
        }

        // Remove tracks from memory
        self.tracks.retain(|t| !tracks_to_remove.contains(&t.id));
        self.sorted_track_ids
            .retain(|id| !tracks_to_remove.contains(id));
        self.filtered_track_ids
            .retain(|id| !tracks_to_remove.contains(id));
        self.queue.retain(|id| !tracks_to_remove.contains(id));

        // Remove folder from memory
        self.folders.remove(folder_idx);

        // Adjust selection
        if self.folder_cursor > 0 && self.folder_cursor >= self.regular_folders_count() {
            self.folder_cursor -= 1;
        }

        // Adjust track selection if needed
        if self.track_selected >= self.filtered_track_ids.len() {
            self.track_selected = self.filtered_track_ids.len().saturating_sub(1);
        }

        self.status_message = Some(format!("Removed folder: {}", folder_name));
    }

    pub fn process_scan_events(&mut self) {
        // Process up to 50 events per tick to keep UI responsive
        const MAX_EVENTS_PER_TICK: usize = 50;

        let events: Vec<ScanEvent> = if let Some(rx) = &mut self.scan_rx {
            let mut events = Vec::new();
            while events.len() < MAX_EVENTS_PER_TICK {
                match rx.try_recv() {
                    Ok(event) => events.push(event),
                    Err(_) => break,
                }
            }
            events
        } else {
            return;
        };

        if events.is_empty() {
            return;
        }

        // Get the folder path for the current scan (last folder added)
        let folder_path = self.folders.last().map(|f| f.path.clone());

        let mut needs_sort = false;
        let mut tracks_added = 0;

        for event in events {
            match event {
                ScanEvent::TrackFound(track) => {
                    // Save track to database
                    if let Some(ref fp) = folder_path {
                        let _ = self.db.add_track(&track, fp);
                    }
                    self.sorted_track_ids.push(track.id);
                    self.filtered_track_ids.push(track.id);
                    self.tracks.push(track);
                    tracks_added += 1;
                }
                ScanEvent::ScanComplete { folder, count } => {
                    self.status_message = Some(format!("Loaded {} tracks from {}", count, folder));
                    if let Some(f) = self.folders.last_mut() {
                        f.track_count = count;
                        // Update folder track count in database
                        let _ = self.db.update_folder_track_count(&f.path, count);
                    }
                    needs_sort = true;
                }
                ScanEvent::Error { path, error } => {
                    self.status_message = Some(format!("Error: {} - {}", path, error));
                }
            }
        }

        // Show progress while scanning
        if tracks_added > 0 && !needs_sort {
            self.status_message = Some(format!("Scanning... {} tracks found", self.tracks.len()));
        }

        if needs_sort {
            self.sort_tracks();

            // Start fetching missing artwork after scan completes
            self.start_artwork_fetch();
        }
    }

    /// Find tracks that are missing album art and start fetching
    pub fn start_artwork_fetch(&mut self) {
        if self.artwork_fetch_in_progress {
            return;
        }

        // Find tracks missing album art (no album_art_data)
        let missing: Vec<(String, String, String)> = self
            .tracks
            .iter()
            .filter(|t| t.album_art_data.is_none())
            .map(|t| {
                (
                    t.path.to_string_lossy().to_string(),
                    t.artist.clone(),
                    t.album.clone(),
                )
            })
            .collect();

        if missing.is_empty() {
            return;
        }

        // Deduplicate by album (only fetch once per unique artist+album)
        let mut seen = std::collections::HashSet::new();
        let unique_albums: Vec<(String, String, String)> = missing
            .into_iter()
            .filter(|(_path, artist, album)| {
                let key = format!("{}|{}", artist.to_lowercase(), album.to_lowercase());
                if seen.contains(&key) {
                    false
                } else {
                    seen.insert(key);
                    true
                }
            })
            .collect();

        if unique_albums.is_empty() {
            return;
        }

        self.artwork_fetch_in_progress = true;
        self.status_message = Some(format!(
            "Fetching album art for {} albums...",
            unique_albums.len()
        ));

        let (tx, rx) = mpsc::channel(100);
        self.artwork_rx = Some(rx);

        tokio::spawn(async move {
            fetch_missing_artwork(unique_albums, tx).await;
        });
    }

    /// Process artwork fetch events
    pub fn process_artwork_events(&mut self) {
        let events: Vec<ArtworkEvent> = if let Some(rx) = &mut self.artwork_rx {
            let mut events = Vec::new();
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
            events
        } else {
            return;
        };

        for event in events {
            match event {
                ArtworkEvent::Found { artist, album, data } => {
                    // Update all tracks with this artist+album
                    let artist_lower = artist.to_lowercase();
                    let album_lower = album.to_lowercase();

                    for track in &mut self.tracks {
                        if track.artist.to_lowercase() == artist_lower
                            && track.album.to_lowercase() == album_lower
                        {
                            track.album_art_data = Some(data.clone());
                        }
                    }

                    self.status_message = Some(format!("Found art: {} - {}", artist, album));
                }
                ArtworkEvent::Written { path } => {
                    self.status_message = Some(format!("Wrote art to: {}", path));
                }
                ArtworkEvent::NotFound { artist, album } => {
                    // Silent - don't spam the user
                    let _ = (artist, album);
                }
                ArtworkEvent::Error { artist, album, error } => {
                    // Silently ignore artwork fetch errors
                    let _ = (artist, album, error);
                }
            }
        }

        // Check if fetch is complete (channel closed)
        if let Some(rx) = &mut self.artwork_rx {
            if rx.try_recv().is_err() {
                // Channel might be empty or closed - check if sender is dropped
                // We'll just mark as not in progress after processing
            }
        }
    }

    /// Manually trigger artwork fetch for all tracks missing art
    pub fn fetch_all_missing_artwork(&mut self) {
        self.artwork_fetch_in_progress = false; // Reset to allow new fetch
        self.start_artwork_fetch();
    }

    fn sort_tracks(&mut self) {
        use std::collections::HashMap;

        // Build a HashMap for O(1) lookups during sort
        let track_map: HashMap<Uuid, &Track> = self.tracks.iter().map(|t| (t.id, t)).collect();
        let sort_mode = self.config.sort_mode;

        self.sorted_track_ids.sort_by(|a, b| {
            let track_a = track_map.get(a);
            let track_b = track_map.get(b);

            match (track_a, track_b) {
                (Some(a), Some(b)) => match sort_mode {
                    SortMode::ArtistAlbum => a
                        .artist
                        .cmp(&b.artist)
                        .then(a.album.cmp(&b.album))
                        .then(a.track_number.cmp(&b.track_number)),
                    SortMode::Album => a.album.cmp(&b.album).then(a.track_number.cmp(&b.track_number)),
                    SortMode::Title => a.title.cmp(&b.title),
                    SortMode::DateAdded => b.added_at.cmp(&a.added_at),
                    SortMode::Duration => a.duration.cmp(&b.duration),
                    SortMode::Path => a.path.cmp(&b.path),
                },
                _ => std::cmp::Ordering::Equal,
            }
        });

        self.filter_tracks();
    }

    fn filter_tracks(&mut self) {
        use std::collections::HashMap;

        if self.search_query.is_empty() {
            self.filtered_track_ids = self.sorted_track_ids.clone();
        } else {
            // Build a HashMap for O(1) lookups during filter
            let track_map: HashMap<Uuid, &Track> = self.tracks.iter().map(|t| (t.id, t)).collect();
            let query = self.search_query.to_lowercase();

            self.filtered_track_ids = self
                .sorted_track_ids
                .iter()
                .filter(|id| {
                    track_map
                        .get(id)
                        .map(|t| {
                            t.title.to_lowercase().contains(&query)
                                || t.artist.to_lowercase().contains(&query)
                                || t.album.to_lowercase().contains(&query)
                        })
                        .unwrap_or(false)
                })
                .copied()
                .collect();
        }

        // Reset selection if out of bounds
        if self.track_selected >= self.filtered_track_ids.len() {
            self.track_selected = self.filtered_track_ids.len().saturating_sub(1);
        }
    }

    pub fn current_track(&self) -> Option<&Track> {
        self.current_track_id
            .and_then(|id| self.tracks.iter().find(|t| t.id == id))
    }

    /// Get the current theme
    pub fn theme(&self) -> Theme {
        self.config.theme.theme()
    }

    /// Ensure the selected track is visible by adjusting scroll offset
    fn ensure_track_visible(&mut self) {
        // Use the track list area to calculate visible rows
        let area = self.layout_areas.track_list;
        let inner_height = area.height.saturating_sub(2); // Remove borders
        let header_height = 1;
        let visible_rows = inner_height.saturating_sub(header_height) as usize;

        if visible_rows == 0 {
            return;
        }

        // Scroll down if selection is below visible area
        if self.track_selected >= self.track_list_offset + visible_rows {
            self.track_list_offset = self.track_selected - visible_rows + 1;
        }
        // Scroll up if selection is above visible area
        if self.track_selected < self.track_list_offset {
            self.track_list_offset = self.track_selected;
        }
    }

    /// Get spectrum bars for visualization
    pub fn get_spectrum_bars(&mut self) -> Vec<f32> {
        self.spectrum_analyzer.analyze()
    }

    pub fn check_track_ended(&mut self) -> Result<()> {
        if self.player.is_finished() {
            match self.config.repeat {
                RepeatMode::One => {
                    if let Some(track_id) = self.current_track_id {
                        self.play_track(track_id)?;
                    }
                }
                RepeatMode::All | RepeatMode::Off => {
                    if self.queue_position + 1 < self.queue.len() || self.config.repeat == RepeatMode::All {
                        self.next_track()?;
                    } else {
                        self.player.stop();
                        self.current_track_id = None;
                    }
                }
            }
        }
        Ok(())
    }

    fn quit(&mut self) -> Result<()> {
        // Save state before quitting
        self.db.save_config(&self.config)?;

        // Save playlists
        for playlist in &self.playlists {
            self.db.save_playlist(playlist)?;
        }

        // Save session
        self.db.save_session(self.current_track_id, &self.queue)?;

        self.running = false;
        Ok(())
    }

    /// Update cached layout areas for mouse hit testing
    pub fn update_layout_areas(&mut self, areas: LayoutAreas) {
        self.layout_areas = areas;
    }

    /// Handle a mouse click at the given coordinates
    fn handle_mouse_click(&mut self, x: u16, y: u16) -> Result<()> {
        let areas = self.layout_areas;

        // Check which panel was clicked and handle accordingly
        if Self::point_in_rect(x, y, areas.watched_section) {
            self.active_panel = Panel::WatchedFolders;
            self.handle_panel_click(x, y, areas.watched_section, self.watched_folders_count());
        } else if Self::point_in_rect(x, y, areas.library_section) {
            self.active_panel = Panel::Library;
            self.handle_panel_click(x, y, areas.library_section, self.regular_folders_count());
        } else if Self::point_in_rect(x, y, areas.playlists_section) {
            self.active_panel = Panel::Playlists;
            self.handle_panel_click(x, y, areas.playlists_section, self.playlists.len());
        } else if Self::point_in_rect(x, y, areas.track_list) {
            self.active_panel = Panel::Tracks;
            let old_selected = self.track_selected;
            self.handle_panel_click(x, y, areas.track_list, self.filtered_track_ids.len());
            // Double-click detection: if clicking same track, play it
            if old_selected == self.track_selected && !self.filtered_track_ids.is_empty() {
                self.play_selected_track()?;
            }
        } else if Self::point_in_rect(x, y, areas.now_playing) {
            self.active_panel = Panel::NowPlaying;
            // Handle clicks on playback controls
            self.handle_now_playing_click(x, y, areas.now_playing)?;
        }

        Ok(())
    }

    /// Handle click within a panel to select an item
    fn handle_panel_click(&mut self, _x: u16, y: u16, area: Rect, item_count: usize) {
        if item_count == 0 {
            return;
        }

        // Calculate which item was clicked (accounting for border)
        let inner_y = area.y + 1; // Skip top border
        let inner_height = area.height.saturating_sub(2); // Remove borders

        if y >= inner_y && y < inner_y + inner_height {
            let clicked_row = (y - inner_y) as usize;

            // Update the appropriate selection based on active panel
            match self.active_panel {
                Panel::WatchedFolders => {
                    let new_index = clicked_row.min(item_count.saturating_sub(1));
                    self.watched_folder_cursor = new_index;
                    // Select this folder and filter tracks
                    self.select_watched_folder_at_cursor();
                }
                Panel::Library => {
                    let new_index = clicked_row.min(item_count.saturating_sub(1));
                    self.folder_cursor = new_index;
                    // Select this folder and filter tracks
                    self.select_folder_at_cursor();
                }
                Panel::Playlists => {
                    let new_index = clicked_row.min(item_count.saturating_sub(1));
                    self.playlist_selected = new_index;
                }
                Panel::Tracks | Panel::NowPlaying => {
                    // Track list has a header row and uses stored scroll offset
                    let header_height = 1;
                    if clicked_row < header_height {
                        return; // Clicked on header
                    }
                    let visual_row = clicked_row - header_height;

                    // Use the stored scroll offset (don't recalculate - keeps view stable)
                    let actual_index = self.track_list_offset + visual_row;
                    let new_index = actual_index.min(item_count.saturating_sub(1));
                    self.track_selected = new_index;
                    // Don't call ensure_track_visible here - clicking shouldn't scroll
                }
            }
        }
    }

    /// Handle a click within the Now Playing panel
    fn handle_now_playing_click(&mut self, x: u16, y: u16, _area: Rect) -> Result<()> {
        let areas = self.layout_areas;

        // Check playback control buttons (using stored areas)
        if Self::point_in_rect(x, y, areas.np_prev_btn) {
            self.prev_track()?;
            return Ok(());
        }

        if Self::point_in_rect(x, y, areas.np_play_btn) {
            self.toggle_playback();
            return Ok(());
        }

        if Self::point_in_rect(x, y, areas.np_next_btn) {
            self.next_track()?;
            return Ok(());
        }

        // Check progress bar click for seeking
        if Self::point_in_rect(x, y, areas.np_progress_bar) && areas.np_progress_bar.width > 0 {
            // Calculate seek position based on click position within the progress bar
            let bar_start = areas.np_progress_bar.x;
            let bar_width = areas.np_progress_bar.width;
            let click_offset = x.saturating_sub(bar_start);
            let seek_ratio = click_offset as f64 / bar_width as f64;

            // Seek to the calculated position
            let duration = self.player.duration();
            let seek_pos = Duration::from_secs_f64(duration.as_secs_f64() * seek_ratio);
            self.player.seek(seek_pos)?;
            return Ok(());
        }

        Ok(())
    }

    /// Select the folder at the current cursor position (regular folders)
    fn select_folder_at_cursor(&mut self) {
        let regular_folders: Vec<PathBuf> = self
            .folders
            .iter()
            .filter(|f| !f.is_watched)
            .map(|f| f.path.clone())
            .collect();

        if let Some(path) = regular_folders.get(self.folder_cursor) {
            // Clear other folder selections and select just this one
            self.folders_selected.clear();
            self.watched_folders_selected.clear();
            self.folders_selected.insert(path.clone());
            self.apply_folder_filter();
        }
    }

    /// Select the watched folder at the current cursor position
    fn select_watched_folder_at_cursor(&mut self) {
        let watched_folders: Vec<PathBuf> = self
            .folders
            .iter()
            .filter(|f| f.is_watched)
            .map(|f| f.path.clone())
            .collect();

        if let Some(path) = watched_folders.get(self.watched_folder_cursor) {
            // Clear other folder selections and select just this one
            self.folders_selected.clear();
            self.watched_folders_selected.clear();
            self.watched_folders_selected.insert(path.clone());
            self.apply_folder_filter();
        }
    }

    /// Toggle selection of the folder at cursor (for multi-select with Ctrl+click)
    fn toggle_folder_at_cursor(&mut self) {
        let regular_folders: Vec<PathBuf> = self
            .folders
            .iter()
            .filter(|f| !f.is_watched)
            .map(|f| f.path.clone())
            .collect();

        if let Some(path) = regular_folders.get(self.folder_cursor) {
            if self.folders_selected.contains(path) {
                self.folders_selected.remove(path);
            } else {
                self.folders_selected.insert(path.clone());
            }
            self.apply_folder_filter();
        }
    }

    /// Toggle selection of the watched folder at cursor
    fn toggle_watched_folder_at_cursor(&mut self) {
        let watched_folders: Vec<PathBuf> = self
            .folders
            .iter()
            .filter(|f| f.is_watched)
            .map(|f| f.path.clone())
            .collect();

        if let Some(path) = watched_folders.get(self.watched_folder_cursor) {
            if self.watched_folders_selected.contains(path) {
                self.watched_folders_selected.remove(path);
            } else {
                self.watched_folders_selected.insert(path.clone());
            }
            self.apply_folder_filter();
        }
    }

    /// Clear all folder selections (show all tracks)
    fn clear_folder_selection(&mut self) {
        self.folders_selected.clear();
        self.watched_folders_selected.clear();
        self.apply_folder_filter();
    }

    /// Apply folder filter to track list
    fn apply_folder_filter(&mut self) {
        let all_selected: HashSet<&PathBuf> = self
            .folders_selected
            .iter()
            .chain(self.watched_folders_selected.iter())
            .collect();

        if all_selected.is_empty() {
            // No folders selected - show all tracks (apply search filter only)
            self.filter_tracks();
        } else {
            // Filter tracks to only show those from selected folders
            self.filtered_track_ids = self
                .sorted_track_ids
                .iter()
                .filter(|id| {
                    self.tracks
                        .iter()
                        .find(|t| &t.id == *id)
                        .map(|track| {
                            // Check if track's path starts with any selected folder
                            all_selected.iter().any(|folder_path| {
                                track.path.starts_with(folder_path)
                            })
                        })
                        .unwrap_or(false)
                })
                .copied()
                .collect();

            // Also apply search filter if there's a query
            if !self.search_query.is_empty() {
                let query = self.search_query.to_lowercase();
                self.filtered_track_ids.retain(|id| {
                    self.tracks
                        .iter()
                        .find(|t| t.id == *id)
                        .map(|track| {
                            track.title.to_lowercase().contains(&query)
                                || track.artist.to_lowercase().contains(&query)
                                || track.album.to_lowercase().contains(&query)
                        })
                        .unwrap_or(false)
                });
            }
        }

        // Reset track selection if out of bounds
        if self.track_selected >= self.filtered_track_ids.len() {
            self.track_selected = self.filtered_track_ids.len().saturating_sub(1);
        }
        self.track_list_offset = 0;
    }

    /// Handle mouse drag (for progress bar seeking)
    fn handle_mouse_drag(&mut self, x: u16, y: u16) -> Result<()> {
        let areas = self.layout_areas;

        // Only handle drag on progress bar for seeking
        if Self::point_in_rect(x, y, areas.np_progress_bar) && areas.np_progress_bar.width > 0 {
            let bar_start = areas.np_progress_bar.x;
            let bar_width = areas.np_progress_bar.width;
            let click_offset = x.saturating_sub(bar_start);
            let seek_ratio = (click_offset as f64 / bar_width as f64).clamp(0.0, 1.0);

            let duration = self.player.duration();
            let seek_pos = Duration::from_secs_f64(duration.as_secs_f64() * seek_ratio);
            self.player.seek(seek_pos)?;
        }

        Ok(())
    }

    /// Handle mouse scroll in a panel
    fn handle_mouse_scroll(&mut self, x: u16, y: u16, up: bool) {
        let areas = self.layout_areas;

        // Determine which panel the scroll is in
        let (panel, item_count) = if Self::point_in_rect(x, y, areas.watched_section) {
            (Panel::WatchedFolders, self.watched_folders_count())
        } else if Self::point_in_rect(x, y, areas.library_section) {
            (Panel::Library, self.regular_folders_count())
        } else if Self::point_in_rect(x, y, areas.playlists_section) {
            (Panel::Playlists, self.playlists.len())
        } else if Self::point_in_rect(x, y, areas.track_list) {
            (Panel::Tracks, self.filtered_track_ids.len())
        } else {
            return;
        };

        if item_count == 0 {
            return;
        }

        // Set active panel and scroll
        self.active_panel = panel;

        match panel {
            Panel::WatchedFolders => {
                if up && self.watched_folder_cursor > 0 {
                    self.watched_folder_cursor -= 1;
                } else if !up && self.watched_folder_cursor + 1 < item_count {
                    self.watched_folder_cursor += 1;
                }
            }
            Panel::Library => {
                if up && self.folder_cursor > 0 {
                    self.folder_cursor -= 1;
                } else if !up && self.folder_cursor + 1 < item_count {
                    self.folder_cursor += 1;
                }
            }
            Panel::Playlists => {
                if up && self.playlist_selected > 0 {
                    self.playlist_selected -= 1;
                } else if !up && self.playlist_selected + 1 < item_count {
                    self.playlist_selected += 1;
                }
            }
            Panel::Tracks | Panel::NowPlaying => {
                if up && self.track_selected > 0 {
                    self.track_selected -= 1;
                    // Scroll up if selection goes above visible area
                    if self.track_selected < self.track_list_offset {
                        self.track_list_offset = self.track_selected;
                    }
                } else if !up && self.track_selected + 1 < item_count {
                    self.track_selected += 1;
                    // Scroll down if needed
                    self.ensure_track_visible();
                }
            }
        }
    }

    /// Check if a point is within a rectangle
    fn point_in_rect(x: u16, y: u16, rect: Rect) -> bool {
        x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
    }
}
