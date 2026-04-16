use std::collections::VecDeque;
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
        // Idle state: block waiting for commands
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
                        // Re-enter the load path iteratively instead of recursing
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
            // Ignore pause/resume/stop when idle
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

    let sample_rate = match track.codec_params.sample_rate {
        Some(sr) => sr,
        None => {
            let _ = event_tx.send(PlayerEvent::Error {
                message: "missing sample rate".into(),
            });
            return PlayResult::Error;
        }
    };

    let channels = track
        .codec_params
        .channels
        .map(|c| c.count() as u16)
        .unwrap_or(2);

    let duration_ms = track
        .codec_params
        .n_frames
        .map(|frames| frames * 1000 / sample_rate as u64)
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

    // Shared sample buffer between decode thread and cpal callback.
    // Capacity: 2 seconds of audio. The decode loop throttles at 1 second,
    // so the buffer should not grow much beyond this.
    let buffer: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::with_capacity(
        sample_rate as usize * channels as usize * 2,
    )));

    // Build cpal output stream
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

    let stream_config = cpal::StreamConfig {
        channels: channels as cpal::ChannelCount,
        sample_rate,
        buffer_size: cpal::BufferSize::Default,
    };

    let buf_ref = Arc::clone(&buffer);
    let vol_ref = Arc::clone(volume);

    let stream = match device.build_output_stream(
        &stream_config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let vol = f32::from_bits(vol_ref.load(Ordering::Relaxed));
            let mut buf = buf_ref.lock().expect("audio buffer lock poisoned");
            for sample in data.iter_mut() {
                *sample = buf.pop_front().unwrap_or(0.0) * vol;
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

    let one_sec_samples = sample_rate as usize * channels as usize;
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

        // Throttle if buffer has more than 1 second of audio
        {
            let buf = buffer.lock().expect("audio buffer lock poisoned");
            if buf.len() > one_sec_samples {
                drop(buf);
                thread::sleep(Duration::from_millis(10));
                continue;
            }
        }

        // Decode next packet
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                // EOF: wait for buffer to drain, then signal track finished
                loop {
                    {
                        let buf = buffer.lock().expect("audio buffer lock poisoned");
                        if buf.is_empty() {
                            break;
                        }
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
                // Non-fatal: skip this packet
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

        // Reuse sample buffer across packets when the format hasn't changed,
        // avoiding a heap allocation per packet.
        let sbuf = match &mut sample_buf {
            Some(existing) if existing.capacity() >= num_frames => existing,
            _ => {
                sample_buf = Some(SampleBuffer::<f32>::new(num_frames as u64, spec));
                sample_buf.as_mut().unwrap()
            }
        };
        sbuf.copy_interleaved_ref(decoded);
        let samples = sbuf.samples();

        decoded_samples += num_frames as u64;

        {
            let mut buf = buffer.lock().expect("audio buffer lock poisoned");
            buf.extend(samples);
        }

        // Store latest samples for visualization (capped at 2048 samples)
        if let Ok(mut vb) = viz_buf.lock() {
            vb.clear();
            if samples.len() <= 2048 {
                vb.extend_from_slice(samples);
            } else {
                vb.extend_from_slice(&samples[samples.len() - 2048..]);
            }
        }

        // Send position updates roughly every 250ms
        if last_position_report.elapsed() >= Duration::from_millis(250) {
            let position_ms = decoded_samples * 1000 / sample_rate as u64;
            let _ = event_tx.send(PlayerEvent::Position {
                position_ms,
                duration_ms,
            });
            last_position_report = Instant::now();
        }
    }
}
