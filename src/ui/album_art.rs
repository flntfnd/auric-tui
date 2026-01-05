use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::StatefulWidget,
};
use ratatui_image::{
    picker::Picker,
    protocol::StatefulProtocol,
    Resize, StatefulImage,
};

use crate::library::Track;

/// Manages album art rendering with terminal graphics protocol detection
pub struct AlbumArtRenderer {
    picker: Picker,
    current_image: Option<StatefulProtocol>,
    current_track_id: Option<uuid::Uuid>,
}

impl AlbumArtRenderer {
    pub fn new() -> Self {
        // Create picker to detect terminal capabilities (Sixel, Kitty, etc.)
        let picker = Picker::from_query_stdio().unwrap_or_else(|_| {
            // Fallback to halfblocks if query fails
            Picker::halfblocks()
        });

        Self {
            picker,
            current_image: None,
            current_track_id: None,
        }
    }

    /// Update the album art for a new track
    pub fn set_track(&mut self, track: Option<&Track>) {
        let track_id = track.map(|t| t.id);

        // Only update if track changed
        if track_id == self.current_track_id {
            return;
        }

        self.current_track_id = track_id;

        // Try to load album art from track
        let image = track.and_then(|t| {
            // First try pre-loaded image
            t.album_art.clone().or_else(|| {
                // Then try to decode from raw data
                t.album_art_data
                    .as_ref()
                    .and_then(|data| image::load_from_memory(data).ok())
            })
        });

        // Create protocol image if we have album art
        self.current_image = image.map(|img| self.picker.new_resize_protocol(img));
    }

    /// Render the album art (or placeholder) to the given area
    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        if let Some(ref mut protocol) = self.current_image {
            // Create a stateful image widget
            let image_widget = StatefulImage::new().resize(Resize::Fit(None));
            StatefulWidget::render(image_widget, area, buf, protocol);
        } else {
            render_placeholder(area, buf);
        }
    }
}

impl Default for AlbumArtRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Render a placeholder when no album art is available
fn render_placeholder(area: Rect, buf: &mut Buffer) {
    let style = Style::default().fg(Color::DarkGray);

    let art = [
        "┌──────────┐",
        "│          │",
        "│    ♪♫    │",
        "│          │",
        "│  No Art  │",
        "│          │",
        "└──────────┘",
    ];

    let start_y = area.y + (area.height.saturating_sub(art.len() as u16)) / 2;
    let start_x = area.x + (area.width.saturating_sub(12)) / 2;

    for (i, line) in art.iter().enumerate() {
        if start_y + (i as u16) < area.y + area.height {
            buf.set_string(start_x, start_y + i as u16, *line, style);
        }
    }
}
