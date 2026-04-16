use crate::theme::Palette;
use ratatui::prelude::*;
use ratatui::widgets::Widget;

const BAR_CHARS: [char; 8] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '█'];

pub struct SpectrumWidget<'a> {
    pub bands: &'a [f32],
    pub palette: &'a Palette,
}

impl<'a> Widget for SpectrumWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 4 || area.height < 1 || self.bands.is_empty() {
            return;
        }
        let num_bands = self.bands.len().min(area.width as usize);
        let bar_width = (area.width as usize / num_bands).max(1);

        for (i, &magnitude) in self.bands.iter().take(num_bands).enumerate() {
            let x = area.x + (i * bar_width) as u16;
            if x >= area.x + area.width {
                break;
            }
            let color = if i < num_bands / 3 {
                self.palette.visualizer_low
            } else if i < 2 * num_bands / 3 {
                self.palette.visualizer_mid
            } else {
                self.palette.visualizer_high
            };

            let max_h = area.height as f32;
            let fill_h = (magnitude.clamp(0.0, 1.0) * max_h).round();

            for row in 0..area.height {
                let y = area.y + area.height - 1 - row;
                let row_f = row as f32;
                let ch = if row_f + 1.0 <= fill_h {
                    BAR_CHARS[7]
                } else if row_f < fill_h {
                    let frac = fill_h - row_f;
                    BAR_CHARS[(frac * 7.0).clamp(0.0, 7.0) as usize]
                } else {
                    BAR_CHARS[0]
                };
                if ch != ' ' {
                    for dx in 0..bar_width.min((area.x + area.width - x) as usize) {
                        buf.set_string(
                            x + dx as u16,
                            y,
                            ch.to_string(),
                            Style::default().fg(color),
                        );
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
        // Log-scale frequency mapping: 20 Hz to 16 kHz
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
