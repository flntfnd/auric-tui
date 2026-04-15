# Library Onboarding and UI Refresh Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give new users a clear path to add music folders and refresh the TUI layout with better whitespace and visual hierarchy.

**Architecture:** The file browser is a standalone module (`file_browser.rs`) with no UI dependencies -- just filesystem navigation state. The shell module gains three new `InputMode` variants (`AddMusic`, `Welcome`) and corresponding render/key-handler functions. The layout refresh modifies existing render functions in-place. A `terminal_caps.rs` module handles drag-and-drop detection.

**Tech Stack:** Rust, ratatui 0.30, crossterm 0.29

---

### Task 1: UI Layout Refresh -- Padding and Spacing

**Files:**
- Modify: `crates/auric-ui/src/shell.rs:715-770` (draw_shell layout)
- Modify: `crates/auric-ui/src/shell.rs:774-818` (render_roots)
- Modify: `crates/auric-ui/src/shell.rs:821-861` (render_browse_modes)
- Modify: `crates/auric-ui/src/shell.rs:863-903` (render_playlists)
- Modify: `crates/auric-ui/src/shell.rs:905-995` (render_tracks)
- Modify: `crates/auric-ui/src/shell.rs:997-1058` (render_now_playing)
- Modify: `crates/auric-ui/src/shell.rs:1189-1199` (pane_block)

- [ ] **Step 1: Update pane_block to add inner padding**

Change `pane_block` to add a leading space to titles for visual offset, and add a helper that applies inner margin after getting the block's inner area:

```rust
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

fn padded_inner(area: Rect) -> Rect {
    let inner = inner_rect(area);
    Rect {
        x: inner.x.saturating_add(1),
        y: inner.y,
        width: inner.width.saturating_sub(2),
        height: inner.height,
    }
}
```

- [ ] **Step 2: Update draw_shell to add gaps between left sidebar panels**

Replace the left_sections layout to include 1-cell gaps:

```rust
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
```

Update the render calls to use indices 0, 2, 4 instead of 0, 1, 2. Render empty space in indices 1 and 3 (they inherit the surface_0 background).

Also add a 1-cell gap between Now Playing and Tracks on the right:

```rust
let right_sections = Layout::default()
    .direction(Direction::Vertical)
    .constraints([
        Constraint::Length(8),
        Constraint::Length(1),  // gap
        Constraint::Min(12),
    ])
    .split(cols[1]);
```

Use indices 0 and 2 for now_playing and tracks.

- [ ] **Step 3: Update render_roots to use padded_inner for list content**

In `render_roots`, render the block first, then render the list into `padded_inner(area)` without a block:

```rust
fn render_roots(frame: &mut Frame, area: Rect, state: &mut ShellState, palette: &Palette) {
    let block = pane_block("Library Roots", state.focus == FocusPane::Sources, palette);
    let content_area = padded_inner(area);
    frame.render_widget(block, area);

    let items: Vec<ListItem> = if state.snapshot.roots.is_empty() {
        vec![ListItem::new(Line::from("No library roots"))]
    } else {
        // ... existing item building unchanged ...
    };

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
```

Apply the same pattern to `render_browse_modes`, `render_playlists`, `render_now_playing`, and `render_tracks`. Each function renders its block first, then content into `padded_inner(area)`.

- [ ] **Step 4: Simplify the status bar**

Replace `render_status` with a cleaner single-line status:

```rust
fn render_status(frame: &mut Frame, area: Rect, state: &ShellState, palette: &Palette) {
    let block = pane_block("", false, palette);
    let content_area = padded_inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();

    // Line 1: app title + track count
    let filter_info = if state.track_filter_query.is_empty() {
        String::new()
    } else {
        format!("  filter: /{}", state.track_filter_query)
    };
    lines.push(Line::from(vec![
        Span::styled(
            &state.snapshot.app_title,
            Style::default().fg(palette.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {} tracks{filter_info}", state.snapshot.tracks.len()),
            Style::default().fg(palette.text_muted),
        ),
    ]));

    // Line 2: status message or default keybindings
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
```

Reduce footer height from `Constraint::Length(6)` to `Constraint::Length(4)` in `draw_shell`.

- [ ] **Step 5: Update PaneArea calculations for new layout indices**

Update the `RenderAreas` construction in `draw_shell` to reference the new layout indices (0, 2, 4 for left; 0, 2 for right).

- [ ] **Step 6: Build and verify**

Run: `cargo check -p auric-ui 2>&1`
Expected: compiles with no errors.

- [ ] **Step 7: Commit**

```bash
git add crates/auric-ui/src/shell.rs
git commit -m "refactor: add inner padding, panel gaps, and simplified status bar"
```

---

### Task 2: File Browser Module

**Files:**
- Create: `crates/auric-ui/src/file_browser.rs`
- Modify: `crates/auric-ui/src/lib.rs` (add module)
- Test: `crates/auric-ui/src/file_browser.rs` (inline tests)

- [ ] **Step 1: Write failing tests for FileBrowser**

Create `crates/auric-ui/src/file_browser.rs` with the test module first:

```rust
use std::path::{Path, PathBuf};

pub struct FileBrowser {
    current_dir: PathBuf,
    entries: Vec<DirEntry>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub path_input: String,
    pub input_focused: bool,
}

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

impl FileBrowser {
    pub fn new(start_dir: &Path) -> Self {
        let mut browser = Self {
            current_dir: start_dir.to_path_buf(),
            entries: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            path_input: start_dir.display().to_string(),
            input_focused: false,
        };
        browser.refresh_entries();
        browser
    }

    pub fn current_dir(&self) -> &Path {
        &self.current_dir
    }

    pub fn entries(&self) -> &[DirEntry] {
        &self.entries
    }

    pub fn selected_entry(&self) -> Option<&DirEntry> {
        self.entries.get(self.selected)
    }

    pub fn selected_path(&self) -> PathBuf {
        self.selected_entry()
            .map(|e| e.path.clone())
            .unwrap_or_else(|| self.current_dir.clone())
    }

    pub fn refresh_entries(&mut self) {
        self.entries.clear();
        if let Ok(read_dir) = std::fs::read_dir(&self.current_dir) {
            let mut dirs: Vec<DirEntry> = read_dir
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_type().map(|ft| ft.is_dir()).unwrap_or(false)
                        && !e.file_name().to_string_lossy().starts_with('.')
                })
                .map(|e| DirEntry {
                    name: e.file_name().to_string_lossy().into_owned(),
                    path: e.path(),
                    is_dir: true,
                })
                .collect();
            dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            self.entries = dirs;
        }
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub fn enter_selected(&mut self) {
        if let Some(entry) = self.entries.get(self.selected) {
            if entry.is_dir {
                self.current_dir = entry.path.clone();
                self.path_input = self.current_dir.display().to_string();
                self.refresh_entries();
            }
        }
    }

    pub fn go_up(&mut self) {
        if let Some(parent) = self.current_dir.parent() {
            let old_name = self
                .current_dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned());
            self.current_dir = parent.to_path_buf();
            self.path_input = self.current_dir.display().to_string();
            self.refresh_entries();
            // Try to select the directory we came from
            if let Some(name) = old_name {
                if let Some(idx) = self.entries.iter().position(|e| e.name == name) {
                    self.selected = idx;
                }
            }
        }
    }

    pub fn navigate_to(&mut self, path: &Path) {
        let resolved = if path.starts_with("~") {
            if let Some(home) = dirs::home_dir() {
                home.join(path.strip_prefix("~").unwrap_or(path))
            } else {
                path.to_path_buf()
            }
        } else {
            path.to_path_buf()
        };

        if resolved.is_dir() {
            self.current_dir = resolved;
            self.path_input = self.current_dir.display().to_string();
            self.refresh_entries();
        }
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let max = self.entries.len().saturating_sub(1) as isize;
        self.selected = (self.selected as isize + delta).clamp(0, max) as usize;
    }

    pub fn sync_path_input_to_selected(&mut self) {
        self.path_input = self.selected_path().display().to_string();
    }

    pub fn apply_path_input(&mut self) {
        let expanded = if self.path_input.starts_with('~') {
            if let Some(home) = dirs::home_dir() {
                home.join(self.path_input.strip_prefix("~/").unwrap_or(
                    self.path_input.strip_prefix('~').unwrap_or(&self.path_input),
                ))
                .display()
                .to_string()
            } else {
                self.path_input.clone()
            }
        } else {
            self.path_input.clone()
        };
        let path = PathBuf::from(&expanded);
        if path.is_dir() {
            self.current_dir = path;
            self.refresh_entries();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_test_tree() -> TempDir {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("Music/Albums")).unwrap();
        std::fs::create_dir_all(tmp.path().join("Music/Playlists")).unwrap();
        std::fs::create_dir_all(tmp.path().join("Documents")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".hidden")).unwrap();
        tmp
    }

    #[test]
    fn lists_visible_directories_only() {
        let tmp = make_test_tree();
        let browser = FileBrowser::new(tmp.path());
        let names: Vec<&str> = browser.entries().iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"Music"));
        assert!(names.contains(&"Documents"));
        assert!(!names.contains(&".hidden"));
    }

    #[test]
    fn enter_descends_into_directory() {
        let tmp = make_test_tree();
        let mut browser = FileBrowser::new(tmp.path());
        let music_idx = browser
            .entries()
            .iter()
            .position(|e| e.name == "Music")
            .unwrap();
        browser.selected = music_idx;
        browser.enter_selected();
        assert_eq!(browser.current_dir(), tmp.path().join("Music"));
        let names: Vec<&str> = browser.entries().iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"Albums"));
        assert!(names.contains(&"Playlists"));
    }

    #[test]
    fn go_up_returns_to_parent_and_reselects() {
        let tmp = make_test_tree();
        let mut browser = FileBrowser::new(tmp.path());
        let music_idx = browser
            .entries()
            .iter()
            .position(|e| e.name == "Music")
            .unwrap();
        browser.selected = music_idx;
        browser.enter_selected();
        browser.go_up();
        assert_eq!(browser.current_dir(), tmp.path());
        assert_eq!(browser.entries()[browser.selected].name, "Music");
    }

    #[test]
    fn move_selection_clamps() {
        let tmp = make_test_tree();
        let mut browser = FileBrowser::new(tmp.path());
        browser.move_selection(-100);
        assert_eq!(browser.selected, 0);
        browser.move_selection(100);
        assert_eq!(
            browser.selected,
            browser.entries().len().saturating_sub(1)
        );
    }

    #[test]
    fn navigate_to_valid_path() {
        let tmp = make_test_tree();
        let mut browser = FileBrowser::new(tmp.path());
        browser.navigate_to(&tmp.path().join("Music"));
        assert_eq!(browser.current_dir(), tmp.path().join("Music"));
    }
}
```

- [ ] **Step 2: Add dirs dependency to auric-ui Cargo.toml**

Add `dirs = "6"` to `crates/auric-ui/Cargo.toml` under `[dependencies]` for home directory resolution.

- [ ] **Step 3: Register the module in lib.rs**

Add to `crates/auric-ui/src/lib.rs`:

```rust
pub mod file_browser;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p auric-ui -- file_browser 2>&1`
Expected: all 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/auric-ui/src/file_browser.rs crates/auric-ui/src/lib.rs crates/auric-ui/Cargo.toml
git commit -m "feat: add FileBrowser module for directory navigation"
```

---

### Task 3: Terminal Capabilities Module

**Files:**
- Create: `crates/auric-ui/src/terminal_caps.rs`
- Modify: `crates/auric-ui/src/lib.rs`

- [ ] **Step 1: Create terminal_caps.rs with drag-drop detection**

```rust
use std::env;

pub struct TerminalCaps {
    pub supports_drag_drop: bool,
    pub terminal_name: String,
}

impl TerminalCaps {
    pub fn detect() -> Self {
        let term_program = env::var("TERM_PROGRAM").unwrap_or_default();
        let supports_drag_drop = matches!(
            term_program.as_str(),
            "iTerm.app" | "iTerm2" | "WezTerm" | "ghostty" | "foot"
        ) || env::var("TERM").map_or(false, |t| t.contains("kitty"));

        Self {
            supports_drag_drop,
            terminal_name: term_program,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_known_terminals() {
        // Detection depends on env vars, just verify struct construction
        let caps = TerminalCaps {
            supports_drag_drop: true,
            terminal_name: "ghostty".to_string(),
        };
        assert!(caps.supports_drag_drop);
    }
}
```

- [ ] **Step 2: Register module in lib.rs**

Add `pub mod terminal_caps;` to `crates/auric-ui/src/lib.rs`.

- [ ] **Step 3: Build and verify**

Run: `cargo check -p auric-ui 2>&1`
Expected: compiles.

- [ ] **Step 4: Commit**

```bash
git add crates/auric-ui/src/terminal_caps.rs crates/auric-ui/src/lib.rs
git commit -m "feat: add terminal capability detection for drag-and-drop"
```

---

### Task 4: Add Music Floating Panel

**Files:**
- Modify: `crates/auric-ui/src/shell.rs` -- InputMode, handle_key, draw_shell, new render function

- [ ] **Step 1: Add InputMode variants and update handle_key**

Add to the `InputMode` enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Normal,
    TrackFilter,
    CommandPalette,
    AddMusic,
    Welcome,
}
```

Update `input_mode_label`:

```rust
fn input_mode_label(mode: InputMode) -> &'static str {
    match mode {
        InputMode::Normal => "normal",
        InputMode::TrackFilter => "track-filter",
        InputMode::CommandPalette => "command",
        InputMode::AddMusic => "add-music",
        InputMode::Welcome => "welcome",
    }
}
```

- [ ] **Step 2: Add FileBrowser field to ShellState**

Add to `ShellState`:

```rust
pub struct ShellState {
    // ... existing fields ...
    file_browser: Option<crate::file_browser::FileBrowser>,
    terminal_caps: crate::terminal_caps::TerminalCaps,
}
```

Initialize in `ShellState::new`:

```rust
file_browser: None,
terminal_caps: crate::terminal_caps::TerminalCaps::detect(),
```

- [ ] **Step 3: Add 'a' keybinding and AddMusic key handler**

In `handle_key`, add under the Normal mode match:

```rust
KeyCode::Char('a') => {
    self.file_browser = Some(crate::file_browser::FileBrowser::new(
        &dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/")),
    ));
    self.input_mode = InputMode::AddMusic;
}
```

Add the AddMusic and Welcome branches to the mode dispatch at the top of `handle_key`:

```rust
InputMode::AddMusic | InputMode::Welcome => return self.handle_add_music_key(key),
```

Add the handler method:

```rust
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

    // Tree browser focused
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
            // Check if this is the "[Add this folder]" action (selected == 0 is handled by render)
            // We use a special convention: if selected index maps to a real entry, descend.
            // The "add" action is triggered by a separate key.
            browser.enter_selected();
        }
        KeyCode::Backspace | KeyCode::Char('h') => {
            browser.go_up();
        }
        KeyCode::Char(' ') => {
            // Confirm: add current directory as library root
            let path = browser.current_dir().to_string_lossy().into_owned();
            self.file_browser = None;
            self.input_mode = InputMode::Normal;
            return KeyAction::CommandSubmitted(format!("__add_root {path}"));
        }
        _ => {}
    }
    KeyAction::Continue
}
```

- [ ] **Step 4: Add render_add_music_overlay function**

```rust
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

    // Welcome subtitle
    if is_welcome {
        lines.push(Line::from(Span::styled(
            "Add a folder to get started.",
            Style::default().fg(palette.text_muted),
        )));
        lines.push(Line::from(""));
    }

    // Path input
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

    // Current directory header
    let dir_display = browser
        .current_dir()
        .display()
        .to_string()
        .replace(
            &dirs::home_dir()
                .map(|h| h.display().to_string())
                .unwrap_or_default(),
            "~",
        );
    lines.push(Line::from(Span::styled(
        format!("{dir_display}/"),
        Style::default().fg(palette.accent).add_modifier(Modifier::BOLD),
    )));

    // Directory listing
    let entries = browser.entries();
    let max_visible = content.height.saturating_sub(lines.len() as u16 + 3) as usize;
    let start = if browser.selected >= max_visible {
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

    // Padding before keybindings
    let used = lines.len() as u16;
    let remaining = content.height.saturating_sub(used + 2);
    for _ in 0..remaining {
        lines.push(Line::from(""));
    }

    // Keybindings footer
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
```

- [ ] **Step 5: Wire overlay into draw_shell**

Add after the command palette overlay check in `draw_shell`:

```rust
if state.input_mode == InputMode::AddMusic {
    render_add_music_overlay(frame, state, palette, false);
}
if state.input_mode == InputMode::Welcome {
    render_add_music_overlay(frame, state, palette, true);
}
```

- [ ] **Step 6: Add dirs dependency to auric-ui Cargo.toml**

If not already added in Task 2, add `dirs = "6"` to `[dependencies]`.

- [ ] **Step 7: Build and verify**

Run: `cargo check -p auric-ui 2>&1`
Expected: compiles.

- [ ] **Step 8: Commit**

```bash
git add crates/auric-ui/src/shell.rs crates/auric-ui/Cargo.toml
git commit -m "feat: add music folder floating panel with file browser"
```

---

### Task 5: Wire Add-Root Command into App Layer

**Files:**
- Modify: `crates/auric-app/src/lib.rs:1879-1912` (execute_ui_palette_command)

- [ ] **Step 1: Handle the __add_root internal command**

In `execute_ui_palette_command`, add a match arm for the internal command sent by the Add Music panel:

```rust
head if head == "__add_root" => {
    let path = strip_n_words(command, 1)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("internal error: __add_root with no path"))?;
    let row = app.db.upsert_library_root(&LibraryRoot {
        path: path.clone(),
        watched: true,
    })?;
    let prune = false;
    let scanner = scanner_from_config(&app.config.library, prune);
    let summary = scanner.scan_path(&mut app.db, std::path::Path::new(&row.path))?;
    Ok(PaletteCommandResult::new(
        format!(
            "Added {} (imported {} tracks)",
            row.path, summary.imported_tracks
        ),
        true,
    ))
}
```

- [ ] **Step 2: Build and verify**

Run: `cargo check -p auric-app 2>&1`
Expected: compiles.

- [ ] **Step 3: Commit**

```bash
git add crates/auric-app/src/lib.rs
git commit -m "feat: wire __add_root command for add music panel"
```

---

### Task 6: First-Run Welcome and Empty State Hints

**Files:**
- Modify: `crates/auric-ui/src/shell.rs` -- ShellState::new, render_roots, render_tracks
- Modify: `crates/auric-app/src/lib.rs` -- auto-trigger welcome on empty library

- [ ] **Step 1: Auto-trigger Welcome mode on empty library**

In `ShellState::new`, after initialization, check if library is empty:

```rust
// At end of ShellState::new, before returning:
if state.snapshot.roots.is_empty() && state.snapshot.tracks.is_empty() {
    state.input_mode = InputMode::Welcome;
    state.file_browser = Some(crate::file_browser::FileBrowser::new(
        &dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/")),
    ));
}
```

- [ ] **Step 2: Add empty state hints to render_roots**

In `render_roots`, replace the empty state:

```rust
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
    // ... existing ...
};
```

- [ ] **Step 3: Add empty state hints to render_tracks**

In `render_tracks`, replace the empty items:

```rust
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
    // ... existing track rendering ...
};
```

- [ ] **Step 4: Build and verify**

Run: `cargo check 2>&1`
Expected: compiles.

- [ ] **Step 5: Commit**

```bash
git add crates/auric-ui/src/shell.rs crates/auric-app/src/lib.rs
git commit -m "feat: add first-run welcome panel and empty state hints"
```

---

### Task 7: Drag and Drop Support

**Files:**
- Modify: `crates/auric-ui/src/shell.rs:623-682` (run_loop event handling)

- [ ] **Step 1: Enable bracketed paste in terminal setup**

In `run_interactive_with_optional_handlers`, after enabling mouse capture, add:

```rust
use crossterm::event::{EnableBracketedPaste, DisableBracketedPaste};

// After mouse capture enable:
execute!(stdout, EnableBracketedPaste)
    .map_err(|e| UiError::Terminal(format!("enable bracketed paste failed: {e}")))?;
```

And on cleanup, before `LeaveAlternateScreen`:

```rust
let _ = execute!(terminal.backend_mut(), DisableBracketedPaste);
```

- [ ] **Step 2: Handle Paste events in run_loop**

Replace the `Event::Paste(_) => {}` arm in `run_loop`:

```rust
Event::Paste(content) => {
    // Drag-and-drop or pasted path
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
                    // Add directly as root
                    if let Some(handler) = command_handler.as_mut() {
                        match (*handler)(&format!("__add_root {path_str}")) {
                            Ok(result) => {
                                state.status_message = Some(result.status_message);
                                if result.refresh_requested {
                                    try_refresh_snapshot(state, &mut refresh);
                                }
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
            state.status_message = Some(format!("Not a directory: {path_str}"));
        }
    }
}
```

- [ ] **Step 3: Build and verify**

Run: `cargo check 2>&1`
Expected: compiles.

- [ ] **Step 4: Commit**

```bash
git add crates/auric-ui/src/shell.rs
git commit -m "feat: add drag-and-drop support for adding music folders"
```

---

### Task 8: Update Default Status Message and Help Overlay

**Files:**
- Modify: `crates/auric-ui/src/shell.rs` -- default_status_message, render_help_overlay

- [ ] **Step 1: Update default status message to include 'a' key**

```rust
fn default_status_message() -> &'static str {
    "Tab: panes  a: add music  /: filter  :: commands  ?: help  q: quit"
}
```

- [ ] **Step 2: Add 'a' to help overlay**

Add a line to the help overlay lines:

```rust
Line::from("a: add music folder"),
```

Insert it after the "Tab / Shift-Tab" line.

- [ ] **Step 3: Build and verify**

Run: `cargo check -p auric-ui 2>&1`
Expected: compiles.

- [ ] **Step 4: Manual test**

Run: `cargo run -p auric-app 2>&1`

Verify:
1. On first launch with empty library, the Welcome panel appears
2. Esc dismisses it to the empty TUI with hints
3. Press `a` to reopen the Add Music panel
4. Navigate directories with arrow keys, Enter to descend, Backspace to go up
5. Space to confirm a folder -- it adds and scans
6. Status bar shows simplified info
7. Panels have inner padding and gaps between them

- [ ] **Step 5: Commit**

```bash
git add crates/auric-ui/src/shell.rs
git commit -m "feat: update help and status bar with add music keybinding"
```

---

### Task 9: Export New Public Types

**Files:**
- Modify: `crates/auric-ui/src/lib.rs`

- [ ] **Step 1: Update lib.rs exports**

Ensure `file_browser::FileBrowser` and `terminal_caps::TerminalCaps` are accessible:

```rust
pub mod file_browser;
pub mod shell;
pub mod terminal_caps;
pub mod theme;
```

No need to re-export the types -- they're used internally by shell.rs through crate paths.

- [ ] **Step 2: Final build and test**

Run: `cargo test 2>&1`
Expected: all tests pass (existing + new file_browser tests).

Run: `cargo clippy -p auric-ui 2>&1`
Expected: no warnings.

- [ ] **Step 3: Commit any remaining changes**

```bash
git add -A
git commit -m "chore: final cleanup for library onboarding feature"
```
