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

        let elapsed_width = self.elapsed.len() as u16;
        let remaining_width = self.remaining.len() as u16;
        let bar_start = area.x + elapsed_width + 1;
        let bar_end = area.x + area.width.saturating_sub(remaining_width + 1);
        let bar_width = bar_end.saturating_sub(bar_start);

        // Elapsed label
        buf.set_string(area.x, area.y, self.elapsed, Style::default().fg(self.palette.text_muted));

        // Progress bar
        let filled = ((bar_width as f32) * self.progress.clamp(0.0, 1.0)).round() as u16;
        let filled_str = "━";
        let empty_str = "─";
        for x in bar_start..bar_end {
            let is_filled = x.saturating_sub(bar_start) < filled;
            let (s, color) = if is_filled {
                (filled_str, self.palette.progress_fill)
            } else {
                (empty_str, self.palette.border)
            };
            buf.set_string(x, area.y, s, Style::default().fg(color));
        }

        // Playhead
        if filled > 0 && bar_start + filled < bar_end {
            buf.set_string(
                bar_start + filled,
                area.y,
                "●",
                Style::default().fg(self.palette.accent),
            );
        }

        // Remaining label
        if bar_end + 1 < area.x + area.width {
            buf.set_string(
                bar_end + 1,
                area.y,
                self.remaining,
                Style::default().fg(self.palette.text_muted),
            );
        }
    }
}

/// Map a mouse click x-coordinate to a progress value (0.0-1.0).
/// Returns None if the click is outside the bar area.
pub fn click_to_progress(
    click_x: u16,
    bar_area: Rect,
    elapsed_width: u16,
    remaining_width: u16,
) -> Option<f32> {
    let bar_start = bar_area.x + elapsed_width + 1;
    let bar_end = bar_area.x + bar_area.width.saturating_sub(remaining_width + 1);
    if click_x >= bar_start && click_x < bar_end {
        let bar_width = bar_end.saturating_sub(bar_start);
        if bar_width > 0 {
            return Some((click_x - bar_start) as f32 / bar_width as f32);
        }
    }
    None
}
