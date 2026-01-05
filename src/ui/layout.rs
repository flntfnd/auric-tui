use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub struct AppLayout {
    pub header: Rect,
    pub watched_section: Rect,
    pub library_section: Rect,
    pub playlists_section: Rect,
    pub now_playing: Rect,
    pub spectrum: Rect,
    pub track_list: Rect,
    pub footer: Rect,
}

impl AppLayout {
    pub fn new(area: Rect, has_watched_folders: bool) -> Self {
        // Main vertical split: header, content, footer
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),  // Header
                Constraint::Min(10),    // Content
                Constraint::Length(2),  // Footer
            ])
            .split(area);

        let header = main_chunks[0];
        let content = main_chunks[1];
        let footer = main_chunks[2];

        // Content horizontal split: left panel, main panel
        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25), // Left panel (folders/playlists)
                Constraint::Percentage(75), // Main panel (now playing + tracks)
            ])
            .split(content);

        let left_panel = content_chunks[0];
        let main_panel = content_chunks[1];

        // Left panel split: watched folders, regular folders, playlists
        let (watched_section, library_section, playlists_section) = if has_watched_folders {
            let left_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(30), // Watched folders
                    Constraint::Percentage(35), // Regular folders
                    Constraint::Percentage(35), // Playlists
                ])
                .split(left_panel);
            (left_chunks[0], left_chunks[1], left_chunks[2])
        } else {
            let left_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(0),      // No watched folders section
                    Constraint::Percentage(50), // Folders
                    Constraint::Percentage(50), // Playlists
                ])
                .split(left_panel);
            (left_chunks[0], left_chunks[1], left_chunks[2])
        };

        // Main panel split: now playing row, track list
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(8),  // Now playing area
                Constraint::Min(5),     // Track list
            ])
            .split(main_panel);

        let now_playing_row = main_chunks[0];
        let track_list = main_chunks[1];

        // Split now playing row: now playing info (left), spectrum (right)
        let now_playing_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(30),        // Now playing info (album art + text)
                Constraint::Length(34),     // Spectrum visualizer (fixed width for 32 bars + border)
            ])
            .split(now_playing_row);

        let now_playing = now_playing_chunks[0];
        let spectrum = now_playing_chunks[1];

        Self {
            header,
            watched_section,
            library_section,
            playlists_section,
            now_playing,
            spectrum,
            track_list,
            footer,
        }
    }
}
