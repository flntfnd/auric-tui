# Rich TUI Experience Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transform auric from a basic TUI into a rich, polished music player with album art, animated transitions, spectrum visualizer, miller-column library browser, interactive seek bar, and a proper modal/popup system.

**Architecture:** Split the monolithic shell.rs (2400+ lines) into focused modules. Add new crate dependencies for album art (ratatui-image), animations (tachyonfx), and better widgets. Build a library browse system backed by new DB queries. Add an FFT-based visualizer fed from the audio player's sample buffer.

**Tech Stack:** Rust, ratatui 0.30, ratatui-image, tachyonfx, rustfft (already available via auric-drift)

---

## File Structure

### New files
- `crates/auric-ui/src/browse.rs` -- BrowseState, BrowseMode, miller column logic
- `crates/auric-ui/src/visualizer.rs` -- spectrum analyzer widget, FFT consumer
- `crates/auric-ui/src/seekbar.rs` -- interactive seek bar widget with mouse click/drag
- `crates/auric-ui/src/artwork.rs` -- album art loading and rendering via ratatui-image
- `crates/auric-ui/src/modal.rs` -- generic modal/popup system

### Modified files
- `crates/auric-ui/Cargo.toml` -- add ratatui-image, tachyonfx dependencies
- `crates/auric-ui/src/lib.rs` -- register new modules, export types
- `crates/auric-ui/src/shell.rs` -- refactor rendering, integrate new modules, focus system polish
- `crates/auric-ui/src/theme.rs` -- add focus border styles
- `crates/auric-library/src/db.rs` -- add queries for distinct artists, albums, genres, filtered track lists
- `crates/auric-app/src/lib.rs` -- wire browse state into snapshot, provide artwork data
- `crates/auric-audio/src/player.rs` -- expose sample buffer for visualizer

---

### Task 1: Add DB Queries for Library Browsing

**Files:**
- Modify: `crates/auric-library/src/db.rs`

- [ ] **Step 1: Add distinct_artists query**

```rust
pub fn distinct_artists(&self) -> Result<Vec<String>, DbError> {
    let mut stmt = self.conn.prepare(
        "SELECT DISTINCT artist FROM tracks WHERE artist IS NOT NULL AND artist != '' ORDER BY artist COLLATE NOCASE ASC",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
}
```

- [ ] **Step 2: Add distinct_albums query**

```rust
pub fn distinct_albums(&self) -> Result<Vec<(String, String)>, DbError> {
    let mut stmt = self.conn.prepare(
        "SELECT DISTINCT album, artist FROM tracks WHERE album IS NOT NULL AND album != '' ORDER BY album COLLATE NOCASE ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1).unwrap_or_default()))
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
}
```

- [ ] **Step 3: Add distinct_genres query (using path-based inference)**

Since we don't have a genre column, skip this for now and add a placeholder that returns an empty vec. Genre support can be added when tag parsing includes genre.

```rust
pub fn distinct_genres(&self) -> Result<Vec<String>, DbError> {
    Ok(Vec::new())
}
```

- [ ] **Step 4: Add list_tracks_by_artist and list_tracks_by_album**

```rust
pub fn list_tracks_by_artist(&self, artist: &str) -> Result<Vec<TrackRow>, DbError> {
    let mut stmt = self.conn.prepare(
        "SELECT id, path, title, artist, album, duration_ms, sample_rate, channels, bit_depth, file_mtime_ms, added_at_ms, updated_at_ms
         FROM tracks WHERE artist = ?1 ORDER BY album COLLATE NOCASE ASC, path ASC",
    )?;
    let rows = stmt.query_map(params![artist], read_track_row)?;
    collect_rows(rows)
}

pub fn list_tracks_by_album(&self, album: &str) -> Result<Vec<TrackRow>, DbError> {
    let mut stmt = self.conn.prepare(
        "SELECT id, path, title, artist, album, duration_ms, sample_rate, channels, bit_depth, file_mtime_ms, added_at_ms, updated_at_ms
         FROM tracks WHERE album = ?1 ORDER BY path ASC",
    )?;
    let rows = stmt.query_map(params![album], read_track_row)?;
    collect_rows(rows)
}
```

- [ ] **Step 5: Add get_artwork_data to retrieve artwork blob for a track**

```rust
pub fn get_artwork_data_for_track(&self, track_path: &str) -> Result<Option<Vec<u8>>, DbError> {
    self.conn
        .query_row(
            "SELECT aa.data
             FROM tracks t
             JOIN track_artwork ta ON ta.track_id = t.id
             JOIN artwork_assets aa ON aa.id = ta.artwork_id
             WHERE t.path = ?1
             LIMIT 1",
            params![track_path],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()
        .map_err(DbError::from)
}
```

- [ ] **Step 6: Build and test**

Run: `cargo test -p auric-library 2>&1`
Run: `cargo check 2>&1`

- [ ] **Step 7: Commit**

```bash
git add crates/auric-library/src/db.rs
git commit -m "feat: add library browse queries for artists, albums, and artwork data"
```

---

### Task 2: Focus System Polish

**Files:**
- Modify: `crates/auric-ui/src/theme.rs`
- Modify: `crates/auric-ui/src/shell.rs`

- [ ] **Step 1: Add border style variants to Palette**

In `theme.rs`, add to `Palette`:

```rust
pub border_focused: Color,   // bright border for focused pane
pub border_unfocused: Color, // dim border for unfocused pane
```

Initialize `border_focused` from `palette.focus` and `border_unfocused` from `palette.border` in Default impl. Add to `from_theme` mappings.

- [ ] **Step 2: Update pane_block to use rounded borders and bolder focus styling**

In `shell.rs`, change `pane_block`:

```rust
fn pane_block<'a>(title: &'a str, focused: bool, palette: &Palette) -> Block<'a> {
    let (border_style, border_type) = if focused {
        (
            Style::default().fg(palette.border_focused),
            ratatui::widgets::BorderType::Rounded,
        )
    } else {
        (
            Style::default().fg(palette.border_unfocused),
            ratatui::widgets::BorderType::Rounded,
        )
    };
    let title_style = if focused {
        Style::default().fg(palette.border_focused).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(palette.text_muted)
    };
    Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .title(Span::styled(format!(" {title} "), title_style))
        .border_style(border_style)
        .style(Style::default().bg(palette.bg_panel()).fg(palette.text))
}
```

- [ ] **Step 3: Build and verify**

Run: `cargo check -p auric-ui 2>&1`

- [ ] **Step 4: Commit**

```bash
git add crates/auric-ui/src/theme.rs crates/auric-ui/src/shell.rs
git commit -m "feat: polished focus system with rounded borders and bold titles"
```

---

### Task 3: Browse Library Module (Miller Columns)

**Files:**
- Create: `crates/auric-ui/src/browse.rs`
- Modify: `crates/auric-ui/src/lib.rs`
- Modify: `crates/auric-ui/src/shell.rs`
- Modify: `crates/auric-app/src/lib.rs`

- [ ] **Step 1: Create browse.rs with BrowseState**

```rust
// crates/auric-ui/src/browse.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowseMode {
    Songs,
    Artists,
    Albums,
}

impl BrowseMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Songs => "Songs",
            Self::Artists => "Artists",
            Self::Albums => "Albums",
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::Songs, Self::Artists, Self::Albums]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowseColumn {
    Categories,  // left: browse mode selector (Artists/Albums/Songs)
    Items,       // middle: items in selected category (artist names, album names)
    Tracks,      // right: tracks for selected item
}

#[derive(Debug, Clone)]
pub struct BrowseState {
    pub mode: BrowseMode,
    pub mode_index: usize,
    pub column: BrowseColumn,
    pub items: Vec<String>,           // artist names or album names
    pub item_index: usize,
    pub item_scroll: usize,
    pub selected_item: Option<String>, // currently highlighted item for track filtering
}

impl BrowseState {
    pub fn new() -> Self {
        Self {
            mode: BrowseMode::Songs,
            mode_index: 0,
            column: BrowseColumn::Categories,
            items: Vec::new(),
            item_index: 0,
            item_scroll: 0,
            selected_item: None,
        }
    }

    pub fn set_mode(&mut self, mode: BrowseMode) {
        self.mode = mode;
        self.mode_index = BrowseMode::all().iter().position(|m| *m == mode).unwrap_or(0);
        self.item_index = 0;
        self.item_scroll = 0;
        self.selected_item = None;
    }

    pub fn set_items(&mut self, items: Vec<String>) {
        self.items = items;
        self.item_index = 0;
        self.item_scroll = 0;
        self.update_selected_item();
    }

    pub fn move_item_selection(&mut self, delta: isize) {
        if self.items.is_empty() {
            return;
        }
        let max = self.items.len().saturating_sub(1) as isize;
        self.item_index = (self.item_index as isize + delta).clamp(0, max) as usize;
        self.update_selected_item();
    }

    fn update_selected_item(&mut self) {
        self.selected_item = self.items.get(self.item_index).cloned();
    }

    /// Move focus between columns: left goes to Categories, right goes deeper
    pub fn move_column_right(&mut self) {
        self.column = match self.column {
            BrowseColumn::Categories => BrowseColumn::Items,
            BrowseColumn::Items => BrowseColumn::Tracks,
            BrowseColumn::Tracks => BrowseColumn::Tracks,
        };
    }

    pub fn move_column_left(&mut self) {
        self.column = match self.column {
            BrowseColumn::Categories => BrowseColumn::Categories,
            BrowseColumn::Items => BrowseColumn::Categories,
            BrowseColumn::Tracks => BrowseColumn::Items,
        };
    }
}
```

- [ ] **Step 2: Register module in lib.rs**

Add `pub mod browse;` to `crates/auric-ui/src/lib.rs`.

- [ ] **Step 3: Add BrowseState to ShellState**

In `shell.rs`, add `browse: crate::browse::BrowseState` field to `ShellState`. Initialize with `browse: crate::browse::BrowseState::new()`.

- [ ] **Step 4: Update FocusPane to include Browse**

Change `FocusPane`:
```rust
pub enum FocusPane {
    Sources,
    Browse,
    Tracks,
    Inspector,
}
```

Update `next()` and `prev()` to cycle through all four. Update all match arms that handle `FocusPane` throughout shell.rs.

- [ ] **Step 5: Add ShellSnapshot fields for browse data**

```rust
pub artists: Vec<String>,
pub albums: Vec<(String, String)>,  // (album, artist)
```

Populate in `build_shell_snapshot` in lib.rs:
```rust
artists: app.db.distinct_artists().unwrap_or_default(),
albums: app.db.distinct_albums().unwrap_or_default(),
```

- [ ] **Step 6: Rewrite render_browse_modes as miller columns**

Replace the static browse section with a three-column layout:
- Left column: mode selector (Songs / Artists / Albums) -- navigate with j/k when browse pane focused
- Middle column: items for selected mode (artist names or album names) -- shows when mode is Artists or Albums
- When mode is Songs, the Browse pane acts as a simple mode indicator and Tracks pane shows all tracks

When focused on Browse pane with Artists/Albums mode:
- `j/k`: navigate items in the active column
- `l/Enter`: move to next column / expand
- `h/Backspace`: move to previous column
- Selecting an artist/album filters the Tracks pane to show only matching tracks

- [ ] **Step 7: Wire browse filtering into track display**

When `browse.mode` is Artists and `browse.selected_item` is Some, filter the displayed tracks. Add a `CommandSubmitted("__browse_filter artist <name>")` or similar internal command, OR handle it directly in the UI by filtering `snapshot.tracks` based on `browse.selected_item`.

Simpler approach: add a `browse_filter_artist: Option<String>` and `browse_filter_album: Option<String>` to ShellState. When set, `rebuild_track_filter` also filters by these fields. When browse selection changes, update these fields and rebuild.

- [ ] **Step 8: Build, test, commit**

Run: `cargo check 2>&1`
Run: `cargo test 2>&1`

```bash
git add crates/auric-ui/src/browse.rs crates/auric-ui/src/shell.rs crates/auric-ui/src/lib.rs crates/auric-app/src/lib.rs
git commit -m "feat: miller-column library browser with artist/album filtering"
```

---

### Task 4: Interactive Seek Bar

**Files:**
- Create: `crates/auric-ui/src/seekbar.rs`
- Modify: `crates/auric-ui/src/shell.rs`

- [ ] **Step 1: Create seekbar.rs**

A stateless widget that renders a seek bar and provides a helper to map click position to progress:

```rust
use ratatui::prelude::*;
use ratatui::widgets::Widget;
use crate::theme::Palette;

pub struct SeekBar<'a> {
    pub progress: f32,
    pub elapsed: &'a str,
    pub remaining: &'a str,
    pub palette: &'a Palette,
}

impl<'a> Widget for SeekBar<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 10 || area.height < 1 {
            return;
        }

        // Time labels on left and right
        let elapsed_width = self.elapsed.len() as u16;
        let remaining_width = self.remaining.len() as u16;
        let bar_start = area.x + elapsed_width + 1;
        let bar_end = area.x + area.width - remaining_width - 1;
        let bar_width = bar_end.saturating_sub(bar_start);

        // Elapsed label
        buf.set_string(area.x, area.y, self.elapsed, Style::default().fg(self.palette.text_muted));

        // Progress bar
        let filled = ((bar_width as f32) * self.progress.clamp(0.0, 1.0)).round() as u16;
        for x in bar_start..bar_end {
            let ch = if x - bar_start < filled { '━' } else { '─' };
            let color = if x - bar_start < filled {
                self.palette.progress_fill
            } else {
                self.palette.border
            };
            buf.set_string(x, area.y, ch.to_string(), Style::default().fg(color));
        }

        // Playhead
        if filled > 0 && bar_start + filled < bar_end {
            buf.set_string(bar_start + filled, area.y, "●", Style::default().fg(self.palette.accent));
        }

        // Remaining label
        buf.set_string(bar_end + 1, area.y, self.remaining, Style::default().fg(self.palette.text_muted));
    }
}

/// Map a mouse click x-coordinate to a progress value (0.0-1.0)
pub fn click_to_progress(click_x: u16, area: Rect, elapsed_width: u16, remaining_width: u16) -> Option<f32> {
    let bar_start = area.x + elapsed_width + 1;
    let bar_end = area.x + area.width - remaining_width - 1;
    if click_x >= bar_start && click_x < bar_end {
        let bar_width = bar_end - bar_start;
        if bar_width > 0 {
            return Some((click_x - bar_start) as f32 / bar_width as f32);
        }
    }
    None
}
```

- [ ] **Step 2: Register module, integrate into render_now_playing**

Replace the text-based progress bar in `render_now_playing` with the `SeekBar` widget. Store the seek bar area in `RenderAreas` so mouse clicks can be mapped.

- [ ] **Step 3: Handle mouse clicks on seek bar**

In `handle_mouse`, detect clicks in the seek bar area and emit `KeyAction::Playback(PlaybackAction::Seek { position_ms })` with the calculated position.

Add `Seek { position_ms: u64 }` variant to `PlaybackAction`. Handle it in `handle_tui_playback_action` by calling `app.player.seek(position_ms)`.

- [ ] **Step 4: Build, test, commit**

```bash
git add crates/auric-ui/src/seekbar.rs crates/auric-ui/src/shell.rs crates/auric-ui/src/lib.rs crates/auric-app/src/lib.rs
git commit -m "feat: interactive seek bar with mouse click support"
```

---

### Task 5: Album Art Display

**Files:**
- Create: `crates/auric-ui/src/artwork.rs`
- Modify: `crates/auric-ui/Cargo.toml`
- Modify: `crates/auric-ui/src/shell.rs`
- Modify: `crates/auric-app/src/lib.rs`

- [ ] **Step 1: Add ratatui-image dependency**

In `crates/auric-ui/Cargo.toml`:
```toml
ratatui-image = "3"
image = "0.25"
```

Add to workspace Cargo.toml under `[workspace.dependencies]`:
```toml
ratatui-image = "3"
image = "0.25"
```

- [ ] **Step 2: Create artwork.rs**

```rust
use image::DynamicImage;
use ratatui::prelude::*;
use ratatui_image::{picker::Picker, protocol::StatefulProtocol, StatefulImage};

pub struct ArtworkState {
    picker: Option<Picker>,
    current_image: Option<Box<dyn StatefulProtocol>>,
    current_track_path: String,
}

impl ArtworkState {
    pub fn new() -> Self {
        let picker = Picker::from_query_stdio().ok();
        Self {
            picker,
            current_image: None,
            current_track_path: String::new(),
        }
    }

    pub fn update(&mut self, track_path: &str, image_data: Option<&[u8]>) {
        if track_path == self.current_track_path {
            return; // already showing this track's art
        }
        self.current_track_path = track_path.to_string();

        let Some(picker) = &mut self.picker else {
            self.current_image = None;
            return;
        };

        self.current_image = image_data.and_then(|data| {
            let img = image::load_from_memory(data).ok()?;
            Some(picker.new_resize_protocol(img))
        });
    }

    pub fn has_image(&self) -> bool {
        self.current_image.is_some()
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        if let Some(protocol) = &mut self.current_image {
            let image = StatefulImage::default();
            image.render(area, buf, protocol);
        }
    }
}
```

- [ ] **Step 3: Add ArtworkState to ShellState, add artwork_data to ShellSnapshot**

In `ShellSnapshot`, add:
```rust
pub now_playing_artwork: Option<Vec<u8>>,
```

Populate in `build_shell_snapshot`:
```rust
now_playing_artwork: app.playback_state.current_entry()
    .and_then(|e| app.db.get_artwork_data_for_track(&e.path).ok().flatten()),
```

In `ShellState`, add `artwork: crate::artwork::ArtworkState`. Initialize with `artwork: crate::artwork::ArtworkState::new()`.

- [ ] **Step 4: Integrate into render_now_playing**

Split the Now Playing area: if artwork is available, use ~30% of the width for art, rest for track info. Call `state.artwork.update(track_path, artwork_data)` then `state.artwork.render(art_area, buf)`.

- [ ] **Step 5: Build, test, commit**

```bash
git add crates/auric-ui/Cargo.toml crates/auric-ui/src/artwork.rs crates/auric-ui/src/shell.rs crates/auric-ui/src/lib.rs crates/auric-app/src/lib.rs Cargo.toml
git commit -m "feat: album art display with Kitty/Sixel/halfblocks auto-detection"
```

---

### Task 6: Spectrum Visualizer

**Files:**
- Create: `crates/auric-ui/src/visualizer.rs`
- Modify: `crates/auric-audio/src/player.rs`
- Modify: `crates/auric-ui/src/shell.rs`

- [ ] **Step 1: Expose sample buffer from player for visualization**

In `player.rs`, add a method to `PlayerHandle` that returns a snapshot of recent samples:

```rust
pub fn peek_samples(&self, count: usize) -> Vec<f32> {
    // Send a request to the player thread to snapshot the buffer
    // For simplicity, share the buffer directly via Arc<Mutex<VecDeque<f32>>>
}
```

Simpler approach: add a separate `Arc<Mutex<Vec<f32>>>` visualization buffer that the decode loop copies recent samples into. The UI reads this without blocking playback.

- [ ] **Step 2: Create visualizer.rs**

```rust
use ratatui::prelude::*;
use ratatui::widgets::Widget;
use crate::theme::Palette;

const NUM_BANDS: usize = 32;
const BAR_CHARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

pub struct SpectrumWidget<'a> {
    pub bands: &'a [f32; NUM_BANDS],
    pub palette: &'a Palette,
}

impl<'a> Widget for SpectrumWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 4 || area.height < 1 {
            return;
        }
        let bar_width = (area.width as usize) / NUM_BANDS;
        if bar_width == 0 {
            return;
        }

        for (i, &magnitude) in self.bands.iter().enumerate() {
            let x = area.x + (i * bar_width) as u16;
            if x >= area.x + area.width {
                break;
            }
            let height = (magnitude.clamp(0.0, 1.0) * area.height as f32) as u16;
            for row in 0..area.height {
                let y = area.y + area.height - 1 - row;
                let ch = if row < height {
                    BAR_CHARS[7] // full block
                } else if row == height {
                    let frac = (magnitude * area.height as f32) - height as f32;
                    BAR_CHARS[(frac * 7.0).clamp(0.0, 7.0) as usize]
                } else {
                    ' '
                };
                let color = if i < NUM_BANDS / 3 {
                    self.palette.visualizer_low
                } else if i < 2 * NUM_BANDS / 3 {
                    self.palette.visualizer_mid
                } else {
                    self.palette.visualizer_high
                };
                for dx in 0..bar_width as u16 {
                    if x + dx < area.x + area.width {
                        buf.set_string(x + dx, y, ch.to_string(), Style::default().fg(color));
                    }
                }
            }
        }
    }
}

/// Simple FFT-based spectrum analysis
pub fn analyze_spectrum(samples: &[f32], num_bands: usize) -> Vec<f32> {
    use std::f32::consts::PI;

    if samples.is_empty() {
        return vec![0.0; num_bands];
    }

    // Simple DFT for the bands we need (cheaper than full FFT for 32 bands)
    let n = samples.len().min(2048);
    let mut bands = vec![0.0f32; num_bands];

    for (band_idx, magnitude) in bands.iter_mut().enumerate() {
        // Log-scale frequency mapping
        let freq_lo = 20.0 * (10000.0f32 / 20.0).powf(band_idx as f32 / num_bands as f32);
        let freq_hi = 20.0 * (10000.0f32 / 20.0).powf((band_idx + 1) as f32 / num_bands as f32);
        let bin_lo = (freq_lo / 44100.0 * n as f32) as usize;
        let bin_hi = ((freq_hi / 44100.0 * n as f32) as usize).min(n / 2);

        let mut sum = 0.0f32;
        let mut count = 0;
        for k in bin_lo..=bin_hi {
            let mut re = 0.0f32;
            let mut im = 0.0f32;
            for (i, &s) in samples[..n].iter().enumerate() {
                let angle = 2.0 * PI * k as f32 * i as f32 / n as f32;
                re += s * angle.cos();
                im -= s * angle.sin();
            }
            sum += (re * re + im * im).sqrt();
            count += 1;
        }
        if count > 0 {
            *magnitude = (sum / count as f32 / n as f32 * 4.0).clamp(0.0, 1.0);
        }
    }

    bands
}
```

- [ ] **Step 3: Add visualization buffer to player**

In `player.rs`, add a shared `Arc<Mutex<Vec<f32>>>` that the decode loop writes recent samples to. Add `pub fn peek_visualization_samples(&self, count: usize) -> Vec<f32>` to PlayerHandle.

- [ ] **Step 4: Integrate visualizer into Now Playing panel**

Render the spectrum below the seek bar when playing. Call `analyze_spectrum` on the visualization samples each frame.

- [ ] **Step 5: Build, test, commit**

```bash
git add crates/auric-ui/src/visualizer.rs crates/auric-audio/src/player.rs crates/auric-ui/src/shell.rs crates/auric-ui/src/lib.rs
git commit -m "feat: real-time spectrum visualizer from audio samples"
```

---

### Task 7: Modal/Popup System

**Files:**
- Create: `crates/auric-ui/src/modal.rs`
- Modify: `crates/auric-ui/src/shell.rs`

- [ ] **Step 1: Create modal.rs with generic popup rendering**

```rust
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use crate::theme::Palette;

pub struct Modal<'a> {
    pub title: &'a str,
    pub lines: Vec<Line<'a>>,
    pub width_percent: u16,
    pub height_percent: u16,
    pub palette: &'a Palette,
}

impl<'a> Modal<'a> {
    pub fn render(&self, frame: &mut Frame) {
        let area = centered_rect(self.width_percent, self.height_percent, frame.area());
        frame.render_widget(Clear, area);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .title(format!(" {} ", self.title))
            .border_style(Style::default().fg(self.palette.focus))
            .style(Style::default().bg(self.palette.bg_panel()).fg(self.palette.text));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let content = Rect {
            x: inner.x.saturating_add(1),
            y: inner.y,
            width: inner.width.saturating_sub(2),
            height: inner.height,
        };
        let paragraph = Paragraph::new(self.lines.clone()).wrap(Wrap { trim: true });
        frame.render_widget(paragraph, content);
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
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
```

- [ ] **Step 2: Refactor existing overlays to use Modal**

Convert `render_help_overlay`, `render_command_palette_overlay`, and `render_add_music_overlay` to use the `Modal` struct where possible. This is a cleanup/refactor, not new functionality.

- [ ] **Step 3: Add track info popup**

Add `InputMode::TrackInfo` that shows full metadata for the selected track when pressing `i`. Uses Modal to render.

- [ ] **Step 4: Build, test, commit**

```bash
git add crates/auric-ui/src/modal.rs crates/auric-ui/src/shell.rs crates/auric-ui/src/lib.rs
git commit -m "feat: generic modal system with track info popup"
```

---

### Task 8: Add tachyonfx Animations

**Files:**
- Modify: `crates/auric-ui/Cargo.toml`
- Modify: `crates/auric-ui/src/shell.rs`

- [ ] **Step 1: Add tachyonfx dependency**

In `crates/auric-ui/Cargo.toml`:
```toml
tachyonfx = "0.7"
```

- [ ] **Step 2: Add fade effect on track change**

In the run_loop, when a `PlayerEvent::Playing` is received, trigger a short fade-in effect on the Now Playing panel area. tachyonfx effects are applied post-render to the buffer.

```rust
use tachyonfx::{fx, Effect, EffectTimer, Interpolation};

// When track changes:
let effect = fx::fade_from(Color::Black, Color::Reset, EffectTimer::from_ms(300, Interpolation::Linear));
```

Store the active effect in ShellState. On each frame, if an effect is active, apply it to the relevant area of the buffer after drawing.

- [ ] **Step 3: Add sweep effect on view/mode transitions**

When switching browse modes, apply a brief horizontal sweep effect.

- [ ] **Step 4: Build, test, commit**

```bash
git add crates/auric-ui/Cargo.toml crates/auric-ui/src/shell.rs
git commit -m "feat: add tachyonfx animations for track changes and view transitions"
```

---

## Implementation Order

1. **Task 1: DB queries** (no UI changes, enables everything else)
2. **Task 2: Focus polish** (quick visual improvement)
3. **Task 3: Browse library** (biggest structural change, depends on Task 1)
4. **Task 4: Seek bar** (independent)
5. **Task 5: Album art** (independent, adds crate dep)
6. **Task 6: Visualizer** (depends on player changes)
7. **Task 7: Modal system** (cleanup + new popup)
8. **Task 8: Animations** (polish layer, goes last)

Tasks 1-2 are sequential. Task 3 depends on 1. Tasks 4, 5, 6, 7, 8 are mostly independent of each other but should follow Task 3 since shell.rs changes will conflict.
