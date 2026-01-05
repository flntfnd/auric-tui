use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

/// Unicode block characters for high-resolution vertical bars
/// These give us 8 levels of granularity per character cell
const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Spectrum visualization widget
pub struct SpectrumWidget<'a> {
    bars: &'a [f32],
    style: Style,
    color_low: Color,
    color_mid: Color,
    color_high: Color,
}

impl<'a> SpectrumWidget<'a> {
    pub fn new(bars: &'a [f32]) -> Self {
        Self {
            bars,
            style: Style::default(),
            color_low: Color::Green,
            color_mid: Color::Yellow,
            color_high: Color::Red,
        }
    }

    #[allow(dead_code)]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub fn with_colors(mut self, low: Color, mid: Color, high: Color) -> Self {
        self.color_low = low;
        self.color_mid = mid;
        self.color_high = high;
        self
    }
}

impl Widget for SpectrumWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 || self.bars.is_empty() {
            return;
        }

        // Extract RGB components from theme colors for interpolation
        let (low_r, low_g, low_b) = color_to_rgb(self.color_low);
        let (mid_r, mid_g, mid_b) = color_to_rgb(self.color_mid);
        let (high_r, high_g, high_b) = color_to_rgb(self.color_high);

        let num_bars = self.bars.len();
        let bar_width = area.width as usize;

        // Map the frequency bars to the available width
        // Use interpolation if we have more or fewer bars than width
        for x in 0..bar_width {
            // Map x position to bar index
            let bar_idx = (x * num_bars) / bar_width;
            let magnitude = self.bars.get(bar_idx).copied().unwrap_or(0.0);

            // Calculate bar height in "sub-characters" (8 levels per char)
            let max_height = area.height as f32 * 8.0;
            let bar_height = (magnitude * max_height) as usize;

            // Render from bottom to top
            for y in 0..area.height {
                let row_from_bottom = area.height - 1 - y;
                let cell_bottom = row_from_bottom as usize * 8;
                let cell_top = cell_bottom + 8;

                let block_char = if bar_height >= cell_top {
                    // Full block
                    BLOCKS[8]
                } else if bar_height > cell_bottom {
                    // Partial block
                    let partial = bar_height - cell_bottom;
                    BLOCKS[partial.min(8)]
                } else {
                    // Empty
                    BLOCKS[0]
                };

                // Smooth gradient interpolation between theme colors
                let position = bar_idx as f32 / num_bars as f32;
                let intensity = magnitude.clamp(0.3, 1.0); // Keep minimum brightness

                let (r, g, b) = if position < 0.5 {
                    // Interpolate from low to mid
                    let t = position * 2.0;
                    (
                        lerp(low_r, mid_r, t),
                        lerp(low_g, mid_g, t),
                        lerp(low_b, mid_b, t),
                    )
                } else {
                    // Interpolate from mid to high
                    let t = (position - 0.5) * 2.0;
                    (
                        lerp(mid_r, high_r, t),
                        lerp(mid_g, high_g, t),
                        lerp(mid_b, high_b, t),
                    )
                };

                // Apply intensity based on magnitude
                let color = Color::Rgb(
                    (r as f32 * intensity) as u8,
                    (g as f32 * intensity) as u8,
                    (b as f32 * intensity) as u8,
                );

                buf.set_string(
                    area.x + x as u16,
                    area.y + y,
                    block_char.to_string(),
                    Style::default().fg(color),
                );
            }
        }
    }
}

/// Linear interpolation between two values
fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t) as u8
}

/// Extract RGB components from a Color, with fallback for non-RGB colors
fn color_to_rgb(color: Color) -> (u8, u8, u8) {
    match color {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Green => (0, 255, 0),
        Color::Yellow => (255, 255, 0),
        Color::Red => (255, 0, 0),
        Color::Cyan => (0, 255, 255),
        Color::Magenta => (255, 0, 255),
        Color::Blue => (0, 0, 255),
        Color::White => (255, 255, 255),
        Color::LightGreen => (144, 238, 144),
        Color::LightYellow => (255, 255, 224),
        Color::LightRed => (255, 128, 128),
        Color::LightCyan => (224, 255, 255),
        Color::LightMagenta => (255, 128, 255),
        Color::LightBlue => (173, 216, 230),
        _ => (128, 128, 128), // Default gray for unknown colors
    }
}

