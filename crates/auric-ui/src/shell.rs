use crate::theme::Palette;
use crate::UiError;
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use std::cmp::min;
use std::io::{self, Stdout};
use std::time::{Duration, Instant};
use tachyonfx::{fx, EffectTimer, Interpolation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconMode {
    NerdFont,
    Ascii,
}

impl IconMode {
    pub fn from_config(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "nerd-font" | "nerdfont" | "nf" => Self::NerdFont,
            _ => Self::Ascii,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    Sources,
    Browse,
    Tracks,
    Inspector,
}

impl FocusPane {
    pub fn next(self) -> Self {
        match self {
            Self::Sources => Self::Browse,
            Self::Browse => Self::Tracks,
            Self::Tracks => Self::Inspector,
            Self::Inspector => Self::Sources,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Sources => Self::Inspector,
            Self::Browse => Self::Sources,
            Self::Tracks => Self::Browse,
            Self::Inspector => Self::Tracks,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ShellListItem {
    pub id: String,
    pub label: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ShellTrackItem {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub path: String,
    pub duration_ms: Option<i64>,
    pub sample_rate: Option<i64>,
    pub channels: Option<i64>,
    pub bit_depth: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ShellSnapshot {
    pub app_title: String,
    pub theme_name: String,
    pub color_scheme: String,
    pub icon_mode: IconMode,
    pub icon_fallback: String,
    pub preferred_terminal_font: String,
    pub mouse_enabled: bool,
    pub artwork_filter: String,
    pub pixel_art_enabled: bool,
    pub pixel_art_cell_size: u16,
    pub roots: Vec<ShellListItem>,
    pub playlists: Vec<ShellListItem>,
    pub tracks: Vec<ShellTrackItem>,
    pub feature_summary: Vec<(String, bool)>,
    pub status_lines: Vec<String>,
    pub playback_status: String,
    pub now_playing_path: String,
    pub now_playing_title: String,
    pub now_playing_artist: String,
    pub now_playing_album: String,
    pub now_playing_artwork: Option<Vec<u8>>,
    pub now_playing_duration_ms: u64,
    pub now_playing_position_ms: u64,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat_mode: String,
    pub queue_length: usize,
    pub queue_position: usize,
    pub artists: Vec<String>,
    pub albums: Vec<(String, String)>,
    pub total_track_count: usize,
    pub setting_use_theme_bg: bool,
    pub setting_icon_pack: String,
    pub setting_pixel_art: bool,
    pub setting_pixel_art_cell_size: u16,
    pub setting_color_scheme: String,
    pub available_themes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ShellState {
    pub snapshot: ShellSnapshot,
    pub focus: FocusPane,
    pub selected_root: usize,
    pub selected_playlist: usize,
    pub selected_track: usize,
    pub track_filter_query: String,
    pub command_palette_input: String,
    pub status_message: Option<String>,
    pub show_help: bool,
    roots_scroll: usize,
    playlists_scroll: usize,
    tracks_scroll: usize,
    input_mode: InputMode,
    filtered_track_indices: Vec<usize>,
    file_browser: Option<crate::file_browser::FileBrowser>,
    terminal_caps: crate::terminal_caps::TerminalCaps,
    scanning_path: Option<String>,
    sort_column: SortColumn,
    last_click: Option<(Instant, u16, u16)>,
    sort_ascending: bool,
    pub playback_position_ms: u64,
    pub playback_duration_ms: u64,
    pub playback_status: String,
    pub seek_bar_area: Rect,
    pub artwork: crate::artwork::ArtworkState,
    pub browse: crate::browse::BrowseState,
    browse_filter_artist: Option<String>,
    browse_filter_album: Option<String>,
    pub spectrum_bands: Vec<f32>,
    pub track_change_time: Option<Instant>,
    last_track_path: String,
    settings_index: usize,
}

impl ShellState {
    pub fn new(snapshot: ShellSnapshot) -> Self {
        let mut state = Self {
            snapshot,
            focus: FocusPane::Tracks,
            selected_root: 0,
            selected_playlist: 0,
            selected_track: 0,
            track_filter_query: String::new(),
            command_palette_input: String::new(),
            status_message: Some(default_status_message().to_string()),
            show_help: false,
            roots_scroll: 0,
            playlists_scroll: 0,
            tracks_scroll: 0,
            input_mode: InputMode::Normal,
            filtered_track_indices: Vec::new(),
            file_browser: None,
            terminal_caps: crate::terminal_caps::TerminalCaps::detect(),
            scanning_path: None,
            sort_column: SortColumn::Title,
            sort_ascending: true,
            last_click: None,
            playback_position_ms: 0,
            playback_duration_ms: 0,
            playback_status: "stopped".to_string(),
            seek_bar_area: Rect::default(),
            artwork: crate::artwork::ArtworkState::new(),
            browse: crate::browse::BrowseState::new(),
            browse_filter_artist: None,
            browse_filter_album: None,
            spectrum_bands: vec![0.0; 32],
            track_change_time: None,
            last_track_path: String::new(),
            settings_index: 0,
        };
        state.rebuild_track_filter();
        // Auto-trigger welcome panel on empty library
        if state.snapshot.roots.is_empty() && state.snapshot.tracks.is_empty() {
            state.input_mode = InputMode::Welcome;
            state.file_browser = Some(crate::file_browser::FileBrowser::new(
                &home_dir().unwrap_or_else(|| std::path::PathBuf::from("/")),
            ));
        }
        state
    }

    pub fn replace_snapshot(&mut self, snapshot: ShellSnapshot) {
        let incoming_path = snapshot.now_playing_path.clone();
        let incoming_status = snapshot.playback_status.clone();
        self.snapshot = snapshot;
        self.selected_root = self
            .selected_root
            .min(self.snapshot.roots.len().saturating_sub(1));
        self.selected_playlist = self
            .selected_playlist
            .min(self.snapshot.playlists.len().saturating_sub(1));
        self.rebuild_track_filter();
        // Trigger fade when a new track starts playing.
        if incoming_status == "playing"
            && !incoming_path.is_empty()
            && incoming_path != self.last_track_path
        {
            self.track_change_time = Some(Instant::now());
            self.last_track_path = incoming_path;
        }
    }

    pub fn move_selection(&mut self, delta: isize) {
        match self.focus {
            FocusPane::Sources => {
                self.selected_root =
                    shift_index(self.selected_root, self.snapshot.roots.len(), delta);
            }
            FocusPane::Browse => {
                if self.browse.show_items && !self.browse.items.is_empty() {
                    self.browse.move_item_selection(delta);
                } else {
                    self.browse.move_mode_selection(delta);
                }
            }
            FocusPane::Tracks => {
                self.selected_track = shift_index(
                    self.selected_track,
                    self.filtered_track_indices.len(),
                    delta,
                );
            }
            FocusPane::Inspector => {
                self.selected_playlist =
                    shift_index(self.selected_playlist, self.snapshot.playlists.len(), delta);
            }
        }
    }

    pub fn move_to_start(&mut self) {
        match self.focus {
            FocusPane::Sources => self.selected_root = 0,
            FocusPane::Browse => {
                if self.browse.show_items && !self.browse.items.is_empty() {
                    self.browse.item_index = 0;
                } else {
                    self.browse.mode_index = 0;
                    self.browse.mode = crate::browse::BrowseMode::all()[0];
                }
            }
            FocusPane::Tracks => self.selected_track = 0,
            FocusPane::Inspector => self.selected_playlist = 0,
        }
    }

    pub fn move_to_end(&mut self) {
        match self.focus {
            FocusPane::Sources => self.selected_root = self.snapshot.roots.len().saturating_sub(1),
            FocusPane::Browse => {
                if self.browse.show_items && !self.browse.items.is_empty() {
                    self.browse.item_index = self.browse.items.len().saturating_sub(1);
                } else {
                    let modes = crate::browse::BrowseMode::all();
                    self.browse.mode_index = modes.len().saturating_sub(1);
                    self.browse.mode = modes[self.browse.mode_index];
                }
            }
            FocusPane::Tracks => {
                self.selected_track = self.filtered_track_indices.len().saturating_sub(1)
            }
            FocusPane::Inspector => {
                self.selected_playlist = self.snapshot.playlists.len().saturating_sub(1)
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> KeyAction {
        if key.kind != KeyEventKind::Press {
            return KeyAction::Continue;
        }

        match self.input_mode {
            InputMode::TrackInfo => {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('i') | KeyCode::Char('q') => {
                        self.input_mode = InputMode::Normal;
                    }
                    _ => {}
                }
                return KeyAction::Continue;
            }
            InputMode::TrackFilter => return self.handle_filter_key(key),
            InputMode::CommandPalette => return self.handle_command_palette_key(key),
            InputMode::AddMusic | InputMode::Welcome => return self.handle_add_music_key(key),
            InputMode::Settings => return self.handle_settings_key(key),
            InputMode::Normal => {}
        }

        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return KeyAction::Quit;
            }
            KeyCode::Char('q') => return KeyAction::Quit,
            KeyCode::Tab => self.focus = self.focus.next(),
            KeyCode::BackTab => self.focus = self.focus.prev(),
            KeyCode::Char('?') => self.show_help = !self.show_help,
            KeyCode::Esc if self.show_help => self.show_help = false,
            KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
            KeyCode::PageDown => self.move_selection(10),
            KeyCode::PageUp => self.move_selection(-10),
            KeyCode::Char('g') => self.move_to_start(),
            KeyCode::Char('G') => self.move_to_end(),
            KeyCode::Char('/') if self.focus == FocusPane::Tracks => self.enter_track_filter_mode(),
            KeyCode::Char(':') => self.enter_command_palette_mode(),
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.enter_command_palette_mode()
            }
            KeyCode::Char('r') => return KeyAction::RefreshRequested,
            KeyCode::Char('a') => {
                self.file_browser = Some(crate::file_browser::FileBrowser::new(
                    &home_dir().unwrap_or_else(|| std::path::PathBuf::from("/")),
                ));
                self.input_mode = InputMode::AddMusic;
            }
            KeyCode::Enter | KeyCode::Char('l') if self.focus == FocusPane::Browse => {
                self.handle_browse_enter();
            }
            KeyCode::Char('h') | KeyCode::Backspace if self.focus == FocusPane::Browse => {
                self.handle_browse_back();
            }
            KeyCode::Enter if self.focus == FocusPane::Tracks => {
                return KeyAction::Playback(PlaybackAction::PlayTrack {
                    track_index: self.selected_track,
                });
            }
            KeyCode::Char(' ') => {
                return KeyAction::Playback(PlaybackAction::TogglePause);
            }
            KeyCode::Char('n') => {
                return KeyAction::Playback(PlaybackAction::Next);
            }
            KeyCode::Char('N') => {
                return KeyAction::Playback(PlaybackAction::Previous);
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                return KeyAction::Playback(PlaybackAction::VolumeUp);
            }
            KeyCode::Char('-') => {
                return KeyAction::Playback(PlaybackAction::VolumeDown);
            }
            KeyCode::Char('s') => {
                return KeyAction::Playback(PlaybackAction::ToggleShuffle);
            }
            KeyCode::Char('o') => {
                self.cycle_sort();
                self.status_message = Some(format!(
                    "Sort: {} {}",
                    self.sort_column.label(),
                    if self.sort_ascending { "▲" } else { "▼" }
                ));
            }
            KeyCode::Char('i') if self.focus == FocusPane::Tracks => {
                if self.selected_track_item().is_some() {
                    self.input_mode = InputMode::TrackInfo;
                }
            }
            KeyCode::Char(',') => {
                self.settings_index = 0;
                self.input_mode = InputMode::Settings;
            }
            _ => {}
        }
        KeyAction::Continue
    }

    fn handle_mouse(&mut self, mouse: MouseEvent, areas: &RenderAreas) -> KeyAction {
        match mouse.kind {
            MouseEventKind::ScrollDown => {
                self.set_focus_from_point(mouse.column, mouse.row, areas);
                self.move_selection(3);
            }
            MouseEventKind::ScrollUp => {
                self.set_focus_from_point(mouse.column, mouse.row, areas);
                self.move_selection(-3);
            }
            MouseEventKind::Down(_) => {
                let x = mouse.column;
                let y = mouse.row;
                // Check if clicking on the seek bar
                if self.seek_bar_area != Rect::default() && self.seek_bar_area.contains((x, y).into()) {
                    let elapsed_width = 5u16; // "MM:SS" is 5 chars
                    let remaining_width = 5u16;
                    if let Some(progress) = crate::seekbar::click_to_progress(
                        x, self.seek_bar_area, elapsed_width, remaining_width,
                    ) {
                        let position_ms = (progress as f64 * self.playback_duration_ms as f64) as u64;
                        return KeyAction::Playback(PlaybackAction::Seek { position_ms });
                    }
                }
                // Check if clicking on track list header for sorting
                if areas.track_header.contains((x, y).into()) {
                    let co = &areas.track_col_offsets;
                    let col = if x >= co.quality_start {
                        Some(SortColumn::Quality)
                    } else if x >= co.artist_start {
                        Some(SortColumn::Artist)
                    } else if x >= co.time_start {
                        Some(SortColumn::Time)
                    } else if x >= co.title_start {
                        Some(SortColumn::Title)
                    } else {
                        None
                    };
                    if let Some(col) = col {
                        self.set_sort_column(col);
                        self.status_message = Some(format!(
                            "Sort: {} {}",
                            self.sort_column.label(),
                            if self.sort_ascending { "▲" } else { "▼" }
                        ));
                    }
                } else {
                    // Double-click detection
                    let is_double = self
                        .last_click
                        .map(|(t, lx, ly)| {
                            t.elapsed() < Duration::from_millis(400) && lx == x && ly == y
                        })
                        .unwrap_or(false);

                    self.set_focus_from_point(x, y, areas);
                    self.select_from_mouse_click(x, y, areas);

                    if is_double && self.focus == FocusPane::Tracks {
                        self.last_click = None;
                        return KeyAction::Playback(PlaybackAction::PlayTrack {
                            track_index: self.selected_track,
                        });
                    }
                    self.last_click = Some((Instant::now(), x, y));
                }
            }
            _ => {}
        }
        KeyAction::Continue
    }

    fn handle_settings_key(&mut self, key: KeyEvent) -> KeyAction {
        let num_settings = 6;
        match key.code {
            KeyCode::Esc | KeyCode::Char(',') | KeyCode::Char('q') => {
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.settings_index = (self.settings_index + 1).min(num_settings - 1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.settings_index = self.settings_index.saturating_sub(1);
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                let command = match self.settings_index {
                    0 => "__setting_cycle theme".to_string(),
                    1 => "__setting_toggle use_theme_background".to_string(),
                    2 => "__setting_cycle icon_pack".to_string(),
                    3 => "__setting_toggle pixel_art_artwork".to_string(),
                    4 => "__setting_cycle pixel_art_cell_size".to_string(),
                    5 => "__setting_cycle color_scheme".to_string(),
                    _ => return KeyAction::Continue,
                };
                return KeyAction::CommandSubmitted(command);
            }
            _ => {}
        }
        KeyAction::Continue
    }

    fn handle_filter_key(&mut self, key: KeyEvent) -> KeyAction {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.input_mode = InputMode::Normal;
                self.status_message = Some(self.filter_status_line(false));
            }
            KeyCode::Backspace => {
                self.track_filter_query.pop();
                self.rebuild_track_filter();
                self.status_message = Some(self.filter_status_line(true));
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.track_filter_query.clear();
                self.rebuild_track_filter();
                self.status_message = Some(self.filter_status_line(true));
            }
            KeyCode::Down => self.move_selection(1),
            KeyCode::Up => self.move_selection(-1),
            KeyCode::PageDown => self.move_selection(10),
            KeyCode::PageUp => self.move_selection(-10),
            KeyCode::Home => self.move_to_start(),
            KeyCode::End => self.move_to_end(),
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT)
                    && !c.is_control() =>
            {
                self.track_filter_query.push(c);
                self.rebuild_track_filter();
                self.status_message = Some(self.filter_status_line(true));
            }
            _ => {}
        }
        KeyAction::Continue
    }

    fn enter_track_filter_mode(&mut self) {
        self.input_mode = InputMode::TrackFilter;
        self.status_message = Some(self.filter_status_line(true));
    }

    fn handle_command_palette_key(&mut self, key: KeyEvent) -> KeyAction {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.command_palette_input.clear();
                self.status_message = Some("Command palette canceled".to_string());
            }
            KeyCode::Enter => {
                self.input_mode = InputMode::Normal;
                let command = self.command_palette_input.trim().to_string();
                self.command_palette_input.clear();
                if command.is_empty() {
                    self.status_message = Some("Command palette canceled".to_string());
                } else {
                    return KeyAction::CommandSubmitted(command);
                }
            }
            KeyCode::Backspace => {
                self.command_palette_input.pop();
                self.status_message = Some(self.command_palette_status_line());
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.command_palette_input.clear();
                self.status_message = Some(self.command_palette_status_line());
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT)
                    && !c.is_control() =>
            {
                self.command_palette_input.push(c);
                self.status_message = Some(self.command_palette_status_line());
            }
            _ => {}
        }
        KeyAction::Continue
    }

    fn enter_command_palette_mode(&mut self) {
        self.input_mode = InputMode::CommandPalette;
        self.command_palette_input.clear();
        self.status_message = Some(self.command_palette_status_line());
    }

    fn handle_add_music_key(&mut self, key: KeyEvent) -> KeyAction {
        let browser = match self.file_browser.as_mut() {
            Some(b) => b,
            None => {
                self.input_mode = InputMode::Normal;
                return KeyAction::Continue;
            }
        };

        if browser.input_focused {
            match key.code {
                KeyCode::Esc => {
                    browser.input_focused = false;
                }
                KeyCode::Tab => {
                    browser.input_focused = false;
                }
                KeyCode::Enter => {
                    browser.apply_path_input();
                    browser.input_focused = false;
                }
                KeyCode::Backspace => {
                    browser.path_input.pop();
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    browser.path_input.clear();
                }
                KeyCode::Char(c) => {
                    browser.path_input.push(c);
                }
                _ => {}
            }
            return KeyAction::Continue;
        }

        match key.code {
            KeyCode::Esc => {
                self.file_browser = None;
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Tab => {
                browser.input_focused = true;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                browser.move_selection(1);
                browser.sync_path_input_to_selected();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                browser.move_selection(-1);
                browser.sync_path_input_to_selected();
            }
            KeyCode::Enter => {
                browser.enter_selected();
            }
            KeyCode::Backspace | KeyCode::Char('h') => {
                browser.go_up();
            }
            KeyCode::Char(' ') => {
                let path = browser.selected_path().to_string_lossy().into_owned();
                self.file_browser = None;
                self.input_mode = InputMode::Normal;
                return KeyAction::CommandSubmitted(format!("__add_root {path}"));
            }
            _ => {}
        }
        KeyAction::Continue
    }

    fn rebuild_track_filter(&mut self) {
        self.filtered_track_indices.clear();
        if self.track_filter_query.is_empty() {
            self.filtered_track_indices
                .extend(0..self.snapshot.tracks.len());
        } else {
            let query = self.track_filter_query.to_lowercase();
            self.filtered_track_indices.extend(
                self.snapshot
                    .tracks
                    .iter()
                    .enumerate()
                    .filter(|(_, track)| track_matches_query(track, &query))
                    .map(|(idx, _)| idx),
            );
        }
        if let Some(ref artist) = self.browse_filter_artist {
            self.filtered_track_indices.retain(|&idx| {
                self.snapshot.tracks[idx]
                    .artist
                    .eq_ignore_ascii_case(artist)
            });
        }
        if let Some(ref album) = self.browse_filter_album {
            self.filtered_track_indices.retain(|&idx| {
                self.snapshot.tracks[idx]
                    .album
                    .eq_ignore_ascii_case(album)
            });
        }
        self.apply_sort();
        self.selected_track = self
            .selected_track
            .min(self.filtered_track_indices.len().saturating_sub(1));
    }

    fn apply_sort(&mut self) {
        let tracks = &self.snapshot.tracks;
        let col = self.sort_column;
        let asc = self.sort_ascending;
        self.filtered_track_indices.sort_by(|&a, &b| {
            let cmp = match col {
                SortColumn::Title => tracks[a].title.to_ascii_lowercase().cmp(&tracks[b].title.to_ascii_lowercase()),
                SortColumn::Artist => tracks[a].artist.to_ascii_lowercase().cmp(&tracks[b].artist.to_ascii_lowercase()),
                SortColumn::Time => tracks[a].duration_ms.cmp(&tracks[b].duration_ms),
                SortColumn::Quality => tracks[a].sample_rate.cmp(&tracks[b].sample_rate),
            };
            if asc { cmp } else { cmp.reverse() }
        });
    }

    fn set_sort_column(&mut self, col: SortColumn) {
        if self.sort_column == col {
            self.sort_ascending = !self.sort_ascending;
        } else {
            self.sort_column = col;
            self.sort_ascending = true;
        }
        self.apply_sort();
        self.selected_track = 0;
    }

    fn cycle_sort(&mut self) {
        if self.sort_ascending {
            self.sort_ascending = false;
        } else {
            self.sort_column = self.sort_column.next();
            self.sort_ascending = true;
        }
        self.apply_sort();
        self.selected_track = 0;
    }

    fn handle_browse_enter(&mut self) {
        if self.browse.show_items && !self.browse.items.is_empty() {
            self.browse.update_selected_item();
            match self.browse.mode {
                crate::browse::BrowseMode::Artists => {
                    self.browse_filter_artist = self.browse.selected_item.clone();
                    self.browse_filter_album = None;
                }
                crate::browse::BrowseMode::Albums => {
                    self.browse_filter_album = self.browse.selected_item.clone();
                    self.browse_filter_artist = None;
                }
                crate::browse::BrowseMode::Songs => {}
            }
            self.rebuild_track_filter();
        } else {
            self.apply_browse_mode();
        }
    }

    fn handle_browse_back(&mut self) {
        if self.browse.show_items {
            self.browse.show_items = false;
            self.browse.selected_item = None;
            self.browse_filter_artist = None;
            self.browse_filter_album = None;
            self.rebuild_track_filter();
        }
    }

    fn apply_browse_mode(&mut self) {
        let mode = crate::browse::BrowseMode::all()[self.browse.mode_index];
        self.browse.set_mode(mode);
        match mode {
            crate::browse::BrowseMode::Songs => {
                self.browse.show_items = false;
                self.browse.items.clear();
                self.browse_filter_artist = None;
                self.browse_filter_album = None;
            }
            crate::browse::BrowseMode::Artists => {
                self.browse.show_items = true;
                self.browse.items = self.snapshot.artists.clone();
            }
            crate::browse::BrowseMode::Albums => {
                self.browse.show_items = true;
                self.browse.items = self
                    .snapshot
                    .albums
                    .iter()
                    .map(|(a, _)| a.clone())
                    .collect();
            }
        }
        self.rebuild_track_filter();
    }

    fn filter_status_line(&self, editing: bool) -> String {
        let mode = if editing { "editing" } else { "applied" };
        format!(
            "Track filter ({mode}): \"{}\" [{}/{}]  Enter/Esc close  Ctrl-U clear",
            self.track_filter_query,
            self.filtered_track_indices.len(),
            self.snapshot.tracks.len()
        )
    }

    fn command_palette_status_line(&self) -> String {
        format!(
            "Command palette: :{}  (Enter run, Esc cancel, Ctrl-U clear)",
            self.command_palette_input
        )
    }

    fn selected_track_item(&self) -> Option<&ShellTrackItem> {
        let track_index = *self.filtered_track_indices.get(self.selected_track)?;
        self.snapshot.tracks.get(track_index)
    }

    fn filtered_track_count(&self) -> usize {
        self.filtered_track_indices.len()
    }

    fn filtered_track_iter(&self) -> impl Iterator<Item = &ShellTrackItem> {
        self.filtered_track_indices
            .iter()
            .filter_map(|idx| self.snapshot.tracks.get(*idx))
    }

    fn sync_scroll_offsets(&mut self, areas: &RenderAreas) {
        self.roots_scroll = normalize_scroll(
            self.roots_scroll,
            self.selected_root,
            self.snapshot.roots.len(),
            areas.roots.visible_items,
        );
        self.playlists_scroll = normalize_scroll(
            self.playlists_scroll,
            self.selected_playlist,
            self.snapshot.playlists.len(),
            areas.playlists.visible_items,
        );
        self.tracks_scroll = normalize_scroll(
            self.tracks_scroll,
            self.selected_track,
            self.filtered_track_indices.len(),
            areas.tracks.visible_items,
        );
        if self.browse.show_items {
            let browse_inner = padded_inner(areas.browse);
            let modes_height = crate::browse::BrowseMode::all().len() as u16 + 1;
            let items_visible = browse_inner.height.saturating_sub(modes_height) as usize;
            self.browse.item_scroll = normalize_scroll(
                self.browse.item_scroll,
                self.browse.item_index,
                self.browse.items.len(),
                items_visible,
            );
        }
    }

    fn set_focus_from_point(&mut self, x: u16, y: u16, areas: &RenderAreas) {
        let point = (x, y).into();
        if areas.roots.outer.contains(point) {
            self.focus = FocusPane::Sources;
        } else if areas.browse.contains(point) {
            self.focus = FocusPane::Browse;
        } else if areas.playlists.outer.contains(point) {
            self.focus = FocusPane::Inspector;
        } else if areas.tracks.outer.contains(point) {
            self.focus = FocusPane::Tracks;
        }
    }

    fn select_from_mouse_click(&mut self, x: u16, y: u16, areas: &RenderAreas) {
        if let Some(index) =
            areas
                .roots
                .mouse_item_index(x, y, self.roots_scroll, self.snapshot.roots.len())
        {
            self.selected_root = index;
            return;
        }
        if let Some(index) = areas.playlists.mouse_item_index(
            x,
            y,
            self.playlists_scroll,
            self.snapshot.playlists.len(),
        ) {
            self.selected_playlist = index;
            return;
        }
        if let Some(index) = areas.tracks.mouse_item_index(
            x,
            y,
            self.tracks_scroll,
            self.filtered_track_indices.len(),
        ) {
            self.selected_track = index;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Title,
    Time,
    Artist,
    Quality,
}

impl SortColumn {
    fn next(self) -> Self {
        match self {
            Self::Title => Self::Artist,
            Self::Artist => Self::Time,
            Self::Time => Self::Quality,
            Self::Quality => Self::Title,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Title => "Title",
            Self::Artist => "Artist",
            Self::Time => "Time",
            Self::Quality => "Quality",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Normal,
    TrackFilter,
    CommandPalette,
    AddMusic,
    Welcome,
    TrackInfo,
    Settings,
}

#[derive(Debug, Clone, PartialEq)]
pub enum KeyAction {
    Continue,
    Quit,
    RefreshRequested,
    CommandSubmitted(String),
    Playback(PlaybackAction),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackAction {
    PlayTrack { track_index: usize },
    TogglePause,
    Stop,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    ToggleShuffle,
    Seek { position_ms: u64 },
}

#[derive(Debug, Clone, Copy)]
pub struct RunOptions {
    pub tick_rate: Duration,
    pub mouse: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            tick_rate: Duration::from_millis(100),
            mouse: true,
        }
    }
}

type RefreshSnapshotFn<'a> = dyn FnMut() -> Result<ShellSnapshot, UiError> + 'a;
type CommandPaletteFn<'a> = dyn FnMut(&str) -> Result<PaletteCommandResult, UiError> + 'a;
type BackgroundScanFn<'a> =
    dyn FnMut(String) -> std::sync::mpsc::Receiver<ScanProgress> + 'a;

/// Progress messages sent from a background scan thread.
#[derive(Debug, Clone)]
pub enum ScanProgress {
    /// Periodic progress update: discovered N files so far.
    Progress { discovered: usize, path: String },
    /// Scan completed successfully.
    Done { message: String },
    /// Scan failed.
    Error { message: String },
}

#[derive(Debug, Clone)]
pub struct PlayerEventUpdate {
    pub position_ms: u64,
    pub duration_ms: u64,
    pub status: String,
    pub track_finished: bool,
    pub spectrum_bands: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteCommandResult {
    pub status_message: String,
    pub refresh_requested: bool,
    /// If set, the event loop should spawn a background scan for this path.
    pub background_scan_path: Option<String>,
}

impl PaletteCommandResult {
    pub fn new(status_message: impl Into<String>, refresh_requested: bool) -> Self {
        Self {
            status_message: status_message.into(),
            refresh_requested,
            background_scan_path: None,
        }
    }

    pub fn with_background_scan(
        status_message: impl Into<String>,
        scan_path: String,
    ) -> Self {
        Self {
            status_message: status_message.into(),
            refresh_requested: false,
            background_scan_path: Some(scan_path),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct PaneArea {
    outer: Rect,
    inner: Rect,
    visible_items: usize,
    item_height: u16,
}

impl PaneArea {
    fn bordered(area: Rect, item_height: u16) -> Self {
        Self::from_list_area(area, inner_rect(area), item_height)
    }

    fn from_list_area(outer: Rect, list_area: Rect, item_height: u16) -> Self {
        let inner = list_area;
        let inner_height = inner.height as usize;
        let item_height_usize = usize::from(item_height.max(1));
        let visible_items = if inner_height == 0 {
            0
        } else {
            inner_height.div_ceil(item_height_usize)
        };
        Self {
            outer,
            inner,
            visible_items,
            item_height: item_height.max(1),
        }
    }

    fn mouse_item_index(&self, x: u16, y: u16, scroll: usize, len: usize) -> Option<usize> {
        if len == 0 || !self.inner.contains((x, y).into()) {
            return None;
        }
        let local_row = usize::from(y.saturating_sub(self.inner.y));
        let item_height = usize::from(self.item_height.max(1));
        let index = scroll.saturating_add(local_row / item_height);
        (index < len).then_some(index)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct RenderAreas {
    roots: PaneArea,
    browse: Rect,
    playlists: PaneArea,
    tracks: PaneArea,
    track_header: Rect,
    track_col_offsets: TrackColumnOffsets,
}

#[derive(Debug, Clone, Copy, Default)]
struct TrackColumnOffsets {
    title_start: u16,
    time_start: u16,
    artist_start: u16,
    quality_start: u16,
}

pub fn run_interactive(
    state: &mut ShellState,
    palette: &Palette,
    options: RunOptions,
) -> Result<(), UiError> {
    run_interactive_with_optional_handlers(state, palette, options, None, None, None)
}

pub fn run_interactive_with_refresh<F>(
    state: &mut ShellState,
    palette: &Palette,
    options: RunOptions,
    mut refresh: F,
) -> Result<(), UiError>
where
    F: FnMut() -> Result<ShellSnapshot, UiError>,
{
    run_interactive_with_optional_handlers(state, palette, options, Some(&mut refresh), None, None)
}

pub fn run_interactive_with_handlers<FRefresh, FCommand>(
    state: &mut ShellState,
    palette: &Palette,
    options: RunOptions,
    mut refresh: FRefresh,
    mut command_handler: FCommand,
) -> Result<(), UiError>
where
    FRefresh: FnMut() -> Result<ShellSnapshot, UiError>,
    FCommand: FnMut(&str) -> Result<PaletteCommandResult, UiError>,
{
    run_interactive_with_optional_handlers(
        state,
        palette,
        options,
        Some(&mut refresh),
        Some(&mut command_handler),
        None,
    )
}

pub fn run_interactive_with_scan<FRefresh, FCommand, FScan>(
    state: &mut ShellState,
    palette: &Palette,
    options: RunOptions,
    mut refresh: FRefresh,
    mut command_handler: FCommand,
    mut scan_handler: FScan,
) -> Result<(), UiError>
where
    FRefresh: FnMut() -> Result<ShellSnapshot, UiError>,
    FCommand: FnMut(&str) -> Result<PaletteCommandResult, UiError>,
    FScan: FnMut(String) -> std::sync::mpsc::Receiver<ScanProgress>,
{
    run_interactive_with_optional_handlers(
        state,
        palette,
        options,
        Some(&mut refresh),
        Some(&mut command_handler),
        Some(&mut scan_handler),
    )
}

type PlaybackActionFn<'a> = dyn FnMut(PlaybackAction) -> Result<PaletteCommandResult, UiError> + 'a;
type PlayerPollFn<'a> = dyn FnMut() -> Vec<PlayerEventUpdate> + 'a;

fn run_interactive_with_optional_handlers(
    state: &mut ShellState,
    palette: &Palette,
    options: RunOptions,
    refresh: Option<&mut RefreshSnapshotFn<'_>>,
    command_handler: Option<&mut CommandPaletteFn<'_>>,
    scan_handler: Option<&mut BackgroundScanFn<'_>>,
) -> Result<(), UiError> {
    run_interactive_full_inner(
        state,
        palette,
        options,
        refresh,
        command_handler,
        scan_handler,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_interactive_full<FRefresh, FCommand, FScan, FPlayback, FPlayerPoll>(
    state: &mut ShellState,
    palette: &Palette,
    options: RunOptions,
    mut refresh: FRefresh,
    mut command_handler: FCommand,
    mut scan_handler: FScan,
    mut playback_handler: FPlayback,
    mut player_poll: FPlayerPoll,
) -> Result<(), UiError>
where
    FRefresh: FnMut() -> Result<ShellSnapshot, UiError>,
    FCommand: FnMut(&str) -> Result<PaletteCommandResult, UiError>,
    FScan: FnMut(String) -> std::sync::mpsc::Receiver<ScanProgress>,
    FPlayback: FnMut(PlaybackAction) -> Result<PaletteCommandResult, UiError>,
    FPlayerPoll: FnMut() -> Vec<PlayerEventUpdate>,
{
    run_interactive_full_inner(
        state,
        palette,
        options,
        Some(&mut refresh),
        Some(&mut command_handler),
        Some(&mut scan_handler),
        Some(&mut playback_handler),
        Some(&mut player_poll),
    )
}

#[allow(clippy::too_many_arguments)]
fn run_interactive_full_inner(
    state: &mut ShellState,
    palette: &Palette,
    options: RunOptions,
    refresh: Option<&mut RefreshSnapshotFn<'_>>,
    command_handler: Option<&mut CommandPaletteFn<'_>>,
    scan_handler: Option<&mut BackgroundScanFn<'_>>,
    playback_handler: Option<&mut PlaybackActionFn<'_>>,
    player_poll: Option<&mut PlayerPollFn<'_>>,
) -> Result<(), UiError> {
    enable_raw_mode().map_err(|e| UiError::Terminal(format!("enable_raw_mode failed: {e}")))?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)
        .map_err(|e| UiError::Terminal(format!("enter alt screen failed: {e}")))?;
    if options.mouse {
        execute!(stdout, EnableMouseCapture)
            .map_err(|e| UiError::Terminal(format!("enable mouse capture failed: {e}")))?;
    }
    execute!(stdout, EnableBracketedPaste)
        .map_err(|e| UiError::Terminal(format!("enable bracketed paste failed: {e}")))?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)
        .map_err(|e| UiError::Terminal(format!("terminal init failed: {e}")))?;

    let result = run_loop(
        &mut terminal,
        state,
        palette,
        options,
        refresh,
        command_handler,
        scan_handler,
        playback_handler,
        player_poll,
    );

    let _ = execute!(terminal.backend_mut(), DisableBracketedPaste);
    if options.mouse {
        let _ = execute!(terminal.backend_mut(), DisableMouseCapture);
    }
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    drop(terminal);
    let _ = disable_raw_mode();

    result
}

#[allow(clippy::too_many_arguments)]
fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &mut ShellState,
    palette: &Palette,
    options: RunOptions,
    mut refresh: Option<&mut RefreshSnapshotFn<'_>>,
    mut command_handler: Option<&mut CommandPaletteFn<'_>>,
    mut scan_handler: Option<&mut BackgroundScanFn<'_>>,
    mut playback_handler: Option<&mut PlaybackActionFn<'_>>,
    mut player_poll: Option<&mut PlayerPollFn<'_>>,
) -> Result<(), UiError> {
    use std::sync::mpsc;

    let mut last_draw = Instant::now();
    let mut last_areas = RenderAreas::default();
    let mut scan_rx: Option<mpsc::Receiver<ScanProgress>> = None;
    let mut last_scan_refresh = Instant::now();

    // Helper closure: handle a PaletteCommandResult, optionally starting a background scan.
    let handle_command_result = |state: &mut ShellState,
                                 result: PaletteCommandResult,
                                 refresh: &mut Option<&mut RefreshSnapshotFn<'_>>,
                                 scan_handler: &mut Option<&mut BackgroundScanFn<'_>>,
                                 scan_rx: &mut Option<mpsc::Receiver<ScanProgress>>| {
        state.status_message = Some(result.status_message);
        if result.refresh_requested {
            try_refresh_snapshot(state, refresh);
        }
        if let Some(scan_path) = result.background_scan_path {
            if let Some(handler) = scan_handler.as_mut() {
                state.scanning_path = Some(scan_path.clone());
                state.status_message = Some(format!("Scanning {}...", scan_path));
                *scan_rx = Some((*handler)(scan_path));
            }
        }
    };

    loop {
        // Poll background scan progress (non-blocking)
        if let Some(rx) = &scan_rx {
            loop {
                match rx.try_recv() {
                    Ok(ScanProgress::Progress { discovered, path }) => {
                        state.status_message =
                            Some(format!("Scanning {path}... ({discovered} tracks imported)"));
                        // Refresh snapshot frequently to show track count updates
                        if last_scan_refresh.elapsed() >= Duration::from_millis(750) {
                            try_refresh_snapshot(state, &mut refresh);
                            last_scan_refresh = Instant::now();
                        }
                    }
                    Ok(ScanProgress::Done { message }) => {
                        state.scanning_path = None;
                        state.status_message = Some(message);
                        try_refresh_snapshot(state, &mut refresh);
                        break;
                    }
                    Ok(ScanProgress::Error { message }) => {
                        state.scanning_path = None;
                        state.status_message = Some(format!("Scan failed: {message}"));
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        state.scanning_path = None;
                        state.status_message = Some("Scan finished".to_string());
                        try_refresh_snapshot(state, &mut refresh);
                        break;
                    }
                }
            }
            if state.scanning_path.is_none() {
                scan_rx = None;
            }
        }

        // Poll player events
        if let Some(poll_fn) = player_poll.as_mut() {
            for update in (*poll_fn)() {
                if !update.status.is_empty() {
                    state.playback_status = update.status;
                }
                if update.position_ms > 0 || update.duration_ms > 0 {
                    state.playback_position_ms = update.position_ms;
                    state.playback_duration_ms = update.duration_ms;
                }
                if !update.spectrum_bands.is_empty() {
                    state.spectrum_bands = update.spectrum_bands;
                }
                if update.track_finished {
                    // Auto-advance to next track
                    if let Some(handler) = playback_handler.as_mut() {
                        if let Ok(result) = (*handler)(PlaybackAction::Next) {
                            state.status_message = Some(result.status_message);
                            if result.refresh_requested {
                                try_refresh_snapshot(state, &mut refresh);
                            }
                        }
                    }
                }
            }
        }

        terminal
            .draw(|f| {
                last_areas = draw_shell(f, state, palette);
            })
            .map_err(|e| UiError::Terminal(format!("draw failed: {e}")))?;

        let elapsed = last_draw.elapsed();
        let timeout = options.tick_rate.saturating_sub(elapsed);
        if event::poll(timeout).map_err(|e| UiError::Terminal(format!("poll failed: {e}")))? {
            match event::read().map_err(|e| UiError::Terminal(format!("read event failed: {e}")))? {
                Event::Key(key) => match state.handle_key(key) {
                    KeyAction::Quit => return Ok(()),
                    KeyAction::Continue => {}
                    KeyAction::RefreshRequested => {
                        try_refresh_snapshot(state, &mut refresh);
                    }
                    KeyAction::CommandSubmitted(command) => {
                        if let Some(handler) = command_handler.as_mut() {
                            match (*handler)(&command) {
                                Ok(result) => {
                                    handle_command_result(
                                        state,
                                        result,
                                        &mut refresh,
                                        &mut scan_handler,
                                        &mut scan_rx,
                                    );
                                }
                                Err(err) => {
                                    state.status_message = Some(format!("Command failed: {err}"));
                                }
                            }
                        } else {
                            state.status_message = Some(format!(
                                "Command palette unavailable in this shell mode: {command}"
                            ));
                        }
                    }
                    KeyAction::Playback(action) => {
                        if let Some(handler) = playback_handler.as_mut() {
                            match (*handler)(action) {
                                Ok(result) => {
                                    state.status_message = Some(result.status_message);
                                    if result.refresh_requested {
                                        try_refresh_snapshot(state, &mut refresh);
                                    }
                                }
                                Err(err) => {
                                    state.status_message =
                                        Some(format!("Playback error: {err}"));
                                }
                            }
                        }
                    }
                },
                Event::Mouse(mouse) => {
                    if options.mouse {
                        let mouse_action = state.handle_mouse(mouse, &last_areas);
                        if let KeyAction::Playback(action) = mouse_action {
                            if let Some(handler) = playback_handler.as_mut() {
                                match (*handler)(action) {
                                    Ok(result) => {
                                        state.status_message = Some(result.status_message);
                                        if result.refresh_requested {
                                            try_refresh_snapshot(state, &mut refresh);
                                        }
                                    }
                                    Err(err) => {
                                        state.status_message =
                                            Some(format!("Playback error: {err}"));
                                    }
                                }
                            }
                        }
                    }
                }
                Event::Resize(_, _) => {}
                Event::Paste(content) => {
                    let paths: Vec<String> = content
                        .lines()
                        .map(|l| l.trim().to_string())
                        .filter(|l| !l.is_empty())
                        .collect();

                    for path_str in paths {
                        let path = std::path::Path::new(&path_str);
                        if path.is_dir() {
                            match state.input_mode {
                                InputMode::AddMusic | InputMode::Welcome => {
                                    if let Some(browser) = state.file_browser.as_mut() {
                                        browser.navigate_to(path);
                                    }
                                }
                                InputMode::Normal => {
                                    if let Some(handler) = command_handler.as_mut() {
                                        match (*handler)(&format!("__add_root {path_str}")) {
                                            Ok(result) => {
                                                handle_command_result(
                                                    state,
                                                    result,
                                                    &mut refresh,
                                                    &mut scan_handler,
                                                    &mut scan_rx,
                                                );
                                            }
                                            Err(err) => {
                                                state.status_message =
                                                    Some(format!("Drop failed: {err}"));
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        } else {
                            state.status_message =
                                Some(format!("Not a directory: {path_str}"));
                        }
                    }
                }
                Event::FocusGained | Event::FocusLost => {}
            }
        }
        last_draw = Instant::now();
    }
}

pub fn render_once_to_text(
    state: &mut ShellState,
    palette: &Palette,
    width: u16,
    height: u16,
) -> Result<String, UiError> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend)
        .map_err(|e| UiError::Terminal(format!("test terminal init failed: {e}")))?;
    terminal
        .draw(|f| {
            let _ = draw_shell(f, state, palette);
        })
        .map_err(|e| UiError::Terminal(format!("test draw failed: {e}")))?;

    let buffer = terminal.backend().buffer().clone();
    let mut lines = Vec::with_capacity(height as usize);
    for y in 0..height {
        let mut line = String::new();
        for x in 0..width {
            let symbol = buffer[(x, y)].symbol();
            line.push_str(symbol);
        }
        while line.ends_with(' ') {
            line.pop();
        }
        lines.push(line);
    }
    Ok(lines.join("\n"))
}

fn draw_shell(frame: &mut Frame, state: &mut ShellState, palette: &Palette) -> RenderAreas {
    let root = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(palette.bg_root())),
        root,
    );

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(12), Constraint::Length(4)])
        .split(root);
    let main = vertical[0];
    let footer = vertical[1];

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(27), Constraint::Percentage(73)])
        .split(main);

    let left_sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(1),  // gap
            Constraint::Length(7),
            Constraint::Length(1),  // gap
            Constraint::Min(8),
        ])
        .split(cols[0]);

    let right_sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),
            Constraint::Length(1),  // gap
            Constraint::Min(12),
        ])
        .split(cols[1]);

    let (header_area, library_rows_area) = library_panel_inner_areas(right_sections[2]);
    let areas = RenderAreas {
        roots: PaneArea::bordered(left_sections[0], 1),
        browse: left_sections[2],
        playlists: PaneArea::bordered(left_sections[4], 1),
        tracks: PaneArea::from_list_area(right_sections[2], library_rows_area, 1),
        track_header: header_area,
        track_col_offsets: TrackColumnOffsets::default(),
    };
    state.sync_scroll_offsets(&areas);

    render_roots(frame, left_sections[0], state, palette);
    render_browse_modes(frame, left_sections[2], state, palette);
    render_playlists(frame, left_sections[4], state, palette);
    render_now_playing(frame, right_sections[0], state, palette);
    let col_offsets = render_tracks(frame, right_sections[2], state, palette);
    // Can't mutate areas after sync_scroll_offsets borrow, so we rebuild with col_offsets
    let areas = RenderAreas { track_col_offsets: col_offsets, ..areas };
    render_status(frame, footer, state, palette);

    if state.show_help {
        render_help_overlay(frame, palette);
    }
    if state.input_mode == InputMode::CommandPalette {
        render_command_palette_overlay(frame, state, palette);
    }
    if state.input_mode == InputMode::AddMusic {
        render_add_music_overlay(frame, state, palette, false);
    }
    if state.input_mode == InputMode::Welcome {
        render_add_music_overlay(frame, state, palette, true);
    }
    if state.input_mode == InputMode::TrackInfo {
        render_track_info_overlay(frame, state, palette);
    }
    if state.input_mode == InputMode::Settings {
        render_settings_overlay(frame, state, palette);
    }

    // Fade-in effect on the Now Playing panel when a new track starts.
    const FADE_DURATION_MS: u128 = 350;
    if let Some(started) = state.track_change_time {
        let elapsed = started.elapsed();
        if elapsed.as_millis() < FADE_DURATION_MS {
            let mut effect = fx::fade_from_fg(
                palette.text_muted,
                EffectTimer::from_ms(FADE_DURATION_MS as u32, Interpolation::QuadOut),
            );
            effect.process(elapsed.into(), frame.buffer_mut(), right_sections[0]);
        } else {
            state.track_change_time = None;
        }
    }

    areas
}

fn render_roots(frame: &mut Frame, area: Rect, state: &mut ShellState, palette: &Palette) {
    let items: Vec<ListItem> = if state.snapshot.roots.is_empty() {
        vec![
            ListItem::new(Line::from("")),
            ListItem::new(Line::from(Span::styled(
                "No library roots",
                Style::default().fg(palette.text_muted),
            ))),
            ListItem::new(Line::from("")),
            ListItem::new(Line::from(Span::styled(
                "  Press a to add a music folder",
                Style::default().fg(palette.text_muted),
            ))),
        ]
    } else {
        state
            .snapshot
            .roots
            .iter()
            .map(|r| {
                let icon = icon_glyph(state.snapshot.icon_mode, IconToken::Folder);
                let detail = r.detail.as_deref().unwrap_or("");
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{icon} "), Style::default().fg(palette.accent)),
                    Span::raw(&r.label),
                    Span::styled(
                        if detail.is_empty() {
                            String::new()
                        } else {
                            format!("  {detail}")
                        },
                        Style::default().fg(palette.text_muted),
                    ),
                ]))
            })
            .collect()
    };

    let block = pane_block("Library Roots", state.focus == FocusPane::Sources, palette);
    let content_area = padded_inner(area);
    frame.render_widget(block, area);

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(palette.selection_bg)
                .fg(palette.text)
                .add_modifier(Modifier::BOLD),
        );
    let mut list_state = ListState::default().with_selected(Some(min(
        state.selected_root,
        state.snapshot.roots.len().saturating_sub(1),
    )));
    list_state = list_state.with_offset(state.roots_scroll);
    frame.render_stateful_widget(list, content_area, &mut list_state);
}

fn render_browse_modes(frame: &mut Frame, area: Rect, state: &ShellState, palette: &Palette) {
    let focused = state.focus == FocusPane::Browse;
    let block = pane_block("Browse Library", focused, palette);
    let content_area = padded_inner(area);
    frame.render_widget(block, area);

    let modes = crate::browse::BrowseMode::all();
    let mode_icons = [IconToken::Track, IconToken::Folder, IconToken::Playlist];
    let mut lines = Vec::new();

    for (idx, mode) in modes.iter().enumerate() {
        let is_current = idx == state.browse.mode_index;
        let highlight = focused && !state.browse.show_items && is_current;
        let icon = mode_icons.get(idx).copied().unwrap_or(IconToken::Folder);
        let mut spans = vec![
            Span::styled(
                format!("{} ", icon_glyph(state.snapshot.icon_mode, icon)),
                Style::default().fg(if is_current {
                    palette.focus
                } else {
                    palette.text_muted
                }),
            ),
            Span::styled(
                mode.label(),
                Style::default()
                    .fg(if is_current {
                        palette.text
                    } else {
                        palette.text_muted
                    })
                    .add_modifier(if is_current {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
        ];
        if highlight {
            for span in &mut spans {
                span.style = span.style.add_modifier(Modifier::REVERSED);
            }
        }
        lines.push(Line::from(spans));
    }

    if state.browse.show_items && !state.browse.items.is_empty() {
        lines.push(Line::from(""));
        let visible_height = content_area
            .height
            .saturating_sub(lines.len() as u16) as usize;
        let scroll = state.browse.item_scroll;
        let end = (scroll + visible_height).min(state.browse.items.len());
        for idx in scroll..end {
            let item = &state.browse.items[idx];
            let is_selected = idx == state.browse.item_index;
            let is_active = state.browse.selected_item.as_deref() == Some(item.as_str());
            let highlight = focused && state.browse.show_items && is_selected;
            let fg = if is_active {
                palette.focus
            } else if is_selected {
                palette.text
            } else {
                palette.text_muted
            };
            let mut style = Style::default().fg(fg);
            if is_active {
                style = style.add_modifier(Modifier::BOLD);
            }
            if highlight {
                style = style.add_modifier(Modifier::REVERSED);
            }
            lines.push(Line::from(Span::styled(
                format!("  {item}"),
                style,
            )));
        }
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, content_area);
}

fn render_playlists(frame: &mut Frame, area: Rect, state: &mut ShellState, palette: &Palette) {
    let items: Vec<ListItem> = if state.snapshot.playlists.is_empty() {
        vec![ListItem::new(Line::from("No playlists"))]
    } else {
        state
            .snapshot
            .playlists
            .iter()
            .map(|p| {
                let icon = icon_glyph(state.snapshot.icon_mode, IconToken::Playlist);
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{icon} "), Style::default().fg(palette.accent_2)),
                    Span::raw(&p.label),
                ]))
            })
            .collect()
    };

    let block = pane_block("Playlists", state.focus == FocusPane::Inspector, palette);
    let content_area = padded_inner(area);
    frame.render_widget(block, area);

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(palette.selection_bg)
                .add_modifier(Modifier::BOLD),
        );
    let mut list_state = ListState::default().with_selected(Some(min(
        state.selected_playlist,
        state.snapshot.playlists.len().saturating_sub(1),
    )));
    list_state = list_state.with_offset(state.playlists_scroll);
    frame.render_stateful_widget(list, content_area, &mut list_state);
}

fn render_tracks(frame: &mut Frame, area: Rect, state: &mut ShellState, palette: &Palette) -> TrackColumnOffsets {
    let title = if state.track_filter_query.is_empty() {
        let filtered = state.filtered_track_count();
        let total = state.snapshot.total_track_count;
        if filtered < total {
            format!("Library ({}/{})", filtered, total)
        } else {
            format!("Library ({})", filtered)
        }
    } else {
        format!(
            "Library ({}/{}) /{}",
            state.filtered_track_count(),
            state.snapshot.total_track_count,
            state.track_filter_query
        )
    };
    let outer_block = pane_block(&title, state.focus == FocusPane::Tracks, palette);
    let inner = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    if inner.width == 0 || inner.height == 0 {
        return TrackColumnOffsets::default();
    }

    let (header_area, rows_area) = library_panel_inner_areas(area);

    // Calculate column widths proportionally
    let total_w = inner.width as usize;
    let col_icon = 4usize;
    let col_time = 7;
    let col_fav = 4;
    let col_quality = 14;
    let fixed = col_icon + col_time + col_fav + col_quality;
    let flexible = total_w.saturating_sub(fixed);
    let col_title = flexible * 45 / 100;
    let col_artist = flexible.saturating_sub(col_title);

    let header_x = inner.x;
    let offsets = TrackColumnOffsets {
        title_start: header_x + col_icon as u16,
        time_start: header_x + (col_icon + col_title) as u16,
        artist_start: header_x + (col_icon + col_title + col_time) as u16,
        quality_start: header_x + (col_icon + col_title + col_time + col_artist + col_fav) as u16,
    };

    let sort_indicator = |col: SortColumn| -> &str {
        if state.sort_column == col {
            if state.sort_ascending { " ▲" } else { " ▼" }
        } else {
            ""
        }
    };
    let sort_style = |col: SortColumn| -> Style {
        if state.sort_column == col {
            Style::default().fg(palette.accent)
        } else {
            Style::default().fg(palette.text_muted)
        }
    };

    let header = Line::from(vec![
        Span::styled(pad_cell("", col_icon), Style::default().fg(palette.text_muted)),
        Span::styled(
            pad_cell(&format!("Title{}", sort_indicator(SortColumn::Title)), col_title),
            sort_style(SortColumn::Title),
        ),
        Span::styled(
            pad_cell(&format!("Time{}", sort_indicator(SortColumn::Time)), col_time),
            sort_style(SortColumn::Time),
        ),
        Span::styled(
            pad_cell(&format!("Artist{}", sort_indicator(SortColumn::Artist)), col_artist),
            sort_style(SortColumn::Artist),
        ),
        Span::styled(pad_cell("Fav", col_fav), Style::default().fg(palette.text_muted)),
        Span::styled(
            format!("Quality{}", sort_indicator(SortColumn::Quality)),
            sort_style(SortColumn::Quality),
        ),
    ]);
    frame.render_widget(Paragraph::new(header), header_area);

    let items: Vec<ListItem> = if state.filtered_track_count() == 0 {
        if state.snapshot.tracks.is_empty() && state.snapshot.roots.is_empty() {
            vec![
                ListItem::new(Line::from("")),
                ListItem::new(Line::from(Span::styled(
                    "No tracks in library",
                    Style::default().fg(palette.text_muted),
                ))),
                ListItem::new(Line::from("")),
                ListItem::new(Line::from(Span::styled(
                    "  Add a music folder to get started",
                    Style::default().fg(palette.text_muted),
                ))),
                ListItem::new(Line::from(Span::styled(
                    "  Press a or : then root add /path",
                    Style::default().fg(palette.text_muted),
                ))),
            ]
        } else if state.snapshot.tracks.is_empty() {
            vec![
                ListItem::new(Line::from("")),
                ListItem::new(Line::from(Span::styled(
                    "No tracks in library",
                    Style::default().fg(palette.text_muted),
                ))),
                ListItem::new(Line::from("")),
                ListItem::new(Line::from(Span::styled(
                    "  Press : then scan roots to import",
                    Style::default().fg(palette.text_muted),
                ))),
            ]
        } else {
            vec![ListItem::new(Line::from(Span::styled(
                "No tracks match current filter",
                Style::default().fg(palette.text_muted),
            )))]
        }
    } else {
        state
            .filtered_track_iter()
            .map(|t| {
                let icon = icon_glyph(state.snapshot.icon_mode, IconToken::Track);
                let row = format!(
                    "{} {}{}{}{}{}",
                    pad_cell(icon, col_icon.saturating_sub(1)),
                    pad_cell(&truncate_text(&t.title, col_title.saturating_sub(1)), col_title),
                    pad_cell(&format_duration_short(t.duration_ms), col_time),
                    pad_cell(&truncate_text(&t.artist, col_artist.saturating_sub(1)), col_artist),
                    pad_cell("☆", col_fav),
                    format_tech_compact(t.sample_rate, t.bit_depth, t.channels)
                );
                ListItem::new(Line::from(Span::styled(
                    row,
                    Style::default().fg(palette.text),
                )))
            })
            .collect()
    };

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(palette.selection_bg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▌ ")
        .repeat_highlight_symbol(true);
    let selected = if state.filtered_track_count() == 0 {
        None
    } else {
        Some(min(
            state.selected_track,
            state.filtered_track_count().saturating_sub(1),
        ))
    };
    let mut list_state = ListState::default().with_selected(selected);
    list_state = list_state.with_offset(state.tracks_scroll);
    frame.render_stateful_widget(list, rows_area, &mut list_state);
    offsets
}

fn render_now_playing(frame: &mut Frame, area: Rect, state: &mut ShellState, palette: &Palette) {
    let block = pane_block("Now Playing", false, palette);
    let content_area = padded_inner(area);
    frame.render_widget(block, area);

    let is_playing = state.playback_status == "playing";
    let is_paused = state.playback_status == "paused";
    let has_track = !state.snapshot.now_playing_title.is_empty();

    if has_track {
        // Update artwork state when track changes
        state.artwork.update(
            &state.snapshot.now_playing_path,
            state.snapshot.now_playing_artwork.as_deref(),
            state.snapshot.pixel_art_enabled,
            state.snapshot.pixel_art_cell_size,
        );

        // Split content area: artwork on left (square), text on right
        let show_art = state.artwork.has_image() && content_area.height >= 3;
        let art_width = if show_art {
            content_area.height.saturating_mul(2).min(content_area.width / 3)
        } else {
            0
        };
        let text_area = Rect {
            x: content_area.x + art_width,
            y: content_area.y,
            width: content_area.width.saturating_sub(art_width),
            height: content_area.height,
        };

        let status_icon = if is_playing {
            ">"
        } else if is_paused {
            "||"
        } else {
            "[]"
        };

        // Row 0: status icon + title + artist/album
        let title_line = Line::from(vec![
            Span::styled(
                format!("{status_icon} "),
                Style::default().fg(if is_playing {
                    palette.progress_fill
                } else {
                    palette.text_muted
                }),
            ),
            Span::styled(
                state.snapshot.now_playing_title.as_str(),
                Style::default()
                    .fg(palette.text)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "  {}  {}",
                    state.snapshot.now_playing_artist, state.snapshot.now_playing_album
                ),
                Style::default().fg(palette.text_muted),
            ),
        ]);
        let title_area = Rect {
            x: text_area.x,
            y: text_area.y,
            width: text_area.width,
            height: 1,
        };
        frame.render_widget(Paragraph::new(title_line), title_area);

        // Row 1: seek bar
        let position = state.playback_position_ms;
        let duration = state.playback_duration_ms;
        let progress = if duration > 0 {
            position as f32 / duration as f32
        } else {
            0.0
        };
        let elapsed_str = format_ms(position);
        let remaining_ms = duration.saturating_sub(position);
        let remaining_str = format_ms(remaining_ms);

        let seek_bar_rect = Rect {
            x: text_area.x,
            y: text_area.y + 1,
            width: text_area.width,
            height: 1,
        };
        state.seek_bar_area = seek_bar_rect;
        frame.render_widget(
            crate::seekbar::SeekBar {
                progress,
                elapsed: &elapsed_str,
                remaining: &remaining_str,
                palette,
            },
            seek_bar_rect,
        );

        // Row 2: transport info
        let info_line = Line::from(vec![
            Span::styled(
                format!(
                    "vol: {}%  {}  {}  {}/{}",
                    (state.snapshot.volume * 100.0).round() as u32,
                    if state.snapshot.shuffle { "shuffle" } else { "" },
                    match state.snapshot.repeat_mode.as_str() {
                        "one" => "repeat:1",
                        "all" => "repeat:all",
                        _ => "",
                    },
                    state.snapshot.queue_position,
                    state.snapshot.queue_length,
                ),
                Style::default().fg(palette.text_muted),
            ),
        ]);
        let info_area = Rect {
            x: text_area.x,
            y: text_area.y + 2,
            width: text_area.width,
            height: 1,
        };
        frame.render_widget(Paragraph::new(info_line), info_area);

        // Spectrum visualizer: fills remaining height below the three fixed rows
        let viz_top = text_area.y + 3;
        let viz_bottom = text_area.y + text_area.height;
        if is_playing
            && !state.spectrum_bands.is_empty()
            && viz_bottom > viz_top
            && text_area.width >= 4
        {
            let viz_area = Rect {
                x: text_area.x,
                y: viz_top,
                width: text_area.width,
                height: viz_bottom - viz_top,
            };
            frame.render_widget(
                crate::visualizer::SpectrumWidget {
                    bands: &state.spectrum_bands,
                    palette,
                },
                viz_area,
            );
        }

        // Render album artwork on the left
        if show_art {
            let art_area = Rect {
                x: content_area.x,
                y: content_area.y,
                width: art_width,
                height: content_area.height,
            };
            if let Some(protocol) = &mut state.artwork.current_image {
                frame.render_stateful_widget(
                    ratatui_image::StatefulImage::default(),
                    art_area,
                    protocol,
                );
            }
        }
    } else {
        // Reset seek bar area when no track is playing
        state.seek_bar_area = Rect::default();

        let mut lines = Vec::new();
        if let Some(track) = state.selected_track_item() {
            lines.push(Line::from(Span::styled(
                format!("{} - {}", track.title, track.artist),
                Style::default().fg(palette.text_muted),
            )));
            lines.push(Line::from(Span::styled(
                "Press Enter to play",
                Style::default().fg(palette.text_muted),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                "No track selected",
                Style::default().fg(palette.text_muted),
            )));
        }
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: true });
        frame.render_widget(paragraph, content_area);
    }
}

fn format_ms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let minutes = total_secs / 60;
    let seconds = total_secs % 60;
    format!("{minutes:02}:{seconds:02}")
}


fn render_status(frame: &mut Frame, area: Rect, state: &ShellState, palette: &Palette) {
    let block = pane_block("", false, palette);
    let content_area = padded_inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    let filter_info = if state.track_filter_query.is_empty() {
        String::new()
    } else {
        format!("  filter: /{}", state.track_filter_query)
    };
    let mut title_spans = vec![
        Span::styled(
            state.snapshot.app_title.as_str(),
            Style::default().fg(palette.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {} tracks{filter_info}", state.snapshot.total_track_count),
            Style::default().fg(palette.text_muted),
        ),
    ];
    if state.scanning_path.is_some() {
        title_spans.push(Span::styled(
            "  [scanning...]",
            Style::default().fg(palette.warning).add_modifier(Modifier::BOLD),
        ));
    }
    lines.push(Line::from(title_spans));
    let status_msg = state.status_message.as_deref().unwrap_or(default_status_message());
    lines.push(Line::from(Span::styled(
        status_msg,
        Style::default().fg(if state.scanning_path.is_some() {
            palette.accent
        } else {
            palette.text_muted
        }),
    )));
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: true });
    frame.render_widget(paragraph, content_area);

    // Settings shortcut, bottom right
    let hint = " ?: help  ,: settings ";
    let hint_width = hint.len() as u16;
    if content_area.width > hint_width + 2 {
        let hint_area = Rect {
            x: content_area.x + content_area.width - hint_width,
            y: content_area.y,
            width: hint_width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(Span::styled(hint, Style::default().fg(palette.text_muted))),
            hint_area,
        );
    }
}

fn render_track_info_overlay(frame: &mut Frame, state: &ShellState, palette: &Palette) {
    let track = match state.selected_track_item() {
        Some(t) => t,
        None => return,
    };

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("Title:   ", Style::default().fg(palette.text_muted)),
            Span::styled(track.title.as_str(), Style::default().fg(palette.text).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("Artist:  ", Style::default().fg(palette.text_muted)),
            Span::styled(track.artist.as_str(), Style::default().fg(palette.text)),
        ]),
        Line::from(vec![
            Span::styled("Album:   ", Style::default().fg(palette.text_muted)),
            Span::styled(track.album.as_str(), Style::default().fg(palette.text)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Path:    ", Style::default().fg(palette.text_muted)),
            Span::styled(track.path.as_str(), Style::default().fg(palette.text)),
        ]),
        Line::from(""),
    ];

    let duration = track.duration_ms.map(|ms| {
        let secs = ms / 1000;
        format!("{}:{:02}", secs / 60, secs % 60)
    }).unwrap_or_else(|| "--:--".to_string());

    let sample_rate = track.sample_rate.map(|sr| format!("{} Hz", sr)).unwrap_or_else(|| "-".to_string());
    let channels = track.channels.map(|ch| format!("{}", ch)).unwrap_or_else(|| "-".to_string());
    let bit_depth = track.bit_depth.map(|bd| format!("{}-bit", bd)).unwrap_or_else(|| "-".to_string());

    lines.push(Line::from(vec![
        Span::styled("Duration:    ", Style::default().fg(palette.text_muted)),
        Span::styled(duration, Style::default().fg(palette.text)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Sample Rate: ", Style::default().fg(palette.text_muted)),
        Span::styled(sample_rate, Style::default().fg(palette.text)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Channels:    ", Style::default().fg(palette.text_muted)),
        Span::styled(channels, Style::default().fg(palette.text)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Bit Depth:   ", Style::default().fg(palette.text_muted)),
        Span::styled(bit_depth, Style::default().fg(palette.text)),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Press Esc or i to close",
        Style::default().fg(palette.text_muted),
    )));

    crate::modal::render_modal(frame, "Track Info", lines, 60, 50, palette);
}

fn render_settings_overlay(frame: &mut Frame, state: &ShellState, palette: &Palette) {
    let settings: Vec<(&str, String)> = vec![
        ("Theme", state.snapshot.theme_name.clone()),
        ("Use Theme Background", format!("{}", state.snapshot.setting_use_theme_bg)),
        ("Icon Pack", state.snapshot.setting_icon_pack.clone()),
        ("Pixel Art Artwork", format!("{}", state.snapshot.setting_pixel_art)),
        ("Pixel Art Cell Size", format!("{}", state.snapshot.setting_pixel_art_cell_size)),
        ("Color Scheme", state.snapshot.setting_color_scheme.clone()),
    ];

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    for (i, (label, value)) in settings.iter().enumerate() {
        let is_selected = i == state.settings_index;
        let marker = if is_selected { " > " } else { "   " };
        let style = if is_selected {
            Style::default().fg(palette.text).bg(palette.selection_bg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette.text)
        };
        let value_style = if is_selected {
            Style::default().fg(palette.accent).bg(palette.selection_bg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette.accent)
        };

        let restart_note = if i == 0 || i == 5 { "  (restart to apply)" } else { "" };
        lines.push(Line::from(vec![
            Span::styled(marker, style),
            Span::styled(format!("{label:<25}"), style),
            Span::styled(value.as_str(), value_style),
            Span::styled(restart_note, Style::default().fg(palette.text_muted)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "   Enter/Space: change   Esc: close",
        Style::default().fg(palette.text_muted),
    )));

    crate::modal::render_modal(frame, "Settings", lines, 55, 45, palette);
}

fn render_help_overlay(frame: &mut Frame, palette: &Palette) {
    let area = centered_rect(65, 60, frame.area());
    frame.render_widget(Clear, area);
    let lines = vec![
        Line::from(Span::styled(
            "Auric Keyboard Shortcuts",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("Tab / Shift-Tab: switch pane focus"),
        Line::from("Enter: play selected track"),
        Line::from("Space: play/pause"),
        Line::from("n / N: next / previous track"),
        Line::from("+ / -: volume up / down"),
        Line::from("s: toggle shuffle"),
        Line::from("o: cycle sort column (click header to sort)"),
        Line::from("a: add music folder"),
        Line::from("j/k or arrows: move selection"),
        Line::from("PgUp/PgDn: page movement"),
        Line::from("g / G: first / last"),
        Line::from("/: track filter mode (type to filter, Enter/Esc close)"),
        Line::from(": or Ctrl-P: command palette"),
        Line::from("Mouse click: focus pane + select row"),
        Line::from("Mouse wheel: scroll selected pane"),
        Line::from("q or Ctrl-C: quit"),
        Line::from("r: refresh library"),
        Line::from("i: track info"),
        Line::from(",: settings"),
        Line::from("?: toggle this help"),
    ];
    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Help")
                .border_style(Style::default().fg(palette.focus))
                .style(Style::default().bg(palette.bg_panel()).fg(palette.text)),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn render_command_palette_overlay(frame: &mut Frame, state: &ShellState, palette: &Palette) {
    let frame_area = frame.area();
    let width = frame_area.width.saturating_sub(8).clamp(24, 88);
    let x = frame_area.x + frame_area.width.saturating_sub(width) / 2;
    let height = 5u16;
    let y = frame_area
        .y
        .saturating_add(frame_area.height.saturating_sub(height + 2));
    let area = Rect::new(x, y, width, height);
    frame.render_widget(Clear, area);

    let lines = vec![
        Line::from(vec![
            Span::styled(":", Style::default().fg(palette.focus).add_modifier(Modifier::BOLD)),
            Span::styled(
                state.command_palette_input.as_str(),
                Style::default().fg(palette.text),
            ),
        ]),
        Line::from(Span::styled(
            "Examples: help | refresh | scan roots | feature enable visualizer | root add /path --watched",
            Style::default().fg(palette.text_muted),
        )),
        Line::from(Span::styled(
            "playlist create <name> | playlist delete <id> | scan path <dir> --prune",
            Style::default().fg(palette.text_muted),
        )),
    ];
    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Command Palette")
                .border_style(Style::default().fg(palette.focus))
                .style(Style::default().bg(palette.bg_panel()).fg(palette.text)),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn render_add_music_overlay(
    frame: &mut Frame,
    state: &ShellState,
    palette: &Palette,
    is_welcome: bool,
) {
    let frame_area = frame.area();
    let width = (frame_area.width * 60 / 100).max(40).min(frame_area.width.saturating_sub(4));
    let height = (frame_area.height * 70 / 100).max(16).min(frame_area.height.saturating_sub(4));
    let x = frame_area.x + (frame_area.width.saturating_sub(width)) / 2;
    let y = frame_area.y + (frame_area.height.saturating_sub(height)) / 2;
    let area = Rect::new(x, y, width, height);
    frame.render_widget(Clear, area);

    let title = if is_welcome { " Welcome to auric " } else { " Add Music " };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(palette.focus))
        .style(Style::default().bg(palette.bg_panel()).fg(palette.text));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let content = Rect {
        x: inner.x.saturating_add(1),
        y: inner.y,
        width: inner.width.saturating_sub(2),
        height: inner.height,
    };

    if content.height < 4 || content.width < 10 {
        return;
    }

    let browser = match &state.file_browser {
        Some(b) => b,
        None => return,
    };

    let mut lines: Vec<Line> = Vec::new();

    if is_welcome {
        lines.push(Line::from(Span::styled(
            "Add a folder to get started.",
            Style::default().fg(palette.text_muted),
        )));
        lines.push(Line::from(""));
    }

    let input_style = if browser.input_focused {
        Style::default().fg(palette.text).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(palette.text_muted)
    };
    lines.push(Line::from(vec![
        Span::styled("Path: ", Style::default().fg(palette.text_muted)),
        Span::styled(&browser.path_input, input_style),
        if browser.input_focused {
            Span::styled("_", Style::default().fg(palette.focus).add_modifier(Modifier::SLOW_BLINK))
        } else {
            Span::raw("")
        },
    ]));
    lines.push(Line::from(""));

    let dir_display = browser
        .current_dir()
        .display()
        .to_string()
        .replace(
            &home_dir()
                .map(|h| h.display().to_string())
                .unwrap_or_default(),
            "~",
        );
    lines.push(Line::from(Span::styled(
        format!("{dir_display}/"),
        Style::default().fg(palette.accent).add_modifier(Modifier::BOLD),
    )));

    let entries = browser.entries();
    let header_lines = lines.len() as u16;
    let footer_lines = if state.terminal_caps.supports_drag_drop { 2u16 } else { 1u16 };
    let max_visible = content.height.saturating_sub(header_lines + footer_lines + 1) as usize;
    let start = if max_visible > 0 && browser.selected >= max_visible {
        browser.selected - max_visible + 1
    } else {
        0
    };
    let end = (start + max_visible).min(entries.len());

    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (empty directory)",
            Style::default().fg(palette.text_muted),
        )));
    } else {
        for (i, entry) in entries[start..end].iter().enumerate() {
            let actual_idx = start + i;
            let is_selected = actual_idx == browser.selected;
            let marker = if is_selected { ">" } else { " " };
            let icon = if entry.is_dir { "/" } else { "" };
            let style = if is_selected {
                Style::default().fg(palette.text).bg(palette.selection_bg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(palette.text)
            };
            lines.push(Line::from(Span::styled(
                format!("  {marker} {}{icon}", entry.name),
                style,
            )));
        }
    }

    let used = lines.len() as u16;
    let target = content.height.saturating_sub(footer_lines);
    if used < target {
        for _ in 0..(target - used) {
            lines.push(Line::from(""));
        }
    }

    let esc_label = if is_welcome { "esc skip" } else { "esc cancel" };
    lines.push(Line::from(Span::styled(
        format!("  space add  enter open  backspace up  tab path input  {esc_label}"),
        Style::default().fg(palette.text_muted),
    )));

    if state.terminal_caps.supports_drag_drop {
        lines.push(Line::from(Span::styled(
            "  drag folders here to add",
            Style::default().fg(palette.text_muted),
        )));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, content);
}

fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

fn pane_block<'a>(title: &'a str, focused: bool, palette: &Palette) -> Block<'a> {
    let border_style = if focused {
        Style::default().fg(palette.border_focused)
    } else {
        Style::default().fg(palette.border_unfocused)
    };
    let title_style = if focused {
        Style::default().fg(palette.border_focused).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(palette.text_muted)
    };
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(format!(" {title} "), title_style))
        .border_style(border_style)
        .style(Style::default().bg(palette.bg_panel()).fg(palette.text))
}

fn library_panel_inner_areas(area: Rect) -> (Rect, Rect) {
    let inner = inner_rect(area);
    if inner.width == 0 || inner.height == 0 {
        return (
            Rect::new(inner.x, inner.y, 0, 0),
            Rect::new(inner.x, inner.y, 0, 0),
        );
    }
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);
    (sections[0], sections[1])
}

fn pad_cell(text: &str, width: usize) -> String {
    let truncated = truncate_text(text, width.saturating_sub(1));
    format!("{truncated:<width$}")
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    if max_chars == 1 {
        return "…".to_string();
    }
    let mut out = text.chars().take(max_chars - 1).collect::<String>();
    out.push('…');
    out
}

fn format_duration_short(duration_ms: Option<i64>) -> String {
    let Some(ms) = duration_ms else {
        return "--:--".to_string();
    };
    if ms <= 0 {
        return "--:--".to_string();
    }
    let total_secs = ms / 1000;
    let minutes = total_secs / 60;
    let seconds = total_secs % 60;
    format!("{minutes:02}:{seconds:02}")
}

fn format_tech_compact(
    sample_rate: Option<i64>,
    bit_depth: Option<i64>,
    channels: Option<i64>,
) -> String {
    let sr = sample_rate.unwrap_or_default();
    let bd = bit_depth.unwrap_or_default();
    let ch = channels.unwrap_or_default();
    let sr_khz = sr / 1000;
    let sr_rem = (sr % 1000) / 100;
    let sr_str = if sr_khz > 0 && sr_rem > 0 {
        format!("{sr_khz}.{sr_rem}k")
    } else if sr_khz > 0 {
        format!("{sr_khz}k")
    } else {
        format!("{sr}Hz")
    };
    if bd > 0 {
        format!("{bd}b/{sr_str} {ch}ch")
    } else {
        format!("{sr_str} {ch}ch")
    }
}

fn default_status_message() -> &'static str {
    "Enter: play  Space: pause  n/N: next/prev  +/-: volume  a: add music  ?: help"
}

fn track_matches_query(track: &ShellTrackItem, query: &str) -> bool {
    track.title.to_lowercase().contains(query)
        || track.artist.to_lowercase().contains(query)
        || track.album.to_lowercase().contains(query)
        || track.path.to_lowercase().contains(query)
}

fn normalize_scroll(offset: usize, selected: usize, len: usize, visible_items: usize) -> usize {
    if len == 0 || visible_items == 0 {
        return 0;
    }
    let max_offset = len.saturating_sub(visible_items);
    let mut offset = offset.min(max_offset);
    let selected = selected.min(len.saturating_sub(1));
    if selected < offset {
        offset = selected;
    } else if selected >= offset.saturating_add(visible_items) {
        offset = selected.saturating_add(1).saturating_sub(visible_items);
    }
    offset.min(max_offset)
}

fn try_refresh_snapshot(state: &mut ShellState, refresh: &mut Option<&mut RefreshSnapshotFn<'_>>) {
    if let Some(refresh_fn) = refresh.as_mut() {
        match (*refresh_fn)() {
            Ok(snapshot) => {
                let total_tracks = snapshot.total_track_count;
                state.replace_snapshot(snapshot);
                state.status_message = Some(format!(
                    "Library refreshed ({total_tracks} tracks)"
                ));
            }
            Err(err) => {
                state.status_message = Some(format!("Refresh failed: {err}"));
            }
        }
    } else {
        state.status_message = Some("Refresh not available in this shell mode".to_string());
    }
}


fn shift_index(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    let current = current.min(len - 1) as isize;
    let max = (len - 1) as isize;
    (current + delta).clamp(0, max) as usize
}

fn inner_rect(area: Rect) -> Rect {
    if area.width <= 2 || area.height <= 2 {
        return Rect::new(area.x, area.y, 0, 0);
    }
    Rect::new(area.x + 1, area.y + 1, area.width - 2, area.height - 2)
}

fn padded_inner(area: Rect) -> Rect {
    let inner = inner_rect(area);
    Rect {
        x: inner.x.saturating_add(1),
        y: inner.y,
        width: inner.width.saturating_sub(2),
        height: inner.height,
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1]);
    horizontal[1]
}

#[derive(Debug, Clone, Copy)]
enum IconToken {
    Folder,
    Playlist,
    Track,
}

fn icon_glyph(mode: IconMode, token: IconToken) -> &'static str {
    match (mode, token) {
        (IconMode::NerdFont, IconToken::Folder) => "󰉋",
        (IconMode::NerdFont, IconToken::Playlist) => "󰲹",
        (IconMode::NerdFont, IconToken::Track) => "󰎆",
        (IconMode::Ascii, IconToken::Folder) => "[D]",
        (IconMode::Ascii, IconToken::Playlist) => "[P]",
        (IconMode::Ascii, IconToken::Track) => "[*]",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Palette;

    fn sample_state() -> ShellState {
        ShellState::new(ShellSnapshot {
            app_title: "Auric".into(),
            theme_name: "auric-dark".into(),
            color_scheme: "dark".into(),
            icon_mode: IconMode::Ascii,
            icon_fallback: "ascii".into(),
            preferred_terminal_font: "FiraCode Nerd Font Mono".into(),
            mouse_enabled: true,
            artwork_filter: "none".into(),
            pixel_art_enabled: false,
            pixel_art_cell_size: 2,
            roots: vec![ShellListItem {
                id: "r1".into(),
                label: "/music".into(),
                detail: Some("watched".into()),
            }],
            playlists: vec![ShellListItem {
                id: "p1".into(),
                label: "Favorites".into(),
                detail: None,
            }],
            tracks: vec![ShellTrackItem {
                id: "t1".into(),
                title: "Track One".into(),
                artist: "Artist".into(),
                album: "Album".into(),
                path: "/music/Artist/Album/01.flac".into(),
                duration_ms: Some(123_000),
                sample_rate: Some(48_000),
                channels: Some(2),
                bit_depth: Some(24),
            }],
            feature_summary: vec![
                ("metadata".into(), true),
                ("visualizer".into(), false),
                ("mouse".into(), true),
            ],
            status_lines: vec!["ready".into()],
            playback_status: "stopped".to_string(),
            now_playing_path: String::new(),
            now_playing_title: String::new(),
            now_playing_artist: String::new(),
            now_playing_album: String::new(),
            now_playing_artwork: None,
            now_playing_duration_ms: 0,
            now_playing_position_ms: 0,
            volume: 1.0,
            shuffle: false,
            repeat_mode: "off".to_string(),
            queue_length: 0,
            queue_position: 0,
            artists: vec!["Artist".to_string()],
            albums: vec![("Album".to_string(), "Artist".to_string())],
            total_track_count: 1,
            setting_use_theme_bg: false,
            setting_icon_pack: "nerd-font".to_string(),
            setting_pixel_art: false,
            setting_pixel_art_cell_size: 2,
            setting_color_scheme: "dark".to_string(),
            available_themes: vec!["auric-dark".to_string()],
        })
    }

    #[test]
    fn renders_shell_snapshot_to_text() {
        let mut state = sample_state();
        let text = render_once_to_text(&mut state, &Palette::default(), 100, 30).unwrap();
        assert!(text.contains("Library Roots"));
        assert!(text.contains("Library"));
        assert!(text.contains("Favorites"));
        assert!(text.contains("Track One"));
    }

    #[test]
    fn key_navigation_moves_selection() {
        let mut state = sample_state();
        state.focus = FocusPane::Tracks;
        state.snapshot.tracks.push(ShellTrackItem {
            id: "t2".into(),
            title: "Track Two".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            path: "x".into(),
            duration_ms: None,
            sample_rate: None,
            channels: None,
            bit_depth: None,
        });
        state.rebuild_track_filter();
        let _ = state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(state.selected_track, 1);
    }

    #[test]
    fn track_filter_mode_filters_tracks() {
        let mut state = sample_state();
        state.focus = FocusPane::Tracks;
        state.snapshot.tracks.push(ShellTrackItem {
            id: "t2".into(),
            title: "Night Drive".into(),
            artist: "Auric".into(),
            album: "Nocturne".into(),
            path: "/music/Auric/Nocturne/02.flac".into(),
            duration_ms: None,
            sample_rate: None,
            channels: None,
            bit_depth: None,
        });
        state.rebuild_track_filter();

        assert_eq!(
            state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)),
            KeyAction::Continue
        );
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
        let _ = state.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));

        assert_eq!(state.filtered_track_count(), 1);
        assert_eq!(
            state.selected_track_item().map(|t| t.title.as_str()),
            Some("Night Drive")
        );
        assert!(state
            .status_message
            .as_deref()
            .unwrap_or_default()
            .contains("Track filter"));
    }

    #[test]
    fn mouse_click_selects_track_row_with_scroll() {
        let mut state = sample_state();
        state.focus = FocusPane::Tracks;
        for i in 0..8 {
            state.snapshot.tracks.push(ShellTrackItem {
                id: format!("t{}", i + 2),
                title: format!("Track {}", i + 2),
                artist: "Artist".into(),
                album: "Album".into(),
                path: format!("/music/{i}.flac"),
                duration_ms: None,
                sample_rate: None,
                channels: None,
                bit_depth: None,
            });
        }
        state.rebuild_track_filter();
        state.selected_track = 5;
        let areas = RenderAreas {
            roots: PaneArea::bordered(Rect::new(0, 0, 20, 8), 1),
            browse: Rect::new(0, 16, 20, 8),
            playlists: PaneArea::bordered(Rect::new(0, 8, 20, 8), 1),
            tracks: PaneArea::bordered(Rect::new(20, 0, 40, 8), 1),
            track_header: Rect::new(20, 0, 40, 1),
            track_col_offsets: TrackColumnOffsets::default(),
        };
        state.sync_scroll_offsets(&areas);

        let click = MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: areas.tracks.inner.x + 1,
            row: areas.tracks.inner.y,
            modifiers: KeyModifiers::NONE,
        };
        state.handle_mouse(click, &areas);

        assert_eq!(state.focus, FocusPane::Tracks);
        assert_eq!(state.selected_track, state.tracks_scroll);
    }

    #[test]
    fn command_palette_submits_command() {
        let mut state = sample_state();
        assert_eq!(
            state.handle_key(KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE)),
            KeyAction::Continue
        );
        for ch in ['h', 'e', 'l', 'p'] {
            let _ = state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        let action = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(action, KeyAction::CommandSubmitted("help".to_string()));
        assert_eq!(state.input_mode, InputMode::Normal);
        assert!(state.command_palette_input.is_empty());
    }
}
