use crate::theme::Palette;
use ratatui::prelude::*;
use ratatui::widgets::Widget;
use rustfft::{num_complex::Complex, FftPlanner};

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

        let dot_cols = area.width as usize * 2;
        let dot_rows = area.height as usize * 4;
        let num_bands = self.bands.len();

        let total_gaps = num_bands.saturating_sub(1);
        let usable_cols = dot_cols.saturating_sub(total_gaps);
        let band_dot_width = (usable_cols / num_bands).max(1);

        let mut dots = vec![false; dot_cols * dot_rows];

        for (i, &magnitude) in self.bands.iter().enumerate() {
            let col_start = i * (band_dot_width + 1);
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

/// FFT-based spectrum analysis producing log-scaled frequency bands.
/// Uses rustfft for O(n log n) performance instead of naive DFT.
pub fn analyze_spectrum(samples: &[f32], num_bands: usize) -> Vec<f32> {
    if samples.is_empty() || num_bands == 0 {
        return vec![0.0; num_bands];
    }

    // Use power-of-two FFT size for best performance
    let fft_size = 1024;
    let n = samples.len().min(fft_size);

    // Prepare input with Hann window
    let mut buffer: Vec<Complex<f32>> = Vec::with_capacity(fft_size);
    for i in 0..fft_size {
        let sample = if i < n { samples[i] } else { 0.0 };
        let window = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / fft_size as f32).cos());
        buffer.push(Complex::new(sample * window, 0.0));
    }

    // Run FFT
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);
    fft.process(&mut buffer);

    // Compute magnitudes for the first half (positive frequencies)
    let magnitudes: Vec<f32> = buffer[..fft_size / 2]
        .iter()
        .map(|c| (c.re * c.re + c.im * c.im).sqrt() / fft_size as f32)
        .collect();

    // Map FFT bins to log-scaled frequency bands
    let mut bands = vec![0.0f32; num_bands];
    for (band_idx, band_val) in bands.iter_mut().enumerate() {
        let freq_lo = 20.0 * (16000.0f32 / 20.0).powf(band_idx as f32 / num_bands as f32);
        let freq_hi = 20.0 * (16000.0f32 / 20.0).powf((band_idx + 1) as f32 / num_bands as f32);
        let bin_lo = (freq_lo * fft_size as f32 / 44100.0) as usize;
        let bin_hi = ((freq_hi * fft_size as f32 / 44100.0) as usize)
            .min(magnitudes.len())
            .max(bin_lo + 1);

        let mut sum = 0.0f32;
        let mut count = 0;
        for bin in bin_lo..bin_hi.min(magnitudes.len()) {
            sum += magnitudes[bin];
            count += 1;
        }
        if count > 0 {
            *band_val = (sum / count as f32 * 12.0).clamp(0.0, 1.0);
        }
    }

    bands
}

/// Apply exponential smoothing to make the visualizer fluid.
/// `prev` is the previous frame's bands, `current` is the new FFT result.
/// `attack` (0-1) controls how fast bands rise, `decay` controls how fast they fall.
pub fn smooth_bands(prev: &[f32], current: &[f32], attack: f32, decay: f32) -> Vec<f32> {
    current
        .iter()
        .enumerate()
        .map(|(i, &new_val)| {
            let old_val = prev.get(i).copied().unwrap_or(0.0);
            if new_val > old_val {
                old_val + (new_val - old_val) * attack
            } else {
                old_val + (new_val - old_val) * decay
            }
        })
        .collect()
}
