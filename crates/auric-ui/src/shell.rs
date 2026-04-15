use crate::theme::Palette;
use crate::UiError;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
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
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use std::cmp::min;
use std::io::{self, Stdout};
use std::time::{Duration, Instant};

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
    Tracks,
    Inspector,
}

impl FocusPane {
    pub fn next(self) -> Self {
        match self {
            Self::Sources => Self::Tracks,
            Self::Tracks => Self::Inspector,
            Self::Inspector => Self::Sources,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Sources => Self::Inspector,
            Self::Tracks => Self::Sources,
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
        self.snapshot = snapshot;
        self.selected_root = self
            .selected_root
            .min(self.snapshot.roots.len().saturating_sub(1));
        self.selected_playlist = self
            .selected_playlist
            .min(self.snapshot.playlists.len().saturating_sub(1));
        self.rebuild_track_filter();
    }

    pub fn move_selection(&mut self, delta: isize) {
        match self.focus {
            FocusPane::Sources => {
                self.selected_root =
                    shift_index(self.selected_root, self.snapshot.roots.len(), delta);
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
            FocusPane::Tracks => self.selected_track = 0,
            FocusPane::Inspector => self.selected_playlist = 0,
        }
    }

    pub fn move_to_end(&mut self) {
        match self.focus {
            FocusPane::Sources => self.selected_root = self.snapshot.roots.len().saturating_sub(1),
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
            InputMode::TrackFilter => return self.handle_filter_key(key),
            InputMode::CommandPalette => return self.handle_command_palette_key(key),
            InputMode::AddMusic | InputMode::Welcome => return self.handle_add_music_key(key),
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
            _ => {}
        }
        KeyAction::Continue
    }

    fn handle_mouse(&mut self, mouse: MouseEvent, areas: &RenderAreas) {
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
                self.set_focus_from_point(x, y, areas);
                self.select_from_mouse_click(x, y, areas);
            }
            _ => {}
        }
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
                let path = browser.current_dir().to_string_lossy().into_owned();
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
        self.selected_track = self
            .selected_track
            .min(self.filtered_track_indices.len().saturating_sub(1));
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
    }

    fn set_focus_from_point(&mut self, x: u16, y: u16, areas: &RenderAreas) {
        let point = (x, y).into();
        if areas.roots.outer.contains(point) || areas.browse.contains(point) {
            self.focus = FocusPane::Sources;
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
enum InputMode {
    Normal,
    TrackFilter,
    CommandPalette,
    AddMusic,
    Welcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyAction {
    Continue,
    Quit,
    RefreshRequested,
    CommandSubmitted(String),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteCommandResult {
    pub status_message: String,
    pub refresh_requested: bool,
}

impl PaletteCommandResult {
    pub fn new(status_message: impl Into<String>, refresh_requested: bool) -> Self {
        Self {
            status_message: status_message.into(),
            refresh_requested,
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
}

pub fn run_interactive(
    state: &mut ShellState,
    palette: &Palette,
    options: RunOptions,
) -> Result<(), UiError> {
    run_interactive_with_optional_handlers(state, palette, options, None, None)
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
    run_interactive_with_optional_handlers(state, palette, options, Some(&mut refresh), None)
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
    )
}

fn run_interactive_with_optional_handlers(
    state: &mut ShellState,
    palette: &Palette,
    options: RunOptions,
    refresh: Option<&mut RefreshSnapshotFn<'_>>,
    command_handler: Option<&mut CommandPaletteFn<'_>>,
) -> Result<(), UiError> {
    enable_raw_mode().map_err(|e| UiError::Terminal(format!("enable_raw_mode failed: {e}")))?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)
        .map_err(|e| UiError::Terminal(format!("enter alt screen failed: {e}")))?;
    if options.mouse {
        execute!(stdout, EnableMouseCapture)
            .map_err(|e| UiError::Terminal(format!("enable mouse capture failed: {e}")))?;
    }

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
    );

    if options.mouse {
        let _ = execute!(terminal.backend_mut(), DisableMouseCapture);
    }
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    drop(terminal);
    let _ = disable_raw_mode();

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &mut ShellState,
    palette: &Palette,
    options: RunOptions,
    mut refresh: Option<&mut RefreshSnapshotFn<'_>>,
    mut command_handler: Option<&mut CommandPaletteFn<'_>>,
) -> Result<(), UiError> {
    let mut last_draw = Instant::now();
    let mut last_areas = RenderAreas::default();

    loop {
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
                                    state.status_message = Some(result.status_message);
                                    if result.refresh_requested {
                                        try_refresh_snapshot(state, &mut refresh);
                                    }
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
                },
                Event::Mouse(mouse) => {
                    if options.mouse {
                        state.handle_mouse(mouse, &last_areas);
                    }
                }
                Event::Resize(_, _) => {}
                Event::FocusGained | Event::FocusLost | Event::Paste(_) => {}
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
        Block::default().style(Style::default().bg(palette.surface_0)),
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

    let (_, library_rows_area) = library_panel_inner_areas(right_sections[2]);
    let areas = RenderAreas {
        roots: PaneArea::bordered(left_sections[0], 1),
        browse: left_sections[2],
        playlists: PaneArea::bordered(left_sections[4], 1),
        tracks: PaneArea::from_list_area(right_sections[2], library_rows_area, 1),
    };
    state.sync_scroll_offsets(&areas);

    render_roots(frame, left_sections[0], state, palette);
    render_browse_modes(frame, left_sections[2], state, palette);
    render_playlists(frame, left_sections[4], state, palette);
    render_now_playing(frame, right_sections[0], state, palette);
    render_tracks(frame, right_sections[2], state, palette);
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
    let rows = [
        ("Artists", IconToken::Folder),
        ("Genres", IconToken::Theme),
        ("Albums", IconToken::Playlist),
        ("Songs", IconToken::Track),
    ];
    let mut lines = Vec::with_capacity(rows.len());
    for (idx, (label, icon_token)) in rows.iter().enumerate() {
        let selected = idx == 3;
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", icon_glyph(state.snapshot.icon_mode, *icon_token)),
                Style::default().fg(if selected {
                    palette.focus
                } else {
                    palette.text_muted
                }),
            ),
            Span::styled(
                *label,
                Style::default()
                    .fg(if selected {
                        palette.text
                    } else {
                        palette.text_muted
                    })
                    .add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
        ]));
    }

    let block = pane_block("Browse Library", false, palette);
    let content_area = padded_inner(area);
    frame.render_widget(block, area);

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: true });
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

fn render_tracks(frame: &mut Frame, area: Rect, state: &mut ShellState, palette: &Palette) {
    let title = if state.track_filter_query.is_empty() {
        format!("Library ({})", state.filtered_track_count())
    } else {
        format!(
            "Library ({}/{}) /{}",
            state.filtered_track_count(),
            state.snapshot.tracks.len(),
            state.track_filter_query
        )
    };
    let outer_block = pane_block(&title, state.focus == FocusPane::Tracks, palette);
    let inner = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let (header_area, rows_area) = library_panel_inner_areas(area);

    let header = Line::from(vec![
        Span::styled(pad_cell("Art", 4), Style::default().fg(palette.text_muted)),
        Span::styled(
            pad_cell("Song Title", 22),
            Style::default().fg(palette.text_muted),
        ),
        Span::styled(pad_cell("Time", 7), Style::default().fg(palette.text_muted)),
        Span::styled(
            pad_cell("Artist", 16),
            Style::default().fg(palette.text_muted),
        ),
        Span::styled(
            pad_cell("Genre", 10),
            Style::default().fg(palette.text_muted),
        ),
        Span::styled(pad_cell("Fav", 6), Style::default().fg(palette.text_muted)),
        Span::styled("Quality", Style::default().fg(palette.text_muted)),
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
                    "{} {}{}{}{}{}{}",
                    pad_cell(icon, 3),
                    pad_cell(&truncate_text(&t.title, 21), 22),
                    pad_cell(&format_duration_short(t.duration_ms), 7),
                    pad_cell(&truncate_text(&t.artist, 15), 16),
                    pad_cell("-", 10),
                    pad_cell("☆", 6),
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
}

fn render_now_playing(frame: &mut Frame, area: Rect, state: &ShellState, palette: &Palette) {
    let selected = state.selected_track_item();
    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!(
                "{} ",
                icon_glyph(state.snapshot.icon_mode, IconToken::NowPlaying)
            ),
            Style::default().fg(palette.progress_fill),
        ),
        Span::styled("Now Playing / Controls", Style::default().fg(palette.text)),
    ])];

    if let Some(track) = selected {
        lines.push(Line::from(Span::styled(
            format!(
                "{}  {}  {}",
                track.title,
                icon_glyph(state.snapshot.icon_mode, IconToken::NowPlaying),
                track.artist
            ),
            Style::default().add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "[Space] Play/Pause   [N] Next   [B] Prev   [/] Filter   [:] Commands",
            Style::default().fg(palette.text_muted),
        )));
        lines.push(Line::from(Span::styled(
            format!(
                "Album: {}   Quality: {}   Time: {}",
                track.album,
                format_tech_compact(track.sample_rate, track.bit_depth, track.channels),
                format_duration_short(track.duration_ms)
            ),
            Style::default().fg(palette.text_muted),
        )));
        lines.push(Line::from(Span::styled(
            fake_progress_bar(area.width.saturating_sub(6), 0.32),
            Style::default().fg(palette.progress_fill),
        )));
        lines.push(Line::from(Span::styled(
            fake_visualizer_line(area.width.saturating_sub(6)),
            Style::default().fg(palette.accent_2),
        )));
        if state.snapshot.pixel_art_enabled {
            lines.push(Line::from(Span::styled(
                format!(
                    "pixel-art on (cell {}px)",
                    state.snapshot.pixel_art_cell_size
                ),
                Style::default().fg(palette.warning),
            )));
        }
    } else {
        lines.push(Line::from("No track selected"));
    }

    let block = pane_block("Now Playing", false, palette);
    let content_area = padded_inner(area);
    frame.render_widget(block, area);

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: true });
    frame.render_widget(paragraph, content_area);
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
    lines.push(Line::from(vec![
        Span::styled(
            state.snapshot.app_title.clone(),
            Style::default().fg(palette.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {} tracks{filter_info}", state.snapshot.tracks.len()),
            Style::default().fg(palette.text_muted),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        state
            .status_message
            .clone()
            .unwrap_or_else(|| default_status_message().to_string()),
        Style::default().fg(palette.text_muted),
    )));
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: true });
    frame.render_widget(paragraph, content_area);
}

fn render_help_overlay(frame: &mut Frame, palette: &Palette) {
    let area = centered_rect(65, 60, frame.area());
    frame.render_widget(Clear, area);
    let lines = vec![
        Line::from(Span::styled(
            "Auric TUI Preview Help",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("Tab / Shift-Tab: switch pane focus"),
        Line::from("j/k or arrows: move selection"),
        Line::from("PgUp/PgDn: page movement"),
        Line::from("g / G: first / last"),
        Line::from("/: track filter mode (type to filter, Enter/Esc close)"),
        Line::from(": or Ctrl-P: command palette"),
        Line::from("Mouse click: focus pane + select row"),
        Line::from("Mouse wheel: scroll selected pane"),
        Line::from("q or Ctrl-C: quit"),
        Line::from("r: refresh preview snapshot"),
        Line::from("?: toggle this help"),
    ];
    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Help")
                .border_style(Style::default().fg(palette.focus))
                .style(Style::default().bg(palette.surface_1).fg(palette.text)),
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
                state.command_palette_input.clone(),
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
                .style(Style::default().bg(palette.surface_1).fg(palette.text)),
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
        .style(Style::default().bg(palette.surface_1).fg(palette.text));
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
    Block::default()
        .borders(Borders::ALL)
        .title(format!(" {title} "))
        .border_style(if focused {
            Style::default().fg(palette.focus)
        } else {
            Style::default().fg(palette.border)
        })
        .style(Style::default().bg(palette.surface_1).fg(palette.text))
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
    let sr_khz = sample_rate.unwrap_or_default() / 1000;
    let bd = bit_depth.unwrap_or_default();
    let ch = channels.unwrap_or_default();
    if sr_khz > 0 && bd > 0 {
        format!("{bd}b/{sr_khz}k {ch}ch")
    } else {
        format!("{}Hz {}ch {}bit", sample_rate.unwrap_or(0), ch, bd)
    }
}

fn fake_progress_bar(width: u16, progress: f32) -> String {
    let usable = usize::from(width.max(10)).saturating_sub(2);
    let filled = ((usable as f32) * progress.clamp(0.0, 1.0)).round() as usize;
    let mut body = String::with_capacity(usable);
    for idx in 0..usable {
        body.push(if idx < filled { '█' } else { '░' });
    }
    format!("[{body}]")
}

fn fake_visualizer_line(width: u16) -> String {
    let usable = usize::from(width.max(12)).saturating_sub(2);
    let pattern = [1usize, 4, 2, 6, 3, 7, 2, 5, 1, 4, 2, 3];
    let mut out = String::with_capacity(usable);
    for i in 0..usable {
        let amp = pattern[i % pattern.len()];
        out.push(match amp {
            0 | 1 => '▁',
            2 => '▂',
            3 => '▃',
            4 => '▄',
            5 => '▅',
            6 => '▆',
            _ => '▇',
        });
    }
    out
}

fn default_status_message() -> &'static str {
    "Tab: switch pane | /: track filter | : cmd palette | r: refresh | q: quit | ?: help"
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
                let total_tracks = snapshot.tracks.len();
                state.replace_snapshot(snapshot);
                state.status_message = Some(format!(
                    "Refreshed preview snapshot (tracks loaded: {total_tracks})"
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

fn input_mode_label(mode: InputMode) -> &'static str {
    match mode {
        InputMode::Normal => "normal",
        InputMode::TrackFilter => "track-filter",
        InputMode::CommandPalette => "command",
        InputMode::AddMusic => "add-music",
        InputMode::Welcome => "welcome",
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
    NowPlaying,
    Theme,
}

fn icon_glyph(mode: IconMode, token: IconToken) -> &'static str {
    match (mode, token) {
        (IconMode::NerdFont, IconToken::Folder) => "󰉋",
        (IconMode::NerdFont, IconToken::Playlist) => "󰲹",
        (IconMode::NerdFont, IconToken::Track) => "󰎆",
        (IconMode::NerdFont, IconToken::NowPlaying) => "󰎄",
        (IconMode::NerdFont, IconToken::Theme) => "󰔎",
        (IconMode::Ascii, IconToken::Folder) => "[D]",
        (IconMode::Ascii, IconToken::Playlist) => "[P]",
        (IconMode::Ascii, IconToken::Track) => "[*]",
        (IconMode::Ascii, IconToken::NowPlaying) => ">",
        (IconMode::Ascii, IconToken::Theme) => "[#]",
    }
}

fn icon_mode_label(mode: IconMode) -> &'static str {
    match mode {
        IconMode::NerdFont => "nerd-font",
        IconMode::Ascii => "ascii",
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
        })
    }

    #[test]
    fn renders_shell_snapshot_to_text() {
        let mut state = sample_state();
        let text = render_once_to_text(&mut state, &Palette::default(), 100, 30).unwrap();
        assert!(text.contains("Watched Directories"));
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
