pub mod album_art;
pub mod file_browser;
pub mod layout;
pub mod library_panel;
pub mod now_playing;
pub mod spectrum;
pub mod track_list;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Widget},
};

use crate::app::{App, InputMode, LayoutAreas, SettingsSelection};

pub use album_art::AlbumArtRenderer;
pub use file_browser::{FileBrowser, FileBrowserWidget};
pub use layout::AppLayout;
pub use library_panel::{FoldersPanel, PlaylistsPanel, WatchedFoldersPanel};
pub use now_playing::NowPlayingPanel;
pub use track_list::TrackListPanel;

pub fn render(app: &mut App, area: Rect, buf: &mut Buffer) {
    let has_watched_folders = app.watched_folders_count() > 0;
    let layout = AppLayout::new(area, has_watched_folders);

    // Create layout areas for mouse hit testing (will be populated by render functions)
    let mut layout_areas = LayoutAreas {
        watched_section: layout.watched_section,
        library_section: layout.library_section,
        playlists_section: layout.playlists_section,
        track_list: layout.track_list,
        now_playing: layout.now_playing,
        ..Default::default()
    };

    // Header (uses immutable borrow)
    render_header(app, layout.header, buf);

    // Cache layout areas for use after mutable borrow
    let watched_section = layout.watched_section;
    let library_section = layout.library_section;
    let playlists_section = layout.playlists_section;
    let now_playing = layout.now_playing;
    let spectrum_area = layout.spectrum;
    let track_list = layout.track_list;
    let footer = layout.footer;

    // Render now playing first (needs mutable access for album art)
    // This also populates button and progress bar areas in layout_areas
    NowPlayingPanel::new(app).render(now_playing, buf, &mut layout_areas);

    // Update layout areas for mouse hit testing (after now_playing populates sub-areas)
    app.update_layout_areas(layout_areas);

    // Render spectrum analyzer
    render_spectrum(app, spectrum_area, buf);

    // Now render the rest with immutable borrows
    if has_watched_folders {
        WatchedFoldersPanel::new(app).render(watched_section, buf);
    }
    FoldersPanel::new(app).render(library_section, buf);
    PlaylistsPanel::new(app).render(playlists_section, buf);
    TrackListPanel::new(app).render(track_list, buf);

    // Footer
    render_footer(app, footer, buf);

    // Overlays
    match app.input_mode {
        InputMode::FileBrowser | InputMode::AddWatchedFolder => {
            if let Some(ref browser) = app.file_browser {
                let title = if app.input_mode == InputMode::AddWatchedFolder {
                    " Add Watched Folder "
                } else {
                    " Select Folder "
                };
                FileBrowserWidget::new(browser).with_title(title).render(area, buf);
            }
        }
        InputMode::Search => render_input_dialog(app, "Search", "Search tracks:", area, buf),
        InputMode::NewPlaylist => {
            render_input_dialog(app, "New Playlist", "Playlist name:", area, buf)
        }
        InputMode::Help => render_help_dialog(area, buf),
        InputMode::Settings => render_settings_dialog(app, area, buf),
        InputMode::Confirm(action) => {
            let msg = match action {
                crate::app::ConfirmAction::DeletePlaylist => "Delete this playlist?",
                crate::app::ConfirmAction::RemoveFromPlaylist => "Remove from playlist?",
                crate::app::ConfirmAction::DeleteFolder => "Remove this folder?",
            };
            render_confirm_dialog(msg, area, buf);
        }
        InputMode::Normal => {}
    }
}

fn render_header(app: &App, area: Rect, buf: &mut Buffer) {
    let theme = app.theme();
    let title_style = Style::default()
        .fg(theme.accent_primary)
        .add_modifier(Modifier::BOLD);

    let title = Line::from(vec![
        Span::styled(" Auric ", title_style),
        Span::styled("│", Style::default().fg(theme.foreground_dim)),
        Span::styled(" Press ", Style::default().fg(theme.foreground_dim)),
        Span::styled("?", Style::default().fg(theme.hint_key)),
        Span::styled(" for help", Style::default().fg(theme.foreground_dim)),
    ]);

    buf.set_line(area.x, area.y, &title, area.width);
}

fn render_footer(app: &App, area: Rect, buf: &mut Buffer) {
    let theme = app.theme();
    let hint_style = Style::default().fg(theme.hint_text);
    let key_style = Style::default().fg(theme.hint_key);

    let line1 = Line::from(vec![
        Span::styled(" ", hint_style),
        Span::styled("o", key_style),
        Span::styled(":Load  ", hint_style),
        Span::styled("Space", key_style),
        Span::styled(":Play/Pause  ", hint_style),
        Span::styled("[/]", key_style),
        Span::styled(":Seek  ", hint_style),
        Span::styled("+/-", key_style),
        Span::styled(":Volume  ", hint_style),
        Span::styled("s", key_style),
        Span::styled(":Shuffle  ", hint_style),
        Span::styled("r", key_style),
        Span::styled(":Repeat  ", hint_style),
        Span::styled("q", key_style),
        Span::styled(":Quit", hint_style),
    ]);

    buf.set_line(area.x, area.y, &line1, area.width);

    // Status message on second line
    if let Some(ref msg) = app.status_message {
        let status_style = Style::default().fg(theme.success);
        buf.set_string(area.x + 1, area.y + 1, msg, status_style);
    }
}

fn render_spectrum(app: &mut App, area: Rect, buf: &mut Buffer) {
    let theme = app.theme();

    // Draw border
    let block = Block::default()
        .title(" Spectrum ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_inactive));

    let inner = block.inner(area);
    block.render(area, buf);

    // Only render spectrum if enabled
    if app.config.spectrum_enabled {
        // Get spectrum data from the analyzer
        let bars = app.get_spectrum_bars();
        // Render spectrum widget with theme colors
        spectrum::SpectrumWidget::new(&bars)
            .with_colors(theme.spectrum_low, theme.spectrum_mid, theme.spectrum_high)
            .render(inner, buf);
    } else {
        // Show disabled message
        let hint_style = Style::default().fg(theme.foreground_dim);
        buf.set_string(inner.x + 1, inner.y, "Disabled", hint_style);
    }
}

fn render_input_dialog(app: &App, title: &str, prompt: &str, area: Rect, buf: &mut Buffer) {
    let dialog_width = 60.min(area.width - 4);
    let dialog_height = 5;
    let dialog_x = (area.width - dialog_width) / 2;
    let dialog_y = (area.height - dialog_height) / 2;

    let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

    // Clear background
    Clear.render(dialog_area, buf);

    let block = Block::default()
        .title(format!(" {} ", title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(dialog_area);
    block.render(dialog_area, buf);

    // Prompt
    let prompt_style = Style::default().fg(Color::White);
    buf.set_string(inner.x + 1, inner.y, prompt, prompt_style);

    // Input field
    let input_style = Style::default()
        .fg(Color::White)
        .bg(Color::DarkGray);
    let input_text = format!("{}_", &app.input_buffer);
    buf.set_string(inner.x + 1, inner.y + 1, &input_text, input_style);

    // Hint
    let hint_style = Style::default().fg(Color::DarkGray);
    buf.set_string(inner.x + 1, inner.y + 2, "Enter to confirm, Esc to cancel", hint_style);
}

fn render_confirm_dialog(message: &str, area: Rect, buf: &mut Buffer) {
    let dialog_width = 40.min(area.width - 4);
    let dialog_height = 5;
    let dialog_x = (area.width - dialog_width) / 2;
    let dialog_y = (area.height - dialog_height) / 2;

    let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

    Clear.render(dialog_area, buf);

    let block = Block::default()
        .title(" Confirm ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(dialog_area);
    block.render(dialog_area, buf);

    let msg_style = Style::default().fg(Color::White);
    buf.set_string(inner.x + 1, inner.y, message, msg_style);

    let hint_style = Style::default().fg(Color::DarkGray);
    buf.set_string(inner.x + 1, inner.y + 1, "Press 'y' to confirm, 'n' to cancel", hint_style);
}

fn render_help_dialog(area: Rect, buf: &mut Buffer) {
    let dialog_width = 50.min(area.width - 4);
    let dialog_height = 20.min(area.height - 4);
    let dialog_x = (area.width - dialog_width) / 2;
    let dialog_y = (area.height - dialog_height) / 2;

    let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

    Clear.render(dialog_area, buf);

    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(dialog_area);
    block.render(dialog_area, buf);

    let help_text = vec![
        "Navigation:",
        "  Tab/Shift+Tab  Switch panels",
        "  j/k or ↑/↓     Move selection",
        "  Enter          Play selected/Open",
        "",
        "Playback:",
        "  Space          Play/Pause",
        "  [ / ]          Seek back/forward",
        "  + / -          Volume up/down",
        "  s              Toggle shuffle",
        "  r              Toggle repeat",
        "  S              Cycle sort mode",
        "",
        "Library:",
        "  o              Load folder",
        "  w              Add watched folder",
        "  d              Remove folder/stop watch",
        "  A              Fetch missing art",
        "  Ctrl+F         Search tracks",
        "  N              New playlist",
        "  a              Add to playlist",
        "  ,              Settings (theme, etc.)",
        "",
        "Press any key to close",
    ];

    let desc_style = Style::default().fg(Color::White);
    let header_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    for (i, line) in help_text.iter().enumerate() {
        if i as u16 >= inner.height {
            break;
        }

        let style = if line.ends_with(':') && !line.starts_with(' ') {
            header_style
        } else {
            desc_style
        };

        buf.set_string(inner.x + 1, inner.y + i as u16, line, style);
    }
}

fn render_settings_dialog(app: &App, area: Rect, buf: &mut Buffer) {
    let dialog_width = 50.min(area.width - 4);
    let dialog_height = 12.min(area.height - 4);
    let dialog_x = (area.width - dialog_width) / 2;
    let dialog_y = (area.height - dialog_height) / 2;

    let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

    Clear.render(dialog_area, buf);

    let theme = app.theme();
    let block = Block::default()
        .title(" Settings ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.dialog_border));

    let inner = block.inner(dialog_area);
    block.render(dialog_area, buf);

    let selected = app.settings_selection;
    let label_style = Style::default().fg(theme.foreground);
    let value_style = Style::default().fg(theme.accent_primary);
    let selected_style = Style::default()
        .fg(theme.selection_fg)
        .bg(theme.selection_bg)
        .add_modifier(Modifier::BOLD);
    let hint_style = Style::default().fg(theme.foreground_dim);

    // Theme setting
    let y_offset = 0u16;
    let theme_label = "Theme:";
    let theme_value = format!("< {} >", app.config.theme.label());
    if selected == SettingsSelection::Theme {
        buf.set_string(inner.x + 1, inner.y + y_offset, theme_label, selected_style);
        buf.set_string(inner.x + 20, inner.y + y_offset, &theme_value, selected_style);
    } else {
        buf.set_string(inner.x + 1, inner.y + y_offset, theme_label, label_style);
        buf.set_string(inner.x + 20, inner.y + y_offset, &theme_value, value_style);
    }

    // Spectrum Analyzer setting
    let y_offset = 2u16;
    let spectrum_label = "Spectrum Analyzer:";
    let spectrum_value = if app.config.spectrum_enabled { "< ON >" } else { "< OFF >" };
    if selected == SettingsSelection::SpectrumAnalyzer {
        buf.set_string(inner.x + 1, inner.y + y_offset, spectrum_label, selected_style);
        buf.set_string(inner.x + 20, inner.y + y_offset, spectrum_value, selected_style);
    } else {
        buf.set_string(inner.x + 1, inner.y + y_offset, spectrum_label, label_style);
        buf.set_string(inner.x + 20, inner.y + y_offset, spectrum_value, value_style);
    }

    // Album Art setting
    let y_offset = 4u16;
    let art_label = "Album Art:";
    let art_value = if app.config.show_album_art { "< ON >" } else { "< OFF >" };
    if selected == SettingsSelection::AlbumArt {
        buf.set_string(inner.x + 1, inner.y + y_offset, art_label, selected_style);
        buf.set_string(inner.x + 20, inner.y + y_offset, art_value, selected_style);
    } else {
        buf.set_string(inner.x + 1, inner.y + y_offset, art_label, label_style);
        buf.set_string(inner.x + 20, inner.y + y_offset, art_value, value_style);
    }

    // Hints
    let y_offset = 7u16;
    buf.set_string(
        inner.x + 1,
        inner.y + y_offset,
        "Use ↑/↓ to navigate, ←/→ or Enter to change",
        hint_style,
    );
    buf.set_string(
        inner.x + 1,
        inner.y + y_offset + 1,
        "Press Esc to close",
        hint_style,
    );
}
