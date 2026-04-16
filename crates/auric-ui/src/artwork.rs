use ratatui::prelude::*;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::StatefulImage;

pub struct ArtworkState {
    picker: Option<Picker>,
    pub current_image: Option<StatefulProtocol>,
    current_track_path: String,
}

impl std::fmt::Debug for ArtworkState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArtworkState")
            .field("has_image", &self.current_image.is_some())
            .field("track", &self.current_track_path)
            .finish()
    }
}

impl Clone for ArtworkState {
    fn clone(&self) -> Self {
        Self {
            picker: self.picker.clone(),
            current_image: None,
            current_track_path: String::new(),
        }
    }
}

impl ArtworkState {
    /// Create a new ArtworkState. Queries the terminal for graphics protocol
    /// support. Must be called BEFORE entering alternate screen mode.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let picker = Picker::from_query_stdio().ok();
        Self {
            picker,
            current_image: None,
            current_track_path: String::new(),
        }
    }

    /// Update the artwork for the given track. Pass image_data as the raw bytes
    /// (JPEG/PNG) from the database. If pixel_art is true, downscale aggressively
    /// for a chunky look.
    pub fn update(
        &mut self,
        track_path: &str,
        image_data: Option<&[u8]>,
        pixel_art: bool,
        pixel_cell_size: u16,
    ) {
        if track_path == self.current_track_path && self.current_image.is_some() {
            return;
        }
        self.current_track_path = track_path.to_string();

        let Some(picker) = &self.picker else {
            self.current_image = None;
            return;
        };

        self.current_image = image_data.and_then(|data| {
            let mut img = image::load_from_memory(data).ok()?;
            if pixel_art {
                let cell = pixel_cell_size.max(1) as u32;
                let target = cell * 8;
                img = img.resize_exact(target, target, image::imageops::FilterType::Nearest);
            }
            Some(picker.new_resize_protocol(img))
        });
    }

    pub fn has_image(&self) -> bool {
        self.current_image.is_some()
    }

    pub fn render(&mut self, area: Rect, frame: &mut Frame) {
        if let Some(protocol) = &mut self.current_image {
            let image_widget = StatefulImage::default();
            frame.render_stateful_widget(image_widget, area, protocol);
        }
    }

    pub fn clear(&mut self) {
        self.current_image = None;
        self.current_track_path.clear();
    }
}
