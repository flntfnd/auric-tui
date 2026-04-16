use crate::theme::Palette;
use ratatui::prelude::*;
use ratatui::widgets::Widget;

/// Braille dot positions within a 2x4 cell:
/// (0,0)=0x01  (1,0)=0x08
/// (0,1)=0x02  (1,1)=0x10
/// (0,2)=0x04  (1,2)=0x20
/// (0,3)=0x40  (1,3)=0x80
const BRAILLE_BASE: u32 = 0x2800;
const DOT_MAP: [[u8; 4]; 2] = [
    [0x01, 0x02, 0x04, 0x40],
    [0x08, 0x10, 0x20, 0x80],
];

pub struct SpectrumWidget<'a> {
    pub bands: &'a [f32],
    pub palette: &'a Palette,
}

impl<'a> Widget for SpectrumWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 2 || area.height < 1 || self.bands.is_empty() {
            return;
        }

        // Each terminal cell = 2 dots wide, 4 dots tall
        let dot_cols = area.width as usize * 2;
        let dot_rows = area.height as usize * 4;
        let num_bands = self.bands.len();

        // Map bands to dot columns with spacing:
        // each band gets some dot columns, with a 1-dot gap between bands
        let total_gaps = num_bands.saturating_sub(1);
        let usable_cols = dot_cols.saturating_sub(total_gaps);
        let band_dot_width = (usable_cols / num_bands).max(1);

        // Build a dot grid
        let mut dots = vec![false; dot_cols * dot_rows];

        for (i, &magnitude) in self.bands.iter().enumerate() {
            let col_start = i * (band_dot_width + 1); // +1 for gap
            let fill_dots = (magnitude.clamp(0.0, 1.0) * dot_rows as f32).round() as usize;

            for dc in 0..band_dot_width {
                let col = col_start + dc;
                if col >= dot_cols {
                    break;
                }
                for dr in 0..fill_dots {
                    let row = dot_rows - 1 - dr;
                    dots[row * dot_cols + col] = true;
                }
            }
        }

        // Render dot grid as braille characters
        for cy in 0..area.height as usize {
            for cx in 0..area.width as usize {
                let mut pattern: u8 = 0;
                for dx in 0..2 {
                    for dy in 0..4 {
                        let dot_col = cx * 2 + dx;
                        let dot_row = cy * 4 + dy;
                        if dot_col < dot_cols
                            && dot_row < dot_rows
                            && dots[dot_row * dot_cols + dot_col]
                        {
                            pattern |= DOT_MAP[dx][dy];
                        }
                    }
                }

                if pattern != 0 {
                    let ch = char::from_u32(BRAILLE_BASE + pattern as u32).unwrap_or(' ');
                    // Color based on horizontal position (low/mid/high frequency)
                    let band_idx = cx * 2 * num_bands / dot_cols.max(1);
                    let color = if band_idx < num_bands / 3 {
                        self.palette.visualizer_low
                    } else if band_idx < 2 * num_bands / 3 {
                        self.palette.visualizer_mid
                    } else {
                        self.palette.visualizer_high
                    };

                    let x = area.x + cx as u16;
                    let y = area.y + cy as u16;
                    if let Some(cell) = buf.cell_mut((x, y)) {
                        cell.set_char(ch);
                        cell.set_fg(color);
                    }
                }
            }
        }
    }
}

/// DFT-based spectrum analysis producing log-scaled frequency bands.
/// Sample window is capped at 1024 to keep per-frame cost under ~5ms.
pub fn analyze_spectrum(samples: &[f32], num_bands: usize) -> Vec<f32> {
    if samples.is_empty() || num_bands == 0 {
        return vec![0.0; num_bands];
    }

    let n = samples.len().min(1024);
    let mut bands = vec![0.0f32; num_bands];

    for (band_idx, magnitude) in bands.iter_mut().enumerate() {
        let freq_lo = 20.0 * (16000.0f32 / 20.0).powf(band_idx as f32 / num_bands as f32);
        let freq_hi =
            20.0 * (16000.0f32 / 20.0).powf((band_idx + 1) as f32 / num_bands as f32);
        let bin_lo = (freq_lo / 44100.0 * n as f32) as usize;
        let bin_hi = ((freq_hi / 44100.0 * n as f32) as usize)
            .min(n / 2)
            .max(bin_lo + 1);

        let mut sum = 0.0f32;
        let mut count = 0usize;
        for k in bin_lo..bin_hi {
            let mut re = 0.0f32;
            let mut im = 0.0f32;
            for (i, &s) in samples[..n].iter().enumerate() {
                let angle =
                    2.0 * std::f32::consts::PI * k as f32 * i as f32 / n as f32;
                re += s * angle.cos();
                im -= s * angle.sin();
            }
            sum += (re * re + im * im).sqrt();
            count += 1;
        }
        if count > 0 {
            *magnitude = (sum / count as f32 / n as f32 * 8.0).clamp(0.0, 1.0);
        }
    }

    bands
}
