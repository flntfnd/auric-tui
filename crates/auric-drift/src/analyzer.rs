use crate::types::{AnalysisProgress, AnalyzerError, DriftFeatures};
use rayon::prelude::*;
use rustfft::{num_complex::Complex, FftPlanner};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

const FFT_SIZE: usize = 4096;
const HOP_SIZE: usize = 2048;
const MAX_ANALYZE_SECONDS: usize = 90;

pub struct DriftAnalyzer {
    _private: (),
}

impl DriftAnalyzer {
    pub fn new() -> Self {
        Self { _private: () }
    }

    pub fn analyze_file(&self, path: &Path) -> Result<DriftFeatures, AnalyzerError> {
        let sample_rate = detect_sample_rate(path)?;
        let samples = load_mono_samples(path)?;
        if samples.is_empty() {
            return Err(AnalyzerError::EmptyAudio);
        }

        let energy = compute_energy(&samples);
        let dynamic_range = compute_dynamic_range(&samples);
        let brightness = compute_spectral_centroid(&samples, sample_rate);
        let bpm = detect_bpm(&samples, sample_rate);
        let key = detect_key(&samples, sample_rate);

        Ok(DriftFeatures {
            bpm,
            key,
            energy,
            brightness,
            dynamic_range,
        })
    }

    pub fn analyze_batch(
        &self,
        paths: &[&Path],
        progress_callback: Option<Arc<dyn Fn(AnalysisProgress) + Send + Sync>>,
    ) -> Vec<(usize, Result<DriftFeatures, AnalyzerError>)> {
        if paths.is_empty() {
            return Vec::new();
        }

        let completed = Arc::new(AtomicUsize::new(0));
        let total = paths.len();

        paths
            .par_iter()
            .enumerate()
            .map(|(idx, path)| {
                let result = self.analyze_file(path);
                let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(cb) = &progress_callback {
                    cb(AnalysisProgress { completed: done, total });
                }
                (idx, result)
            })
            .collect()
    }
}

impl Default for DriftAnalyzer {
    fn default() -> Self { Self::new() }
}

fn detect_sample_rate(path: &Path) -> Result<f32, AnalyzerError> {
    let file = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| AnalyzerError::UnsupportedFormat(e.to_string()))?;

    let track = probed.format.tracks().iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| AnalyzerError::UnsupportedFormat("no audio track found".to_string()))?;

    track.codec_params.sample_rate
        .map(|sr| sr as f32)
        .ok_or_else(|| AnalyzerError::Decode("no sample rate in codec params".to_string()))
}

fn load_mono_samples(path: &Path) -> Result<Vec<f32>, AnalyzerError> {
    let file = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| AnalyzerError::UnsupportedFormat(e.to_string()))?;

    let mut format = probed.format;
    let track = format.tracks().iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| AnalyzerError::UnsupportedFormat("no audio track".to_string()))?;

    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
    let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2);
    let max_samples = sample_rate as usize * MAX_ANALYZE_SECONDS;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| AnalyzerError::Decode(e.to_string()))?;

    let mut mono_samples = Vec::with_capacity(max_samples);

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(_) => break,
        };

        if packet.track_id() != track_id { continue; }

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let spec = *decoded.spec();
        let n_frames = decoded.frames();
        let mut sample_buf = SampleBuffer::<f32>::new(n_frames as u64, spec);
        sample_buf.copy_interleaved_ref(decoded);
        let interleaved = sample_buf.samples();

        let ch = channels.max(1);
        for frame_idx in 0..n_frames {
            let offset = frame_idx * ch;
            if offset >= interleaved.len() { break; }
            let mut sum = 0.0f32;
            for c in 0..ch.min(interleaved.len() - offset) {
                sum += interleaved[offset + c];
            }
            mono_samples.push(sum / ch as f32);
        }

        if mono_samples.len() >= max_samples {
            mono_samples.truncate(max_samples);
            break;
        }
    }

    Ok(mono_samples)
}

fn compute_energy(samples: &[f32]) -> f32 {
    let sum_sq: f64 = samples.iter().map(|s| (*s as f64) * (*s as f64)).sum();
    let rms = (sum_sq / samples.len().max(1) as f64).sqrt() as f32;
    let db = 20.0 * rms.max(1e-10).log10();
    clamp_normalize(db, -60.0, 0.0)
}

fn compute_dynamic_range(samples: &[f32]) -> f32 {
    let segment_size = (samples.len() / 100).max(1024);
    let mut segment_rms: Vec<f32> = Vec::new();

    let mut offset = 0;
    while offset + segment_size <= samples.len() {
        let segment = &samples[offset..offset + segment_size];
        let sum_sq: f64 = segment.iter().map(|s| (*s as f64) * (*s as f64)).sum();
        let rms = (sum_sq / segment_size as f64).sqrt() as f32;
        if rms > 1e-8 { segment_rms.push(rms); }
        offset += segment_size;
    }

    if segment_rms.len() < 5 { return 0.5; }

    segment_rms.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let low = segment_rms[segment_rms.len() / 10];
    let high = segment_rms[segment_rms.len() * 9 / 10];

    if low <= 1e-10 { return 1.0; }

    let range_db = 20.0 * (high / low).log10();
    clamp_normalize(range_db, 0.0, 40.0)
}

fn compute_spectral_centroid(samples: &[f32], sample_rate: f32) -> f32 {
    let half_fft = FFT_SIZE / 2;
    let bin_width = sample_rate / FFT_SIZE as f32;
    let window = hann_window(FFT_SIZE);

    let mut centroid_sum = 0.0f64;
    let mut frame_count = 0u64;

    let mut offset = 0;
    while offset + FFT_SIZE <= samples.len() {
        let magnitudes = compute_fft_magnitudes(&samples[offset..offset + FFT_SIZE], &window);
        let mut weighted_sum = 0.0f64;
        let mut mag_sum = 0.0f64;
        for (bin, &mag) in magnitudes[1..half_fft].iter().enumerate() {
            let freq = (bin + 1) as f64 * bin_width as f64;
            weighted_sum += freq * mag as f64;
            mag_sum += mag as f64;
        }
        if mag_sum > 1e-10 {
            centroid_sum += weighted_sum / mag_sum;
            frame_count += 1;
        }
        offset += HOP_SIZE * 4;
    }

    if frame_count == 0 { return 0.5; }
    let avg = (centroid_sum / frame_count as f64) as f32;
    let nyquist = sample_rate / 2.0;
    clamp_normalize(avg, 200.0, nyquist * 0.6)
}

fn detect_bpm(samples: &[f32], sample_rate: f32) -> f32 {
    let half_fft = FFT_SIZE / 2;
    let window = hann_window(FFT_SIZE);
    let mut onset_strength: Vec<f32> = Vec::new();
    let mut prev_magnitudes = vec![0.0f32; half_fft];

    let mut offset = 0;
    while offset + FFT_SIZE <= samples.len() {
        let mut magnitudes = compute_fft_magnitudes(&samples[offset..offset + FFT_SIZE], &window);
        for m in &mut magnitudes { *m = m.sqrt(); }

        let mut flux = 0.0f32;
        for bin in 0..half_fft {
            let diff = magnitudes[bin] - prev_magnitudes[bin];
            if diff > 0.0 { flux += diff; }
        }
        onset_strength.push(flux);
        prev_magnitudes = magnitudes;
        offset += HOP_SIZE;
    }

    if onset_strength.len() < 16 { return 120.0; }

    let onset_rate = sample_rate / HOP_SIZE as f32;
    let min_lag = (onset_rate * 60.0 / 200.0) as usize;
    let max_lag = ((onset_rate * 60.0 / 60.0) as usize).min(onset_strength.len() / 2);

    if min_lag >= max_lag { return 120.0; }

    let mut best_lag = min_lag;
    let mut best_corr = f64::NEG_INFINITY;

    for lag in min_lag..=max_lag {
        let n = onset_strength.len() - lag;
        let corr: f64 = (0..n)
            .map(|i| onset_strength[i] as f64 * onset_strength[i + lag] as f64)
            .sum::<f64>() / n as f64;
        if corr > best_corr {
            best_corr = corr;
            best_lag = lag;
        }
    }

    let bpm = onset_rate * 60.0 / best_lag as f32;
    if bpm > 160.0 { bpm / 2.0 }
    else if bpm < 70.0 { bpm * 2.0 }
    else { bpm }
}

fn detect_key(samples: &[f32], sample_rate: f32) -> i32 {
    let half_fft = FFT_SIZE / 2;
    let bin_width = sample_rate / FFT_SIZE as f32;
    let window = hann_window(FFT_SIZE);
    let mut chromagram = [0.0f64; 12];
    let mut frame_count = 0u64;

    let mut offset = 0;
    while offset + FFT_SIZE <= samples.len() {
        let magnitudes = compute_fft_magnitudes(&samples[offset..offset + FFT_SIZE], &window);
        for (bin, &mag) in magnitudes[1..half_fft].iter().enumerate() {
            let freq = (bin + 1) as f32 * bin_width;
            if !(65.0..=2000.0).contains(&freq) { continue; }
            let note_num = 12.0 * (freq / 440.0).log2() + 69.0;
            let pitch_class = (note_num.round() as i32).rem_euclid(12) as usize;
            chromagram[pitch_class] += mag as f64;
        }
        frame_count += 1;
        offset += HOP_SIZE * 4;
    }

    if frame_count == 0 { return 0; }
    for c in &mut chromagram { *c /= frame_count as f64; }
    match_key_profile(&chromagram)
}

fn match_key_profile(chromagram: &[f64; 12]) -> i32 {
    let major: [f64; 12] = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
    let minor: [f64; 12] = [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];

    let mut best_key = 0i32;
    let mut best_corr = f64::NEG_INFINITY;

    for root in 0..12 {
        let mut rotated = [0.0f64; 12];
        for i in 0..12 { rotated[i] = chromagram[(i + root) % 12]; }

        let major_corr = pearson(&rotated, &major);
        let minor_corr = pearson(&rotated, &minor);

        if major_corr > best_corr { best_corr = major_corr; best_key = root as i32; }
        if minor_corr > best_corr { best_corr = minor_corr; best_key = root as i32 + 12; }
    }
    best_key
}

fn pearson(x: &[f64; 12], y: &[f64; 12]) -> f64 {
    let n = 12.0;
    let sum_x: f64 = x.iter().sum();
    let sum_y: f64 = y.iter().sum();
    let sum_xy: f64 = x.iter().zip(y).map(|(a, b)| a * b).sum();
    let sum_x2: f64 = x.iter().map(|a| a * a).sum();
    let sum_y2: f64 = y.iter().map(|a| a * a).sum();

    let num = n * sum_xy - sum_x * sum_y;
    let den = ((n * sum_x2 - sum_x * sum_x) * (n * sum_y2 - sum_y * sum_y)).sqrt();
    if den < 1e-10 { return 0.0; }
    num / den
}

fn compute_fft_magnitudes(frame: &[f32], window: &[f32]) -> Vec<f32> {
    let n = frame.len();
    let half = n / 2;

    let mut windowed: Vec<Complex<f32>> = frame.iter().zip(window)
        .map(|(s, w)| Complex::new(s * w, 0.0))
        .collect();

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(n);
    fft.process(&mut windowed);

    windowed[..half].iter().map(|c| c.norm_sqr()).collect()
}

fn hann_window(size: usize) -> Vec<f32> {
    (0..size).map(|i| {
        let t = std::f32::consts::PI * 2.0 * i as f32 / size as f32;
        0.5 * (1.0 - t.cos())
    }).collect()
}

pub fn clamp_normalize(value: f32, min: f32, max: f32) -> f32 {
    ((value - min) / (max - min)).clamp(0.0, 1.0)
}
