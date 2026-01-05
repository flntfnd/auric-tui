use std::collections::HashMap;

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Row, Table, Widget},
};
use uuid::Uuid;

use crate::app::{App, Panel};
use crate::library::Track;

pub struct TrackListPanel<'a> {
    app: &'a App,
}

impl<'a> TrackListPanel<'a> {
    pub fn new(app: &'a App) -> Self {
        Self { app }
    }

    /// Build a hashmap for fast track lookup (only when needed)
    fn build_track_map(&self) -> HashMap<Uuid, &Track> {
        self.app.tracks.iter().map(|t| (t.id, t)).collect()
    }
}

impl Widget for TrackListPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let theme = self.app.theme();
        let is_active = self.app.active_panel == Panel::Tracks;
        let border_style = if is_active {
            Style::default().fg(theme.accent_primary)
        } else {
            Style::default().fg(theme.border_inactive)
        };

        let title = format!(
            " Tracks ({}) - Sort: {} ",
            self.app.filtered_track_ids.len(),
            self.app.config.sort_mode.label()
        );

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style);

        let inner = block.inner(area);
        block.render(area, buf);

        if self.app.filtered_track_ids.is_empty() {
            let hint_style = Style::default().fg(theme.foreground_dim);
            buf.set_string(inner.x + 1, inner.y + 1, "No tracks loaded", hint_style);
            buf.set_string(
                inner.x + 1,
                inner.y + 2,
                "Press 'o' to load a folder",
                hint_style,
            );
            return;
        }

        // Calculate visible rows
        let header_height = 1;
        let visible_rows = (inner.height.saturating_sub(header_height)) as usize;

        // Use the stored scroll offset from app
        let scroll_offset = self.app.track_list_offset;

        // Header
        let header_style = Style::default()
            .fg(theme.header)
            .add_modifier(Modifier::BOLD);

        let header = Row::new(vec!["", "Artist", "Title", "Time", "Album"])
            .style(header_style)
            .height(1);

        // Build track lookup map for O(1) access
        let track_map = self.build_track_map();

        // Only process visible tracks (skip + take for efficiency)
        let rows: Vec<Row> = self
            .app
            .filtered_track_ids
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_rows)
            .filter_map(|(i, id)| {
                track_map.get(id).map(|track| {
                    let is_selected = i == self.app.track_selected;
                    let is_playing = Some(track.id) == self.app.current_track_id;

                    let style = if is_selected && is_active {
                        Style::default()
                            .fg(theme.selection_fg)
                            .bg(theme.selection_bg)
                            .add_modifier(Modifier::BOLD)
                    } else if is_selected {
                        Style::default()
                            .fg(theme.track_selected)
                            .add_modifier(Modifier::BOLD)
                    } else if is_playing {
                        Style::default()
                            .fg(theme.track_playing)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.foreground)
                    };

                    let playing_indicator = if is_playing { "▶" } else { " " };

                    Row::new(vec![
                        playing_indicator.to_string(),
                        truncate(&track.artist, 20),
                        truncate(&track.title, 30),
                        track.format_duration(),
                        truncate(&track.album, 25),
                    ])
                    .style(style)
                })
            })
            .collect();

        let widths = [
            Constraint::Length(2),
            Constraint::Percentage(20),
            Constraint::Percentage(30),
            Constraint::Length(6),
            Constraint::Percentage(25),
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .column_spacing(1);

        Widget::render(table, inner, buf);
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else if max_len > 3 {
        format!("{}...", s.chars().take(max_len - 3).collect::<String>())
    } else {
        s.chars().take(max_len).collect()
    }
}
