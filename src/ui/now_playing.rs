use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Widget},
};

use crate::app::{App, LayoutAreas, Panel};
use crate::audio::PlaybackState;
use crate::config::{RepeatMode, Theme};

/// Button widths for playback controls (in terminal columns)
const BTN_PREV_WIDTH: u16 = 4;  // "◀◀ "
const BTN_PLAY_WIDTH: u16 = 4;  // "▶▶ " or "⏸⏸"
const BTN_NEXT_WIDTH: u16 = 4;  // "▶▶ "

pub struct NowPlayingPanel<'a> {
    app: &'a mut App,
}

impl<'a> NowPlayingPanel<'a> {
    pub fn new(app: &'a mut App) -> Self {
        Self { app }
    }

    fn format_duration(secs: u64) -> String {
        let mins = secs / 60;
        let secs = secs % 60;
        format!("{}:{:02}", mins, secs)
    }

    pub fn render(self, area: Rect, buf: &mut Buffer, layout_areas: &mut LayoutAreas) {
        let theme = self.app.theme();
        let is_active = self.app.active_panel == Panel::NowPlaying;
        let border_style = if is_active {
            Style::default().fg(theme.accent_primary)
        } else {
            Style::default().fg(theme.border_inactive)
        };

        let block = Block::default()
            .title(" Now Playing ")
            .borders(Borders::ALL)
            .border_style(border_style);

        let inner = block.inner(area);
        block.render(area, buf);

        // Split into left (album art) and right (info)
        let show_album_art = self.app.config.show_album_art;
        let chunks = if show_album_art {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(14), // Album art area
                    Constraint::Min(20),    // Track info
                ])
                .split(inner)
        } else {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(0), // No album art
                    Constraint::Min(20),   // Track info
                ])
                .split(inner)
        };

        let art_area = chunks[0];
        let info_area = chunks[1];

        // Render album art using the renderer (only if enabled)
        if show_album_art {
            self.app.album_art_renderer.render(art_area, buf);
        }

        // Render track info
        self.render_track_info(info_area, buf, &theme, layout_areas);
    }

    fn render_track_info(&self, area: Rect, buf: &mut Buffer, theme: &Theme, layout_areas: &mut LayoutAreas) {
        if let Some(track) = self.app.current_track() {
            // Track title
            let title_style = Style::default()
                .fg(theme.foreground)
                .add_modifier(Modifier::BOLD);
            let title = truncate_string(&track.title, area.width as usize - 2);
            buf.set_string(area.x + 1, area.y, &title, title_style);

            // Artist
            let artist_style = Style::default().fg(theme.now_playing_accent);
            let artist = truncate_string(&track.artist, area.width as usize - 2);
            buf.set_string(area.x + 1, area.y + 1, &artist, artist_style);

            // Album
            let album_style = Style::default().fg(theme.foreground_dim);
            let album = truncate_string(&track.album, area.width as usize - 2);
            buf.set_string(area.x + 1, area.y + 2, &album, album_style);

            // Controls line
            let controls_area = Rect::new(area.x, area.y + 4, area.width, 1);
            self.render_controls(controls_area, buf, theme, layout_areas);

            // Progress bar
            let progress_area = Rect::new(area.x, area.y + 5, area.width, 1);
            self.render_progress(progress_area, buf, theme, layout_areas);
        } else {
            let no_track_style = Style::default().fg(theme.foreground_dim);
            buf.set_string(area.x + 1, area.y + 1, "No track playing", no_track_style);
            buf.set_string(
                area.x + 1,
                area.y + 2,
                "Select a track and press Enter",
                no_track_style,
            );
            // Clear button areas when no track
            layout_areas.np_prev_btn = Rect::default();
            layout_areas.np_play_btn = Rect::default();
            layout_areas.np_next_btn = Rect::default();
            layout_areas.np_progress_bar = Rect::default();
        }
    }

    fn render_controls(&self, area: Rect, buf: &mut Buffer, theme: &Theme, layout_areas: &mut LayoutAreas) {
        let playing = self.app.player.state() == PlaybackState::Playing;
        let shuffle = self.app.config.shuffle;
        let repeat = self.app.config.repeat;
        let volume = (self.app.player.volume() * 100.0) as u8;

        let base_x = area.x + 1;
        let y = area.y;
        let mut x = base_x;

        // Previous button: [◀◀]
        let prev_style = Style::default().fg(theme.foreground);
        buf.set_string(x, y, "[◀◀]", prev_style);
        layout_areas.np_prev_btn = Rect::new(x, y, BTN_PREV_WIDTH, 1);
        x += BTN_PREV_WIDTH + 1; // +1 for spacing

        // Play/Pause button: [▶▶] or [⏸⏸]
        let play_style = if playing {
            Style::default().fg(theme.success)
        } else {
            Style::default().fg(theme.foreground)
        };
        let play_symbol = if playing { "[⏸⏸]" } else { "[▶▶]" };
        buf.set_string(x, y, play_symbol, play_style);
        layout_areas.np_play_btn = Rect::new(x, y, BTN_PLAY_WIDTH, 1);
        x += BTN_PLAY_WIDTH + 1;

        // Next button: [▶▶]
        let next_style = Style::default().fg(theme.foreground);
        buf.set_string(x, y, "[▶▶]", next_style);
        layout_areas.np_next_btn = Rect::new(x, y, BTN_NEXT_WIDTH, 1);
        x += BTN_NEXT_WIDTH + 2; // +2 for extra spacing

        // Shuffle indicator
        let shuffle_style = if shuffle {
            Style::default().fg(theme.success)
        } else {
            Style::default().fg(theme.foreground_dim)
        };
        buf.set_string(x, y, "⤨", shuffle_style);
        x += 2;

        // Repeat indicator
        let repeat_style = match repeat {
            RepeatMode::Off => Style::default().fg(theme.foreground_dim),
            RepeatMode::All | RepeatMode::One => Style::default().fg(theme.success),
        };
        buf.set_string(x, y, repeat.symbol(), repeat_style);
        x += 3;

        // Volume display
        let volume_style = Style::default().fg(theme.accent_primary);
        buf.set_string(x, y, &format!("Vol: {}%", volume), volume_style);
    }

    fn render_progress(&self, area: Rect, buf: &mut Buffer, theme: &Theme, layout_areas: &mut LayoutAreas) {
        let position = self.app.player.position();
        let duration = self.app.player.duration();
        let progress = self.app.player.progress();

        let position_str = Self::format_duration(position.as_secs());
        let duration_str = Self::format_duration(duration.as_secs());

        // Time labels
        let time_style = Style::default().fg(theme.foreground_dim);
        buf.set_string(area.x + 1, area.y, &position_str, time_style);

        let duration_x = area.x + area.width - duration_str.len() as u16 - 1;
        buf.set_string(duration_x, area.y, &duration_str, time_style);

        // Progress bar in the middle
        let bar_start = area.x + position_str.len() as u16 + 2;
        let bar_end = duration_x - 2;
        let bar_width = bar_end.saturating_sub(bar_start) as usize;

        // Store progress bar area for click detection
        layout_areas.np_progress_bar = Rect::new(bar_start, area.y, bar_width as u16, 1);

        if bar_width > 2 {
            let filled = ((progress * bar_width as f64) as usize).min(bar_width);
            let filled_bar = "━".repeat(filled);
            let empty_bar = "─".repeat(bar_width.saturating_sub(filled));

            let filled_style = Style::default().fg(theme.progress_bar_filled);
            let empty_style = Style::default().fg(theme.progress_bar_empty);

            buf.set_string(bar_start, area.y, &filled_bar, filled_style);
            buf.set_string(bar_start + filled as u16, area.y, &empty_bar, empty_style);

            // Position indicator (scrubber handle)
            let handle_pos = bar_start + filled as u16;
            if handle_pos < bar_start + bar_width as u16 {
                buf.set_string(
                    handle_pos,
                    area.y,
                    "⬤",
                    Style::default().fg(theme.accent_primary),
                );
            }
        }
    }
}

fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else if max_len > 3 {
        format!("{}...", &s[..max_len - 3])
    } else {
        s[..max_len].to_string()
    }
}
