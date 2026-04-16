use std::fs::File;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

#[derive(Debug, Clone)]
pub enum PlayerCommand {
    Load { path: String },
    Pause,
    Resume,
    Stop,
    SetVolume { volume: f32 },
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum PlayerEvent {
    Playing { path: String },
    Paused,
    Resumed,
    Stopped,
    Position { position_ms: u64, duration_ms: u64 },
    TrackFinished,
    Error { message: String },
}

impl std::fmt::Debug for PlayerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerHandle").finish_non_exhaustive()
    }
}

pub struct PlayerHandle {
    cmd_tx: mpsc::Sender<PlayerCommand>,
    event_rx: Mutex<mpsc::Receiver<PlayerEvent>>,
    thread: Option<thread::JoinHandle<()>>,
    viz_buf: Arc<Mutex<Vec<f32>>>,
}

impl PlayerHandle {
    pub fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();

        let viz_buf = Arc::new(Mutex::new(Vec::new()));
        let viz_buf_clone = Arc::clone(&viz_buf);

        let thread = thread::Builder::new()
            .name("auric-player".into())
            .spawn(move || player_thread(cmd_rx, event_tx, viz_buf_clone))
            .expect("failed to spawn player thread");

        Self {
            cmd_tx,
            event_rx: Mutex::new(event_rx),
            thread: Some(thread),
            viz_buf,
        }
    }

    pub fn load(&self, path: &str) {
        let _ = self.cmd_tx.send(PlayerCommand::Load {
            path: path.to_string(),
        });
    }

    pub fn pause(&self) {
        let _ = self.cmd_tx.send(PlayerCommand::Pause);
    }

    pub fn resume(&self) {
        let _ = self.cmd_tx.send(PlayerCommand::Resume);
    }

    pub fn stop(&self) {
        let _ = self.cmd_tx.send(PlayerCommand::Stop);
    }

    pub fn set_volume(&self, volume: f32) {
        let _ = self.cmd_tx.send(PlayerCommand::SetVolume { volume });
    }

    pub fn poll_events(&self) -> Vec<PlayerEvent> {
        let rx = self.event_rx.lock().expect("event_rx lock poisoned");
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        events
    }

    pub fn peek_visualization_samples(&self, count: usize) -> Vec<f32> {
        self.viz_buf
            .lock()
            .map(|buf| {
                let start = buf.len().saturating_sub(count);
                buf[start..].to_vec()
            })
            .unwrap_or_default()
    }
}

impl Drop for PlayerHandle {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(PlayerCommand::Shutdown);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn player_thread(
    cmd_rx: mpsc::Receiver<PlayerCommand>,
    event_tx: mpsc::Sender<PlayerEvent>,
    viz_buf: Arc<Mutex<Vec<f32>>>,
) {
    let volume = Arc::new(AtomicU32::new(f32::to_bits(1.0)));

    loop {
        let cmd = match cmd_rx.recv() {
            Ok(cmd) => cmd,
            Err(_) => return,
        };

        match cmd {
            PlayerCommand::Load { path } => {
                let result = play_track(&path, &cmd_rx, &event_tx, &volume, &viz_buf);
                match result {
                    PlayResult::Finished | PlayResult::Stopped | PlayResult::Error => {}
                    PlayResult::LoadNew(new_path) => {
                        let mut current_path = new_path;
                        while let PlayResult::LoadNew(next) = play_track(
                            &current_path,
                            &cmd_rx,
                            &event_tx,
                            &volume,
                            &viz_buf,
                        ) {
                            current_path = next;
                        }
                    }
                    PlayResult::Shutdown => return,
                    PlayResult::Disconnected => return,
                }
            }
            PlayerCommand::SetVolume { volume: v } => {
                volume.store(v.to_bits(), Ordering::Relaxed);
            }
            PlayerCommand::Shutdown => return,
            _ => {}
        }
    }
}

enum PlayResult {
    Finished,
    Stopped,
    Error,
    LoadNew(String),
    Shutdown,
    Disconnected,
}

/// Linear interpolation resampling for sample rate conversion.
/// Operates on interleaved multi-channel audio.
fn resample_linear(samples: &[f32], channels: u16, ratio: f64) -> Vec<f32> {
    if (ratio - 1.0).abs() < 0.001 {
        return samples.to_vec();
    }
    let ch = channels as usize;
    let frames_in = samples.len() / ch;
    let frames_out = (frames_in as f64 * ratio).ceil() as usize;
    let mut out = Vec::with_capacity(frames_out * ch);
    for i in 0..frames_out {
        let src_pos = i as f64 / ratio;
        let src_idx = src_pos.floor() as usize;
        let frac = (src_pos - src_idx as f64) as f32;
        for c in 0..ch {
            let s0 = samples.get(src_idx * ch + c).copied().unwrap_or(0.0);
            let s1 = samples.get((src_idx + 1) * ch + c).copied().unwrap_or(s0);
            out.push(s0 + (s1 - s0) * frac);
        }
    }
    out
}

fn upmix_mono_to_stereo(samples: &[f32]) -> Vec<f32> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        out.push(s);
        out.push(s);
    }
    out
}

fn downmix_stereo_to_mono(samples: &[f32]) -> Vec<f32> {
    samples
        .chunks(2)
        .map(|pair| (pair[0] + pair.get(1).copied().unwrap_or(pair[0])) * 0.5)
        .collect()
}

fn play_track(
    path: &str,
    cmd_rx: &mpsc::Receiver<PlayerCommand>,
    event_tx: &mpsc::Sender<PlayerEvent>,
    volume: &Arc<AtomicU32>,
    viz_buf: &Arc<Mutex<Vec<f32>>>,
) -> PlayResult {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            let _ = event_tx.send(PlayerEvent::Error {
                message: format!("failed to open file: {e}"),
            });
            return PlayResult::Error;
        }
    };

    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = std::path::Path::new(path).extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = match symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    ) {
        Ok(p) => p,
        Err(e) => {
            let _ = event_tx.send(PlayerEvent::Error {
                message: format!("probe failed: {e}"),
            });
            return PlayResult::Error;
        }
    };

    let mut format = probed.format;
    let track = match format
        .default_track()
        .or_else(|| format.tracks().first())
    {
        Some(t) => t.clone(),
        None => {
            let _ = event_tx.send(PlayerEvent::Error {
                message: "no audio tracks found".into(),
            });
            return PlayResult::Error;
        }
    };

    if track.codec_params.codec == CODEC_TYPE_NULL {
        let _ = event_tx.send(PlayerEvent::Error {
            message: "unknown codec type".into(),
        });
        return PlayResult::Error;
    }

    let file_sample_rate = match track.codec_params.sample_rate {
        Some(sr) => sr,
        None => {
            let _ = event_tx.send(PlayerEvent::Error {
                message: "missing sample rate".into(),
            });
            return PlayResult::Error;
        }
    };

    let file_channels = track
        .codec_params
        .channels
        .map(|c| c.count() as u16)
        .unwrap_or(2);

    let duration_ms = track
        .codec_params
        .n_frames
        .map(|frames| frames * 1000 / file_sample_rate as u64)
        .unwrap_or(0);

    let mut decoder = match symphonia::default::get_codecs().make(
        &track.codec_params,
        &DecoderOptions::default(),
    ) {
        Ok(d) => d,
        Err(e) => {
            let _ = event_tx.send(PlayerEvent::Error {
                message: format!("decoder creation failed: {e}"),
            });
            return PlayResult::Error;
        }
    };

    let track_id = track.id;

    // Query device for its preferred output configuration
    let host = cpal::default_host();
    let device = match host.default_output_device() {
        Some(d) => d,
        None => {
            let _ = event_tx.send(PlayerEvent::Error {
                message: "no output device available".into(),
            });
            return PlayResult::Error;
        }
    };

    let default_config = match device.default_output_config() {
        Ok(c) => c,
        Err(e) => {
            let _ = event_tx.send(PlayerEvent::Error {
                message: format!("failed to query device config: {e}"),
            });
            return PlayResult::Error;
        }
    };

    let device_sample_rate = default_config.sample_rate();
    let device_channels = default_config.channels();

    let needs_resample = device_sample_rate != file_sample_rate;
    let resample_ratio = device_sample_rate as f64 / file_sample_rate as f64;

    let needs_channel_convert = file_channels != device_channels;

    // Lock-free ring buffer: ~2 seconds at the device's output rate
    // Buffer size in samples (frames * channels)
    let ring_capacity = device_sample_rate as usize * device_channels as usize * 2;
    let (mut producer, consumer) = rtrb::RingBuffer::new(ring_capacity);

    let stream_config = cpal::StreamConfig {
        channels: device_channels as cpal::ChannelCount,
        sample_rate: device_sample_rate,
        buffer_size: cpal::BufferSize::Default,
    };

    let vol_ref = Arc::clone(volume);

    // Consumer lives in the cpal callback: lock-free, allocation-free
    let mut consumer = Some(consumer);
    let stream = match device.build_output_stream(
        &stream_config,
        {
            let mut consumer = consumer.take().expect("consumer already taken");
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let vol = f32::from_bits(vol_ref.load(Ordering::Relaxed));
                for sample in data.iter_mut() {
                    *sample = consumer.pop().unwrap_or(0.0) * vol;
                }
            }
        },
        |err| {
            eprintln!("cpal stream error: {err}");
        },
        None,
    ) {
        Ok(s) => s,
        Err(e) => {
            let _ = event_tx.send(PlayerEvent::Error {
                message: format!("failed to build output stream: {e}"),
            });
            return PlayResult::Error;
        }
    };

    if let Err(e) = stream.play() {
        let _ = event_tx.send(PlayerEvent::Error {
            message: format!("failed to start playback: {e}"),
        });
        return PlayResult::Error;
    }

    let _ = event_tx.send(PlayerEvent::Playing {
        path: path.to_string(),
    });

    // Throttle threshold: 1 second of device-rate audio in the ring buffer
    let one_sec_samples = device_sample_rate as usize * device_channels as usize;
    let mut paused = false;
    let mut last_position_report = Instant::now();
    let mut decoded_samples: u64 = 0;
    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    loop {
        // Check commands
        if paused {
            match cmd_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(PlayerCommand::Resume) => {
                    paused = false;
                    let _ = stream.play();
                    let _ = event_tx.send(PlayerEvent::Resumed);
                }
                Ok(PlayerCommand::Stop) => {
                    let _ = event_tx.send(PlayerEvent::Stopped);
                    return PlayResult::Stopped;
                }
                Ok(PlayerCommand::Load { path: new_path }) => {
                    return PlayResult::LoadNew(new_path);
                }
                Ok(PlayerCommand::SetVolume { volume: v }) => {
                    volume.store(v.to_bits(), Ordering::Relaxed);
                }
                Ok(PlayerCommand::Shutdown) => return PlayResult::Shutdown,
                Ok(PlayerCommand::Pause) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return PlayResult::Disconnected,
            }
            continue;
        }

        // Non-blocking command check while playing
        match cmd_rx.try_recv() {
            Ok(PlayerCommand::Pause) => {
                paused = true;
                let _ = stream.pause();
                let _ = event_tx.send(PlayerEvent::Paused);
                continue;
            }
            Ok(PlayerCommand::Stop) => {
                let _ = event_tx.send(PlayerEvent::Stopped);
                return PlayResult::Stopped;
            }
            Ok(PlayerCommand::Load { path: new_path }) => {
                return PlayResult::LoadNew(new_path);
            }
            Ok(PlayerCommand::SetVolume { volume: v }) => {
                volume.store(v.to_bits(), Ordering::Relaxed);
            }
            Ok(PlayerCommand::Shutdown) => return PlayResult::Shutdown,
            Ok(PlayerCommand::Resume) => {}
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => return PlayResult::Disconnected,
        }

        // Throttle if ring buffer has more than 1 second of audio
        let available = ring_capacity - producer.slots();
        if available > one_sec_samples {
            thread::sleep(Duration::from_millis(10));
            continue;
        }

        // Decode next packet
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                // EOF: wait for ring buffer to drain, then signal track finished
                loop {
                    let buffered = ring_capacity - producer.slots();
                    if buffered == 0 {
                        break;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                let _ = event_tx.send(PlayerEvent::TrackFinished);
                return PlayResult::Finished;
            }
            Err(e) => {
                let _ = event_tx.send(PlayerEvent::Error {
                    message: format!("format read error: {e}"),
                });
                return PlayResult::Error;
            }
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(symphonia::core::errors::Error::DecodeError(msg)) => {
                eprintln!("decode error (skipping): {msg}");
                continue;
            }
            Err(e) => {
                let _ = event_tx.send(PlayerEvent::Error {
                    message: format!("decode error: {e}"),
                });
                return PlayResult::Error;
            }
        };

        let spec = *decoded.spec();
        let num_frames = decoded.frames();

        let sbuf = match &mut sample_buf {
            Some(existing) if existing.capacity() >= num_frames => existing,
            _ => {
                sample_buf = Some(SampleBuffer::<f32>::new(num_frames as u64, spec));
                sample_buf.as_mut().unwrap()
            }
        };
        sbuf.copy_interleaved_ref(decoded);
        let raw_samples = sbuf.samples();

        // Count pre-resample frames for accurate position tracking
        decoded_samples += num_frames as u64;

        // Resample if the device sample rate differs from the file
        let resampled;
        let after_resample = if needs_resample {
            resampled = resample_linear(raw_samples, file_channels, resample_ratio);
            &resampled
        } else {
            raw_samples
        };

        // Channel conversion: match file channels to device channels
        let converted;
        let final_samples = if needs_channel_convert {
            if file_channels == 1 && device_channels == 2 {
                converted = upmix_mono_to_stereo(after_resample);
                &converted
            } else if file_channels == 2 && device_channels == 1 {
                converted = downmix_stereo_to_mono(after_resample);
                &converted
            } else {
                after_resample
            }
        } else {
            after_resample
        };

        // Push processed samples into the lock-free ring buffer
        for &sample in final_samples {
            while producer.push(sample).is_err() {
                std::thread::sleep(Duration::from_millis(1));
            }
        }

        // Store latest samples for visualization (capped at 2048 samples)
        if let Ok(mut vb) = viz_buf.lock() {
            vb.clear();
            if raw_samples.len() <= 2048 {
                vb.extend_from_slice(raw_samples);
            } else {
                vb.extend_from_slice(&raw_samples[raw_samples.len() - 2048..]);
            }
        }

        // Send position updates at ~12fps for smooth visualizer
        if last_position_report.elapsed() >= Duration::from_millis(80) {
            let position_ms = decoded_samples * 1000 / file_sample_rate as u64;
            let _ = event_tx.send(PlayerEvent::Position {
                position_ms,
                duration_ms,
            });
            last_position_report = Instant::now();
        }
    }
}
