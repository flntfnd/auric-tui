use std::sync::{Arc, Mutex};

use rustfft::{num_complex::Complex, FftPlanner};

/// Number of FFT bins (must be power of 2)
const FFT_SIZE: usize = 2048;

/// Number of output bars for display
const NUM_BARS: usize = 32;

/// Smoothing factor for bar decay (0.0 = instant, 1.0 = no decay)
const SMOOTHING: f32 = 0.7;

/// Ring buffer for collecting audio samples
pub struct SampleBuffer {
    samples: Vec<f32>,
    write_pos: usize,
}

impl SampleBuffer {
    pub fn new() -> Self {
        Self {
            samples: vec![0.0; FFT_SIZE],
            write_pos: 0,
        }
    }

    pub fn push(&mut self, sample: f32) {
        self.samples[self.write_pos] = sample;
        self.write_pos = (self.write_pos + 1) % FFT_SIZE;
    }

    pub fn get_samples(&self) -> Vec<f32> {
        // Return samples in order (oldest to newest)
        let mut result = Vec::with_capacity(FFT_SIZE);
        for i in 0..FFT_SIZE {
            let idx = (self.write_pos + i) % FFT_SIZE;
            result.push(self.samples[idx]);
        }
        result
    }
}

impl Default for SampleBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// Spectrum analyzer that performs FFT and returns frequency magnitudes
pub struct SpectrumAnalyzer {
    sample_buffer: Arc<Mutex<SampleBuffer>>,
    fft_planner: FftPlanner<f32>,
    smoothed_bars: Vec<f32>,
    window: Vec<f32>,
}

impl SpectrumAnalyzer {
    pub fn new(sample_buffer: Arc<Mutex<SampleBuffer>>) -> Self {
        // Pre-compute Hann window for smoother FFT
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| {
                let x = std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32;
                0.5 * (1.0 - (2.0 * x).cos())
            })
            .collect();

        Self {
            sample_buffer,
            fft_planner: FftPlanner::new(),
            smoothed_bars: vec![0.0; NUM_BARS],
            window,
        }
    }

    /// Analyze the current audio buffer and return bar magnitudes (0.0 to 1.0)
    pub fn analyze(&mut self) -> Vec<f32> {
        let samples = {
            let buffer = self.sample_buffer.lock().unwrap();
            buffer.get_samples()
        };

        // Apply window function
        let mut windowed: Vec<Complex<f32>> = samples
            .iter()
            .zip(self.window.iter())
            .map(|(s, w)| Complex::new(s * w, 0.0))
            .collect();

        // Perform FFT
        let fft = self.fft_planner.plan_fft_forward(FFT_SIZE);
        fft.process(&mut windowed);

        // Calculate magnitudes (only first half - positive frequencies)
        let half = FFT_SIZE / 2;
        let magnitudes: Vec<f32> = windowed[..half]
            .iter()
            .map(|c| c.norm() / (FFT_SIZE as f32).sqrt())
            .collect();

        // Map frequency bins to display bars using logarithmic scale
        // This gives more resolution to lower frequencies (where music has more content)
        let mut bars = vec![0.0f32; NUM_BARS];

        for bar_idx in 0..NUM_BARS {
            // Logarithmic frequency mapping
            let low_freq = Self::bar_to_freq(bar_idx, NUM_BARS);
            let high_freq = Self::bar_to_freq(bar_idx + 1, NUM_BARS);

            // Convert frequencies to FFT bin indices
            let low_bin = (low_freq * FFT_SIZE as f32 / 44100.0) as usize;
            let high_bin = ((high_freq * FFT_SIZE as f32 / 44100.0) as usize).min(half - 1);

            if low_bin < half && low_bin <= high_bin {
                // Average the magnitudes in this frequency range
                let sum: f32 = magnitudes[low_bin..=high_bin].iter().sum();
                let count = (high_bin - low_bin + 1) as f32;
                bars[bar_idx] = sum / count;
            }
        }

        // Apply logarithmic scaling for better visual representation
        for bar in &mut bars {
            if *bar > 0.0 {
                // Convert to dB scale, clamp, and normalize
                let db = 20.0 * bar.log10();
                // Map -60dB to 0dB range to 0.0 to 1.0
                *bar = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
            }
        }

        // Apply smoothing (exponential moving average for decay effect)
        for (i, bar) in bars.iter().enumerate() {
            self.smoothed_bars[i] = if *bar > self.smoothed_bars[i] {
                // Fast attack
                *bar
            } else {
                // Slow decay
                self.smoothed_bars[i] * SMOOTHING + bar * (1.0 - SMOOTHING)
            };
        }

        self.smoothed_bars.clone()
    }

    /// Convert bar index to frequency using logarithmic scale
    fn bar_to_freq(bar: usize, num_bars: usize) -> f32 {
        const MIN_FREQ: f32 = 20.0;
        const MAX_FREQ: f32 = 20000.0;

        let t = bar as f32 / num_bars as f32;
        MIN_FREQ * (MAX_FREQ / MIN_FREQ).powf(t)
    }
}

/// Shared spectrum data for thread-safe access
#[derive(Clone)]
pub struct SharedSpectrum {
    bars: Arc<Mutex<Vec<f32>>>,
    sample_buffer: Arc<Mutex<SampleBuffer>>,
}

impl SharedSpectrum {
    pub fn new() -> Self {
        Self {
            bars: Arc::new(Mutex::new(vec![0.0; NUM_BARS])),
            sample_buffer: Arc::new(Mutex::new(SampleBuffer::new())),
        }
    }

    /// Get the sample buffer for audio input
    pub fn sample_buffer(&self) -> Arc<Mutex<SampleBuffer>> {
        Arc::clone(&self.sample_buffer)
    }

    /// Push a sample into the buffer
    pub fn push_sample(&self, sample: f32) {
        if let Ok(mut buffer) = self.sample_buffer.lock() {
            buffer.push(sample);
        }
    }

    /// Clear the spectrum (when playback stops)
    pub fn clear(&self) {
        if let Ok(mut bars) = self.bars.lock() {
            bars.fill(0.0);
        }
    }
}

impl Default for SharedSpectrum {
    fn default() -> Self {
        Self::new()
    }
}
