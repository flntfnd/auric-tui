use std::collections::HashSet;
use std::path::PathBuf;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, StatefulWidget, Widget},
};

use crate::app::{App, Panel};
use crate::config::Theme;
use crate::library::{LoadedFolder, Playlist};

/// Panel showing watched folders (auto-sync with file system)
pub struct WatchedFoldersPanel<'a> {
    folders: Vec<&'a LoadedFolder>,
    cursor: usize,
    selected_paths: &'a HashSet<PathBuf>,
    is_active: bool,
    theme: Theme,
}

impl<'a> WatchedFoldersPanel<'a> {
    pub fn new(app: &'a App) -> Self {
        let folders: Vec<&LoadedFolder> = app.folders.iter().filter(|f| f.is_watched).collect();
        Self {
            folders,
            cursor: app.watched_folder_cursor,
            selected_paths: &app.watched_folders_selected,
            is_active: app.active_panel == Panel::WatchedFolders,
            theme: app.theme(),
        }
    }
}

impl Widget for WatchedFoldersPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let theme = &self.theme;
        let accent = theme.accent_secondary; // Magenta-like for watched folders

        let border_style = if self.is_active {
            Style::default().fg(accent)
        } else {
            Style::default().fg(theme.border_inactive)
        };

        let block = Block::default()
            .title(" Watched ")
            .borders(Borders::ALL)
            .border_style(border_style);

        let inner = block.inner(area);
        block.render(area, buf);

        if self.folders.is_empty() {
            let hint = Line::from(vec![
                Span::styled("Press ", Style::default().fg(theme.foreground_dim)),
                Span::styled("w", Style::default().fg(theme.hint_key)),
                Span::styled(" to watch", Style::default().fg(theme.foreground_dim)),
            ]);
            buf.set_line(inner.x + 1, inner.y, &hint, inner.width - 2);
            return;
        }

        let items: Vec<ListItem> = self
            .folders
            .iter()
            .enumerate()
            .map(|(i, folder)| {
                let is_cursor = i == self.cursor;
                let is_selected = self.selected_paths.contains(&folder.path);

                let style = if is_cursor && self.is_active {
                    Style::default()
                        .fg(theme.selection_fg)
                        .bg(accent)
                        .add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default()
                        .fg(accent)
                        .add_modifier(Modifier::BOLD)
                } else if is_cursor {
                    Style::default().fg(accent)
                } else {
                    Style::default().fg(theme.foreground)
                };

                // Show selection indicator
                let prefix = if is_selected { "● " } else { "  " };
                let content = format!("{}{} ({})", prefix, folder.name, folder.track_count);
                ListItem::new(Line::from(content)).style(style)
            })
            .collect();

        let list = List::new(items);
        let mut state = ListState::default().with_selected(Some(self.cursor));
        StatefulWidget::render(list, inner, buf, &mut state);
    }
}

/// Panel showing regular (non-watched) folders
pub struct FoldersPanel<'a> {
    folders: Vec<&'a LoadedFolder>,
    cursor: usize,
    selected_paths: &'a HashSet<PathBuf>,
    is_active: bool,
    theme: Theme,
}

impl<'a> FoldersPanel<'a> {
    pub fn new(app: &'a App) -> Self {
        let folders: Vec<&LoadedFolder> = app.folders.iter().filter(|f| !f.is_watched).collect();
        Self {
            folders,
            cursor: app.folder_cursor,
            selected_paths: &app.folders_selected,
            is_active: app.active_panel == Panel::Library,
            theme: app.theme(),
        }
    }
}

impl Widget for FoldersPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let theme = &self.theme;
        let accent = theme.accent_primary; // Cyan-like for folders

        let border_style = if self.is_active {
            Style::default().fg(accent)
        } else {
            Style::default().fg(theme.border_inactive)
        };

        let block = Block::default()
            .title(" Folders ")
            .borders(Borders::ALL)
            .border_style(border_style);

        let inner = block.inner(area);
        block.render(area, buf);

        if self.folders.is_empty() {
            let hint = Line::from(vec![
                Span::styled("Press ", Style::default().fg(theme.foreground_dim)),
                Span::styled("o", Style::default().fg(theme.hint_key)),
                Span::styled(" to load", Style::default().fg(theme.foreground_dim)),
            ]);
            buf.set_line(inner.x + 1, inner.y, &hint, inner.width - 2);
            return;
        }

        let items: Vec<ListItem> = self
            .folders
            .iter()
            .enumerate()
            .map(|(i, folder)| {
                let is_cursor = i == self.cursor;
                let is_selected = self.selected_paths.contains(&folder.path);

                let style = if is_cursor && self.is_active {
                    Style::default()
                        .fg(theme.selection_fg)
                        .bg(accent)
                        .add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default()
                        .fg(accent)
                        .add_modifier(Modifier::BOLD)
                } else if is_cursor {
                    Style::default().fg(accent)
                } else {
                    Style::default().fg(theme.foreground)
                };

                // Show selection indicator
                let prefix = if is_selected { "● " } else { "  " };
                let content = format!("{}{} ({})", prefix, folder.name, folder.track_count);
                ListItem::new(Line::from(content)).style(style)
            })
            .collect();

        let list = List::new(items);
        let mut state = ListState::default().with_selected(Some(self.cursor));
        StatefulWidget::render(list, inner, buf, &mut state);
    }
}

pub struct PlaylistsPanel<'a> {
    playlists: &'a [Playlist],
    selected: usize,
    is_active: bool,
    theme: Theme,
}

impl<'a> PlaylistsPanel<'a> {
    pub fn new(app: &'a App) -> Self {
        Self {
            playlists: &app.playlists,
            selected: app.playlist_selected,
            is_active: app.active_panel == Panel::Playlists,
            theme: app.theme(),
        }
    }
}

impl Widget for PlaylistsPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let theme = &self.theme;
        let accent = theme.accent_tertiary; // Green-like for playlists

        let border_style = if self.is_active {
            Style::default().fg(theme.accent_primary)
        } else {
            Style::default().fg(theme.border_inactive)
        };

        let block = Block::default()
            .title(" Playlists ")
            .borders(Borders::ALL)
            .border_style(border_style);

        let inner = block.inner(area);
        block.render(area, buf);

        // Always show "New Playlist" option
        let mut items: Vec<ListItem> = vec![ListItem::new(Line::from(vec![
            Span::styled("+ ", Style::default().fg(accent)),
            Span::styled("New Playlist", Style::default().fg(accent)),
        ]))];

        for (i, playlist) in self.playlists.iter().enumerate() {
            let style = if i == self.selected && self.is_active {
                Style::default()
                    .fg(theme.selection_fg)
                    .bg(theme.accent_primary)
                    .add_modifier(Modifier::BOLD)
            } else if i == self.selected {
                Style::default()
                    .fg(theme.accent_primary)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.foreground)
            };

            let content = format!("  {} ({})", playlist.name, playlist.len());
            items.push(ListItem::new(Line::from(content)).style(style));
        }

        let list = List::new(items);
        Widget::render(list, inner, buf);
    }
}
