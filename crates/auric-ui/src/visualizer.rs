use crate::theme::Palette;
use ratatui::prelude::*;
use ratatui::widgets::Widget;
use rustfft::{num_complex::Complex, FftPlanner};

const BRAILLE_BASE: u32 = 0x2800;
const DOT_MAP: [[u8; 4]; 2] = [
    [0x01, 0x02, 0x04, 0x40],
    [0x08, 0x10, 0x20, 0x80],
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualizerStyle {
    Spectrum,
    Oscilloscope,
    Mirror,
    Scatter,
    Wave,
    Fire,
}

impl VisualizerStyle {
    pub fn next(self) -> Self {
        match self {
            Self::Spectrum => Self::Oscilloscope,
            Self::Oscilloscope => Self::Mirror,
            Self::Mirror => Self::Scatter,
            Self::Scatter => Self::Wave,
            Self::Wave => Self::Fire,
            Self::Fire => Self::Spectrum,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Spectrum => "spectrum",
            Self::Oscilloscope => "oscilloscope",
            Self::Mirror => "mirror",
            Self::Scatter => "scatter",
            Self::Wave => "wave",
            Self::Fire => "fire",
        }
    }
}

pub struct VisualizerWidget<'a> {
    pub style: VisualizerStyle,
    pub bands: &'a [f32],
    pub samples: &'a [f32],
    pub palette: &'a Palette,
    pub frame_count: u64,
    pub fire_history: &'a [Vec<f32>],
}

impl<'a> Widget for VisualizerWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 2 || area.height < 1 {
            return;
        }
        match self.style {
            VisualizerStyle::Spectrum => render_spectrum(area, buf, self.bands, self.palette),
            VisualizerStyle::Oscilloscope => {
                render_oscilloscope(area, buf, self.samples, self.palette)
            }
            VisualizerStyle::Mirror => render_mirror(area, buf, self.bands, self.palette),
            VisualizerStyle::Scatter => {
                render_scatter(area, buf, self.bands, self.palette, self.frame_count)
            }
            VisualizerStyle::Wave => {
                render_wave(area, buf, self.bands, self.palette, self.frame_count)
            }
            VisualizerStyle::Fire => {
                render_fire(area, buf, self.fire_history, self.palette)
            }
        }
    }
}

// Helper: set a braille dot at virtual pixel (px, py) within a dot grid
fn set_dot(dots: &mut [u8], dot_cols: usize, px: usize, py: usize) {
    let cx = px / 2;
    let cy = py / 4;
    let dx = px % 2;
    let dy = py % 4;
    let idx = cy * dot_cols.div_ceil(2) + cx;
    if idx < dots.len() {
        dots[idx] |= DOT_MAP[dx][dy];
    }
}

// Helper: render a braille dot grid to the buffer
fn flush_dots(
    dots: &[u8],
    area: Rect,
    buf: &mut Buffer,
    color_fn: &dyn Fn(u16, u16) -> Color,
) {
    let cols = area.width as usize;
    for cy in 0..area.height as usize {
        for cx in 0..cols {
            let idx = cy * cols + cx;
            let pattern = if idx < dots.len() { dots[idx] } else { 0 };
            if pattern != 0 {
                let ch = char::from_u32(BRAILLE_BASE + pattern as u32).unwrap_or(' ');
                let x = area.x + cx as u16;
                let y = area.y + cy as u16;
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_char(ch);
                    cell.set_fg(color_fn(x - area.x, y - area.y));
                }
            }
        }
    }
}

// Helper: get frequency band color
fn band_color(band_idx: usize, num_bands: usize, palette: &Palette) -> Color {
    if band_idx < num_bands / 3 {
        palette.visualizer_low
    } else if band_idx < 2 * num_bands / 3 {
        palette.visualizer_mid
    } else {
        palette.visualizer_high
    }
}

// --- Style: Spectrum (bars with gaps) ---
fn render_spectrum(area: Rect, buf: &mut Buffer, bands: &[f32], palette: &Palette) {
    if bands.is_empty() {
        return;
    }
    let dot_cols = area.width as usize * 2;
    let dot_rows = area.height as usize * 4;
    let num_bands = bands.len();
    let total_gaps = num_bands.saturating_sub(1);
    let usable = dot_cols.saturating_sub(total_gaps);
    let bw = (usable / num_bands).max(1);

    let mut dots = vec![0u8; area.width as usize * area.height as usize];
    for (i, &mag) in bands.iter().enumerate() {
        let col_start = i * (bw + 1);
        let fill = (mag.clamp(0.0, 1.0) * dot_rows as f32).round() as usize;
        for dc in 0..bw {
            let col = col_start + dc;
            if col >= dot_cols {
                break;
            }
            for dr in 0..fill {
                set_dot(&mut dots, dot_cols, col, dot_rows - 1 - dr);
            }
        }
    }
    flush_dots(&dots, area, buf, &|x, _| {
        let bi = x as usize * 2 * num_bands / dot_cols.max(1);
        band_color(bi, num_bands, palette)
    });
}

// --- Style: Oscilloscope (waveform trace) ---
fn render_oscilloscope(area: Rect, buf: &mut Buffer, samples: &[f32], palette: &Palette) {
    if samples.is_empty() {
        return;
    }
    let dot_cols = area.width as usize * 2;
    let dot_rows = area.height as usize * 4;
    let center = dot_rows / 2;
    let n = samples.len();

    let mut dots = vec![0u8; area.width as usize * area.height as usize];

    let mut prev_y: Option<usize> = None;
    for px in 0..dot_cols {
        let si = px * n / dot_cols;
        let s = samples[si.min(n - 1)];
        let py = (center as f32 - s * center as f32 * 0.9).clamp(0.0, (dot_rows - 1) as f32) as usize;

        // Bresenham-style line from previous point
        if let Some(prev) = prev_y {
            let (y0, y1) = if prev < py { (prev, py) } else { (py, prev) };
            for y in y0..=y1 {
                set_dot(&mut dots, dot_cols, px, y);
            }
        } else {
            set_dot(&mut dots, dot_cols, px, py);
        }
        prev_y = Some(py);
    }

    flush_dots(&dots, area, buf, &|_, _| palette.accent);
}

// --- Style: Mirror (symmetric spectrum from center) ---
fn render_mirror(area: Rect, buf: &mut Buffer, bands: &[f32], palette: &Palette) {
    if bands.is_empty() {
        return;
    }
    let dot_cols = area.width as usize * 2;
    let dot_rows = area.height as usize * 4;
    let center = dot_rows / 2;
    let num_bands = bands.len();
    let total_gaps = num_bands.saturating_sub(1);
    let usable = dot_cols.saturating_sub(total_gaps);
    let bw = (usable / num_bands).max(1);

    let mut dots = vec![0u8; area.width as usize * area.height as usize];
    for (i, &mag) in bands.iter().enumerate() {
        let col_start = i * (bw + 1);
        let fill = (mag.clamp(0.0, 1.0) * center as f32).round() as usize;
        for dc in 0..bw {
            let col = col_start + dc;
            if col >= dot_cols {
                break;
            }
            // Up from center
            for dr in 0..fill {
                if center > dr {
                    set_dot(&mut dots, dot_cols, col, center - 1 - dr);
                }
            }
            // Down from center
            for dr in 0..fill {
                if center + dr < dot_rows {
                    set_dot(&mut dots, dot_cols, col, center + dr);
                }
            }
        }
    }
    flush_dots(&dots, area, buf, &|x, _| {
        let bi = x as usize * 2 * num_bands / dot_cols.max(1);
        band_color(bi, num_bands, palette)
    });
}

// --- Style: Scatter (particles driven by frequency energy) ---
fn render_scatter(
    area: Rect,
    buf: &mut Buffer,
    bands: &[f32],
    palette: &Palette,
    frame: u64,
) {
    if bands.is_empty() {
        return;
    }
    let dot_cols = area.width as usize * 2;
    let dot_rows = area.height as usize * 4;
    let num_bands = bands.len();

    let mut dots = vec![0u8; area.width as usize * area.height as usize];

    // Deterministic pseudo-random scatter based on band energy + frame
    for (i, &mag) in bands.iter().enumerate() {
        let num_dots = (mag * 12.0).round() as usize;
        let band_x = i * dot_cols / num_bands;
        let spread = (dot_cols / num_bands).max(2);

        for d in 0..num_dots {
            // Use frame + band + dot index for pseudo-random positioning
            let seed = (frame.wrapping_mul(31).wrapping_add(i as u64 * 97).wrapping_add(d as u64 * 53)) as usize;
            let px = band_x + (seed % spread);
            let height = (mag * dot_rows as f32 * 0.8) as usize;
            let py = dot_rows - 1 - (seed / 3 % height.max(1));
            if px < dot_cols && py < dot_rows {
                set_dot(&mut dots, dot_cols, px, py);
            }
        }
    }
    flush_dots(&dots, area, buf, &|x, _| {
        let bi = x as usize * 2 * num_bands / dot_cols.max(1);
        band_color(bi, num_bands, palette)
    });
}

// --- Style: Wave (modulated sine wave) ---
fn render_wave(
    area: Rect,
    buf: &mut Buffer,
    bands: &[f32],
    palette: &Palette,
    frame: u64,
) {
    if bands.is_empty() {
        return;
    }
    let dot_cols = area.width as usize * 2;
    let dot_rows = area.height as usize * 4;
    let center = dot_rows / 2;
    let num_bands = bands.len();

    // Overall energy drives amplitude
    let energy: f32 = bands.iter().sum::<f32>() / num_bands as f32;
    let time = frame as f32 * 0.15;

    let mut dots = vec![0u8; area.width as usize * area.height as usize];

    let mut prev_y: Option<usize> = None;
    for px in 0..dot_cols {
        let t = px as f32 / dot_cols as f32;
        // Band-modulated amplitude at this x position
        let bi = (t * num_bands as f32) as usize;
        let local_mag = bands.get(bi).copied().unwrap_or(energy);

        let amplitude = (0.2 + local_mag * 0.7) * center as f32;
        let phase = t * std::f32::consts::PI * 4.0 + time;
        let py = (center as f32 + amplitude * phase.sin())
            .clamp(0.0, (dot_rows - 1) as f32) as usize;

        if let Some(prev) = prev_y {
            let (y0, y1) = if prev < py { (prev, py) } else { (py, prev) };
            for y in y0..=y1 {
                set_dot(&mut dots, dot_cols, px, y);
            }
        } else {
            set_dot(&mut dots, dot_cols, px, py);
        }
        prev_y = Some(py);
    }
    flush_dots(&dots, area, buf, &|x, _| {
        let bi = x as usize * 2 * num_bands / dot_cols.max(1);
        band_color(bi, num_bands, palette)
    });
}

// --- Style: Fire (scrolling spectrogram) ---
fn render_fire(
    area: Rect,
    buf: &mut Buffer,
    history: &[Vec<f32>],
    palette: &Palette,
) {
    let dot_cols = area.width as usize * 2;
    let dot_rows = area.height as usize * 4;
    if history.is_empty() {
        return;
    }

    let num_bands = history[0].len().max(1);
    let num_rows = history.len();

    let mut dots = vec![0u8; area.width as usize * area.height as usize];

    // Map history rows to dot rows (history[0] = newest = bottom)
    for (hi, row) in history.iter().enumerate() {
        let dot_y_base = dot_rows.saturating_sub((hi + 1) * dot_rows / num_rows.max(1));
        let dot_y_end = dot_rows.saturating_sub(hi * dot_rows / num_rows.max(1));

        for (bi, &mag) in row.iter().enumerate() {
            if mag < 0.05 {
                continue;
            }
            let col_start = bi * dot_cols / num_bands;
            let col_end = ((bi + 1) * dot_cols / num_bands).min(dot_cols);
            for col in col_start..col_end {
                for dy in dot_y_base..dot_y_end {
                    if dy < dot_rows {
                        set_dot(&mut dots, dot_cols, col, dy);
                    }
                }
            }
        }
    }

    flush_dots(&dots, area, buf, &|_, y| {
        // Color gradient: bottom (hot) to top (cool)
        let t = y as f32 / area.height as f32;
        if t > 0.7 {
            palette.visualizer_high
        } else if t > 0.3 {
            palette.visualizer_mid
        } else {
            palette.visualizer_low
        }
    });
}

// --- FFT Analysis ---

pub fn analyze_spectrum(samples: &[f32], num_bands: usize) -> Vec<f32> {
    if samples.is_empty() || num_bands == 0 {
        return vec![0.0; num_bands];
    }

    let fft_size = 1024;

    let mut buffer: Vec<Complex<f32>> = (0..fft_size)
        .map(|i| {
            let sample = samples.get(i).copied().unwrap_or(0.0);
            let window =
                0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / fft_size as f32).cos());
            Complex::new(sample * window, 0.0)
        })
        .collect();

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);
    fft.process(&mut buffer);

    let magnitudes: Vec<f32> = buffer[..fft_size / 2]
        .iter()
        .map(|c| (c.re * c.re + c.im * c.im).sqrt() / fft_size as f32)
        .collect();

    let mut bands = vec![0.0f32; num_bands];
    for (band_idx, band_val) in bands.iter_mut().enumerate() {
        let freq_lo =
            20.0 * (16000.0f32 / 20.0).powf(band_idx as f32 / num_bands as f32);
        let freq_hi =
            20.0 * (16000.0f32 / 20.0).powf((band_idx + 1) as f32 / num_bands as f32);
        let bin_lo = (freq_lo * fft_size as f32 / 44100.0) as usize;
        let bin_hi = ((freq_hi * fft_size as f32 / 44100.0) as usize)
            .min(magnitudes.len())
            .max(bin_lo + 1);

        let slice = &magnitudes[bin_lo..bin_hi.min(magnitudes.len())];
        let sum: f32 = slice.iter().sum();
        let count = slice.len();
        if count > 0 {
            *band_val = (sum / count as f32 * 12.0).clamp(0.0, 1.0);
        }
    }

    bands
}

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
