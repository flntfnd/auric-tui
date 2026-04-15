# Audio Playback Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add real audio playback to auric -- decode files with symphonia, output via cpal, controlled through TUI keybindings with real-time position updates.

**Architecture:** A `PlayerHandle` communicates with a background player thread via mpsc channels. The thread decodes audio with symphonia and outputs via cpal. The app layer manages the queue and advances tracks. The TUI polls player events each tick and maps keybindings to player commands.

**Tech Stack:** Rust, symphonia (decode), cpal (output), mpsc channels (thread communication)

---

## File Structure

### New files
- `crates/auric-audio/src/player.rs` -- PlayerHandle, PlayerCommand, PlayerEvent, background thread, decode+output loop

### Modified files
- `crates/auric-audio/src/lib.rs` -- add `pub mod player`, re-export types
- `crates/auric-audio/Cargo.toml` -- no changes needed (symphonia + cpal already present)
- `crates/auric-ui/src/shell.rs` -- playback fields in ShellSnapshot/ShellState, keybindings, now playing render, player event polling
- `crates/auric-ui/src/lib.rs` -- export new types
- `crates/auric-app/src/lib.rs` -- PlayerHandle in BootstrappedApp, queue building, track advancement, wire into TUI handlers

---

### Task 1: Player Module -- Types and PlayerHandle

**Files:**
- Create: `crates/auric-audio/src/player.rs`
- Modify: `crates/auric-audio/src/lib.rs`

- [ ] **Step 1: Create player.rs with types and PlayerHandle**

```rust
// crates/auric-audio/src/player.rs

use std::collections::VecDeque;
use std::fs::File;
use std::path::Path;
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
    Seek { position_ms: u64 },
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

pub struct PlayerHandle {
    cmd_tx: mpsc::Sender<PlayerCommand>,
    evt_rx: mpsc::Receiver<PlayerEvent>,
    thread: Option<thread::JoinHandle<()>>,
}

impl PlayerHandle {
    pub fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (evt_tx, evt_rx) = mpsc::channel();

        let thread = thread::Builder::new()
            .name("auric-player".to_string())
            .spawn(move || {
                player_thread(cmd_rx, evt_tx);
            })
            .expect("failed to spawn player thread");

        Self {
            cmd_tx,
            evt_rx,
            thread: Some(thread),
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

    pub fn seek(&self, position_ms: u64) {
        let _ = self.cmd_tx.send(PlayerCommand::Seek { position_ms });
    }

    pub fn set_volume(&self, volume: f32) {
        let _ = self
            .cmd_tx
            .send(PlayerCommand::SetVolume { volume: volume.clamp(0.0, 1.0) });
    }

    pub fn poll_events(&self) -> Vec<PlayerEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.evt_rx.try_recv() {
            events.push(event);
        }
        events
    }
}

impl Drop for PlayerHandle {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(PlayerCommand::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
```

- [ ] **Step 2: Add the player thread function (decode + output)**

Add this to the same file, below `PlayerHandle`:

```rust
fn player_thread(cmd_rx: mpsc::Receiver<PlayerCommand>, evt_tx: mpsc::Sender<PlayerEvent>) {
    let mut state = ThreadState::Idle;
    let mut volume: f32 = 1.0;

    loop {
        match &mut state {
            ThreadState::Idle => {
                // Block waiting for a command
                match cmd_rx.recv() {
                    Ok(PlayerCommand::Load { path }) => {
                        state = match start_playback(&path, volume, &evt_tx) {
                            Ok(s) => s,
                            Err(err) => {
                                let _ = evt_tx.send(PlayerEvent::Error {
                                    message: err.to_string(),
                                });
                                ThreadState::Idle
                            }
                        };
                    }
                    Ok(PlayerCommand::SetVolume { volume: v }) => {
                        volume = v;
                    }
                    Ok(PlayerCommand::Shutdown) | Err(_) => return,
                    Ok(_) => {} // ignore pause/resume/stop/seek when idle
                }
            }
            ThreadState::Playing(active) => {
                // Non-blocking check for commands
                match cmd_rx.try_recv() {
                    Ok(PlayerCommand::Pause) => {
                        active.stream.pause().ok();
                        active.paused = true;
                        let _ = evt_tx.send(PlayerEvent::Paused);
                    }
                    Ok(PlayerCommand::Resume) => {
                        active.stream.play().ok();
                        active.paused = false;
                        let _ = evt_tx.send(PlayerEvent::Resumed);
                    }
                    Ok(PlayerCommand::Stop) => {
                        drop(state);
                        state = ThreadState::Idle;
                        let _ = evt_tx.send(PlayerEvent::Stopped);
                        continue;
                    }
                    Ok(PlayerCommand::Load { path }) => {
                        drop(state);
                        state = match start_playback(&path, volume, &evt_tx) {
                            Ok(s) => s,
                            Err(err) => {
                                let _ = evt_tx.send(PlayerEvent::Error {
                                    message: err.to_string(),
                                });
                                ThreadState::Idle
                            }
                        };
                        continue;
                    }
                    Ok(PlayerCommand::SetVolume { volume: v }) => {
                        volume = v;
                        active.volume.store(
                            v.to_bits(),
                            std::sync::atomic::Ordering::Relaxed,
                        );
                    }
                    Ok(PlayerCommand::Shutdown) => return,
                    Ok(PlayerCommand::Seek { .. }) => {
                        // Seek not implemented in v1
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                    Err(mpsc::TryRecvError::Disconnected) => return,
                }

                if active.paused {
                    thread::sleep(Duration::from_millis(50));
                    continue;
                }

                // Decode next packet and fill buffer
                match decode_next_packet(active) {
                    DecodeResult::Ok => {
                        // Send position update periodically
                        if active.last_position_send.elapsed() >= Duration::from_millis(250) {
                            let position_ms = samples_to_ms(
                                active.samples_decoded,
                                active.sample_rate,
                                active.channels,
                            );
                            let _ = evt_tx.send(PlayerEvent::Position {
                                position_ms,
                                duration_ms: active.duration_ms,
                            });
                            active.last_position_send = Instant::now();
                        }

                        // Throttle decoding if buffer is full enough
                        let buf_len = active.buffer.lock().map(|b| b.len()).unwrap_or(0);
                        let target = (active.sample_rate as usize) * (active.channels as usize);
                        if buf_len > target {
                            thread::sleep(Duration::from_millis(10));
                        }
                    }
                    DecodeResult::Finished => {
                        // Wait for buffer to drain before signaling finished
                        loop {
                            let buf_len =
                                active.buffer.lock().map(|b| b.len()).unwrap_or(0);
                            if buf_len == 0 {
                                break;
                            }
                            thread::sleep(Duration::from_millis(20));
                            // Check for stop command while draining
                            if let Ok(cmd) = cmd_rx.try_recv() {
                                match cmd {
                                    PlayerCommand::Stop | PlayerCommand::Shutdown => {
                                        drop(state);
                                        state = ThreadState::Idle;
                                        let _ = evt_tx.send(PlayerEvent::Stopped);
                                        if matches!(cmd, PlayerCommand::Shutdown) {
                                            return;
                                        }
                                        continue;
                                    }
                                    PlayerCommand::Load { path } => {
                                        drop(state);
                                        state = match start_playback(&path, volume, &evt_tx) {
                                            Ok(s) => s,
                                            Err(err) => {
                                                let _ = evt_tx.send(PlayerEvent::Error {
                                                    message: err.to_string(),
                                                });
                                                ThreadState::Idle
                                            }
                                        };
                                        continue;
                                    }
                                    _ => {}
                                }
                            }
                        }
                        let _ = evt_tx.send(PlayerEvent::TrackFinished);
                        state = ThreadState::Idle;
                    }
                    DecodeResult::Error(msg) => {
                        let _ = evt_tx.send(PlayerEvent::Error { message: msg });
                        state = ThreadState::Idle;
                    }
                }
            }
        }
    }
}

enum ThreadState {
    Idle,
    Playing(ActivePlayback),
}

// Need manual Drop to avoid "cannot move out of state" issues
impl Drop for ThreadState {
    fn drop(&mut self) {}
}

struct ActivePlayback {
    buffer: Arc<Mutex<VecDeque<f32>>>,
    volume: Arc<std::sync::atomic::AtomicU32>,
    stream: cpal::Stream,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    format_reader: Box<dyn symphonia::core::formats::FormatReader>,
    track_id: u32,
    sample_rate: u32,
    channels: u16,
    duration_ms: u64,
    samples_decoded: u64,
    paused: bool,
    last_position_send: Instant,
}

enum DecodeResult {
    Ok,
    Finished,
    Error(String),
}

fn start_playback(
    path: &str,
    volume: f32,
    evt_tx: &mpsc::Sender<PlayerEvent>,
) -> Result<ThreadState, String> {
    // Open and probe the file
    let file = File::open(path).map_err(|e| format!("failed to open {path}: {e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = Path::new(path).extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("probe failed for {path}: {e}"))?;

    let mut format_reader = probed.format;
    let track = format_reader
        .default_track()
        .or_else(|| format_reader.tracks().first())
        .ok_or_else(|| format!("no audio track in {path}"))?;

    if track.codec_params.codec == CODEC_TYPE_NULL {
        return Err(format!("unknown codec in {path}"));
    }

    let track_id = track.id;
    let codec_params = track.codec_params.clone();
    let sample_rate = codec_params.sample_rate.unwrap_or(44100);
    let channels = codec_params
        .channels
        .map(|c| c.count() as u16)
        .unwrap_or(2);

    // Calculate duration from track metadata
    let duration_ms = codec_params
        .n_frames
        .map(|frames| (frames as f64 / sample_rate as f64 * 1000.0) as u64)
        .unwrap_or(0);

    let decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| format!("decoder init failed: {e}"))?;

    // Set up cpal output
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "no output device available".to_string())?;

    let buffer: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::with_capacity(
        sample_rate as usize * channels as usize * 2,
    )));
    let volume_atomic = Arc::new(std::sync::atomic::AtomicU32::new(volume.to_bits()));

    let buf_clone = Arc::clone(&buffer);
    let vol_clone = Arc::clone(&volume_atomic);
    let out_channels = channels;

    let stream_config = cpal::StreamConfig {
        channels: out_channels,
        sample_rate: cpal::SampleRate(sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    let stream = device
        .build_output_stream(
            &stream_config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let vol =
                    f32::from_bits(vol_clone.load(std::sync::atomic::Ordering::Relaxed));
                let mut buf = buf_clone.lock().unwrap();
                for sample in data.iter_mut() {
                    *sample = buf.pop_front().unwrap_or(0.0) * vol;
                }
            },
            |err| {
                eprintln!("cpal stream error: {err}");
            },
            None,
        )
        .map_err(|e| format!("failed to build output stream: {e}"))?;

    stream.play().map_err(|e| format!("failed to start stream: {e}"))?;

    let _ = evt_tx.send(PlayerEvent::Playing {
        path: path.to_string(),
    });

    Ok(ThreadState::Playing(ActivePlayback {
        buffer,
        volume: volume_atomic,
        stream,
        decoder,
        format_reader,
        track_id,
        sample_rate,
        channels,
        duration_ms,
        samples_decoded: 0,
        paused: false,
        last_position_send: Instant::now(),
    }))
}

fn decode_next_packet(active: &mut ActivePlayback) -> DecodeResult {
    let packet = match active.format_reader.next_packet() {
        Ok(packet) => packet,
        Err(symphonia::core::errors::Error::IoError(ref e))
            if e.kind() == std::io::ErrorKind::UnexpectedEof =>
        {
            return DecodeResult::Finished;
        }
        Err(e) => return DecodeResult::Error(format!("read error: {e}")),
    };

    if packet.track_id() != active.track_id {
        return DecodeResult::Ok; // skip non-audio packets
    }

    let decoded = match active.decoder.decode(&packet) {
        Ok(decoded) => decoded,
        Err(symphonia::core::errors::Error::DecodeError(e)) => {
            // Non-fatal decode error, skip packet
            eprintln!("decode warning: {e}");
            return DecodeResult::Ok;
        }
        Err(e) => return DecodeResult::Error(format!("decode error: {e}")),
    };

    let spec = *decoded.spec();
    let num_frames = decoded.frames();
    let mut sample_buf = SampleBuffer::<f32>::new(num_frames as u64, spec);
    sample_buf.copy_interleaved_ref(decoded);

    let samples = sample_buf.samples();
    active.samples_decoded += samples.len() as u64;

    if let Ok(mut buf) = active.buffer.lock() {
        buf.extend(samples.iter());
    }

    DecodeResult::Ok
}

fn samples_to_ms(total_samples: u64, sample_rate: u32, channels: u16) -> u64 {
    if sample_rate == 0 || channels == 0 {
        return 0;
    }
    let frames = total_samples / channels as u64;
    frames * 1000 / sample_rate as u64
}
```

- [ ] **Step 3: Register module in lib.rs**

Add to `crates/auric-audio/src/lib.rs` at the top (after existing use statements):

```rust
pub mod player;
```

- [ ] **Step 4: Build and verify**

Run: `cargo check -p auric-audio 2>&1`
Expected: compiles.

- [ ] **Step 5: Commit**

```bash
git add crates/auric-audio/src/player.rs crates/auric-audio/src/lib.rs
git commit -m "feat: add audio player with symphonia decode and cpal output"
```

---

### Task 2: Add Playback Fields to ShellSnapshot and ShellState

**Files:**
- Modify: `crates/auric-ui/src/shell.rs`

- [ ] **Step 1: Add playback fields to ShellSnapshot**

After the existing `status_lines` field in `ShellSnapshot` (around line 98), add:

```rust
pub playback_status: String,
pub now_playing_title: String,
pub now_playing_artist: String,
pub now_playing_album: String,
pub now_playing_duration_ms: u64,
pub now_playing_position_ms: u64,
pub volume: f32,
pub shuffle: bool,
pub repeat_mode: String,
pub queue_length: usize,
pub queue_position: usize,
```

- [ ] **Step 2: Add playback state fields to ShellState**

After `scanning_path` in `ShellState` (around line 117), add:

```rust
pub playback_position_ms: u64,
pub playback_duration_ms: u64,
pub playback_status: String,
```

Initialize in `ShellState::new`:

```rust
playback_position_ms: 0,
playback_duration_ms: 0,
playback_status: "stopped".to_string(),
```

- [ ] **Step 3: Update all ShellSnapshot construction sites**

In `crates/auric-ui/src/shell.rs`, find the test helper `sample_state` (in the `#[cfg(test)]` module) and update the `ShellSnapshot` construction to include the new fields with default values:

```rust
playback_status: "stopped".to_string(),
now_playing_title: String::new(),
now_playing_artist: String::new(),
now_playing_album: String::new(),
now_playing_duration_ms: 0,
now_playing_position_ms: 0,
volume: 1.0,
shuffle: false,
repeat_mode: "off".to_string(),
queue_length: 0,
queue_position: 0,
```

Also update `build_shell_snapshot` in `crates/auric-app/src/lib.rs` to populate these from `app.playback_state`:

```rust
playback_status: match app.playback_state.session.status {
    PlaybackStatus::Playing => "playing",
    PlaybackStatus::Paused => "paused",
    PlaybackStatus::Stopped => "stopped",
}.to_string(),
now_playing_title: app.playback_state.current_entry()
    .and_then(|e| e.title.clone())
    .unwrap_or_default(),
now_playing_artist: app.playback_state.current_entry()
    .and_then(|e| e.artist.clone())
    .unwrap_or_default(),
now_playing_album: app.playback_state.current_entry()
    .and_then(|e| e.album.clone())
    .unwrap_or_default(),
now_playing_duration_ms: app.playback_state.current_entry()
    .and_then(|e| e.duration_ms)
    .unwrap_or(0) as u64,
now_playing_position_ms: app.playback_state.session.position_ms,
volume: app.playback_state.session.volume,
shuffle: app.playback_state.session.shuffle,
repeat_mode: match app.playback_state.session.repeat {
    RepeatMode::Off => "off",
    RepeatMode::One => "one",
    RepeatMode::All => "all",
}.to_string(),
queue_length: app.playback_state.queue.len(),
queue_position: app.playback_state.session.current_index
    .map(|i| i + 1)
    .unwrap_or(0),
```

- [ ] **Step 4: Build and verify**

Run: `cargo check 2>&1`
Expected: compiles (may have warnings about unused fields, that's fine for now).

- [ ] **Step 5: Commit**

```bash
git add crates/auric-ui/src/shell.rs crates/auric-app/src/lib.rs
git commit -m "feat: add playback state fields to ShellSnapshot"
```

---

### Task 3: TUI Keybindings for Playback

**Files:**
- Modify: `crates/auric-ui/src/shell.rs`

- [ ] **Step 1: Add PlaybackCommand variant to KeyAction**

Update the `KeyAction` enum:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum KeyAction {
    Continue,
    Quit,
    RefreshRequested,
    CommandSubmitted(String),
    Playback(PlaybackAction),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackAction {
    PlayTrack { track_index: usize },
    TogglePause,
    Stop,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    ToggleShuffle,
}
```

Remove `Eq` from `KeyAction` (since `PlaybackAction` contains `usize` which is `Eq`, this is fine, but removing `Eq` avoids issues if we add float later).

- [ ] **Step 2: Add keybindings in handle_key Normal mode**

In the Normal mode match block (around line 217), add before `_ => {}`:

```rust
KeyCode::Enter if self.focus == FocusPane::Tracks => {
    return KeyAction::Playback(PlaybackAction::PlayTrack {
        track_index: self.selected_track,
    });
}
KeyCode::Char(' ') => {
    return KeyAction::Playback(PlaybackAction::TogglePause);
}
KeyCode::Char('n') => {
    return KeyAction::Playback(PlaybackAction::Next);
}
KeyCode::Char('N') => {
    return KeyAction::Playback(PlaybackAction::Previous);
}
KeyCode::Char('+') | KeyCode::Char('=') => {
    return KeyAction::Playback(PlaybackAction::VolumeUp);
}
KeyCode::Char('-') => {
    return KeyAction::Playback(PlaybackAction::VolumeDown);
}
KeyCode::Char('s') => {
    return KeyAction::Playback(PlaybackAction::ToggleShuffle);
}
```

- [ ] **Step 3: Build and verify**

Run: `cargo check -p auric-ui 2>&1`
Expected: compiles (auric-app will fail until we handle PlaybackAction there, but auric-ui should be fine).

- [ ] **Step 4: Commit**

```bash
git add crates/auric-ui/src/shell.rs
git commit -m "feat: add playback keybindings to TUI"
```

---

### Task 4: Update Now Playing Panel

**Files:**
- Modify: `crates/auric-ui/src/shell.rs`

- [ ] **Step 1: Rewrite render_now_playing with real playback data**

Replace the `render_now_playing` function:

```rust
fn render_now_playing(frame: &mut Frame, area: Rect, state: &ShellState, palette: &Palette) {
    let block = pane_block("Now Playing", false, palette);
    let content_area = padded_inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();

    let is_playing = state.playback_status == "playing";
    let is_paused = state.playback_status == "paused";
    let has_track = !state.snapshot.now_playing_title.is_empty();

    if has_track {
        // Status icon + title
        let status_icon = if is_playing { ">" } else if is_paused { "||" } else { "[]" };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{status_icon} "),
                Style::default().fg(if is_playing {
                    palette.progress_fill
                } else {
                    palette.text_muted
                }),
            ),
            Span::styled(
                &state.snapshot.now_playing_title,
                Style::default().fg(palette.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}  {}", state.snapshot.now_playing_artist, state.snapshot.now_playing_album),
                Style::default().fg(palette.text_muted),
            ),
        ]));

        // Time + progress bar
        let position = state.playback_position_ms;
        let duration = state.playback_duration_ms;
        let progress = if duration > 0 {
            position as f32 / duration as f32
        } else {
            0.0
        };

        let time_str = format!(
            "{}  /  {}",
            format_ms(position),
            format_ms(duration),
        );

        let bar_width = content_area.width.saturating_sub(2);
        lines.push(Line::from(Span::styled(
            progress_bar(bar_width, progress),
            Style::default().fg(palette.progress_fill),
        )));
        lines.push(Line::from(vec![
            Span::styled(
                time_str,
                Style::default().fg(palette.text_muted),
            ),
            Span::styled(
                format!(
                    "   vol: {}%  {}  {}  {}/{}",
                    (state.snapshot.volume * 100.0).round() as u32,
                    if state.snapshot.shuffle { "shuffle" } else { "" },
                    match state.snapshot.repeat_mode.as_str() {
                        "one" => "repeat:1",
                        "all" => "repeat:all",
                        _ => "",
                    },
                    state.snapshot.queue_position,
                    state.snapshot.queue_length,
                ),
                Style::default().fg(palette.text_muted),
            ),
        ]));
    } else if let Some(track) = state.selected_track_item() {
        // No active playback, show selected track info
        lines.push(Line::from(Span::styled(
            format!("{} - {}", track.title, track.artist),
            Style::default().fg(palette.text_muted),
        )));
        lines.push(Line::from(Span::styled(
            "Press Enter to play",
            Style::default().fg(palette.text_muted),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "No track selected",
            Style::default().fg(palette.text_muted),
        )));
    }

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: true });
    frame.render_widget(paragraph, content_area);
}

fn format_ms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let minutes = total_secs / 60;
    let seconds = total_secs % 60;
    format!("{minutes:02}:{seconds:02}")
}

fn progress_bar(width: u16, progress: f32) -> String {
    let usable = usize::from(width.max(10)).saturating_sub(2);
    let filled = ((usable as f32) * progress.clamp(0.0, 1.0)).round() as usize;
    let mut body = String::with_capacity(usable);
    for idx in 0..usable {
        body.push(if idx < filled { '█' } else { '░' });
    }
    format!("[{body}]")
}
```

- [ ] **Step 2: Remove the old fake_progress_bar and fake_visualizer_line functions**

These are no longer needed. Delete them. If any test references them, update the test.

- [ ] **Step 3: Update help overlay and default status message**

Add playback shortcuts to the help overlay:

```rust
Line::from("Enter: play selected track"),
Line::from("Space: play/pause"),
Line::from("n / N: next / previous track"),
Line::from("+ / -: volume up / down"),
Line::from("s: toggle shuffle"),
```

Update default status message:

```rust
fn default_status_message() -> &'static str {
    "Enter: play  Space: pause  n/N: next/prev  +/-: volume  a: add music  ?: help"
}
```

- [ ] **Step 4: Build and verify**

Run: `cargo check -p auric-ui 2>&1`
Expected: compiles.

- [ ] **Step 5: Commit**

```bash
git add crates/auric-ui/src/shell.rs
git commit -m "feat: update now playing panel with real playback data"
```

---

### Task 5: Wire Player into App Layer

**Files:**
- Modify: `crates/auric-app/src/lib.rs`
- Modify: `crates/auric-app/Cargo.toml` (if auric-audio not already a dependency)

This is the integration task -- connect the PlayerHandle to BootstrappedApp, handle PlaybackAction from the TUI, manage the queue, and poll player events.

- [ ] **Step 1: Add PlayerHandle to BootstrappedApp**

Add `auric_audio::player::PlayerHandle` import and add field:

```rust
pub struct BootstrappedApp {
    pub config: AppConfig,
    pub db: Database,
    pub feature_registry: FeatureRegistry,
    pub playback_state: PlaybackState,
    pub report: BootstrapReport,
    pub player: auric_audio::player::PlayerHandle,
}
```

In `bootstrap_from_config_path`, before the `Ok(BootstrappedApp { ... })`:

```rust
let player = auric_audio::player::PlayerHandle::spawn();
```

Add `player` to the struct construction.

- [ ] **Step 2: Add player event polling callback type to the UI**

In `crates/auric-ui/src/shell.rs`, add a new callback type:

```rust
type PlayerEventFn<'a> = dyn FnMut() -> Vec<crate::shell::PlayerEventUpdate> + 'a;
```

And a simple struct for player updates:

```rust
#[derive(Debug, Clone)]
pub struct PlayerEventUpdate {
    pub position_ms: u64,
    pub duration_ms: u64,
    pub status: String,
    pub track_finished: bool,
}
```

Export `PlayerEventUpdate` and `PlaybackAction` from `crates/auric-ui/src/lib.rs`.

- [ ] **Step 3: Extend run_interactive_with_scan to accept playback handler**

Add a new `run_interactive_full` function that accepts all callback types including a playback action handler and player event poller:

```rust
pub fn run_interactive_full<FRefresh, FCommand, FScan, FPlayback, FPlayerPoll>(
    state: &mut ShellState,
    palette: &Palette,
    options: RunOptions,
    mut refresh: FRefresh,
    mut command_handler: FCommand,
    mut scan_handler: FScan,
    mut playback_handler: FPlayback,
    mut player_poll: FPlayerPoll,
) -> Result<(), UiError>
where
    FRefresh: FnMut() -> Result<ShellSnapshot, UiError>,
    FCommand: FnMut(&str) -> Result<PaletteCommandResult, UiError>,
    FScan: FnMut(String) -> std::sync::mpsc::Receiver<ScanProgress>,
    FPlayback: FnMut(PlaybackAction) -> Result<PaletteCommandResult, UiError>,
    FPlayerPoll: FnMut() -> Vec<PlayerEventUpdate>,
```

In the run_loop, add:
- A `KeyAction::Playback(action)` match arm that calls `playback_handler`
- On each tick, call `player_poll()` and update `state.playback_position_ms`, `state.playback_duration_ms`, `state.playback_status`. If `track_finished` is true, dispatch a next-track action through the playback handler.

- [ ] **Step 4: Wire up the preview handler to use run_interactive_full**

In the `"preview"` arm of `handle_ui_command`, switch from `run_interactive_with_scan` to `run_interactive_full`:

```rust
// Playback handler
|action: PlaybackAction| {
    let mut app_ref = app_cell.borrow_mut();
    handle_tui_playback_action(&mut app_ref, action)
        .map_err(|e| auric_ui::UiError::Terminal(format!("playback error: {e}")))
},
// Player event poller
|| {
    let app_ref = app_cell.borrow();
    let events = app_ref.player.poll_events();
    events.into_iter().filter_map(|evt| {
        match evt {
            auric_audio::player::PlayerEvent::Position { position_ms, duration_ms } => {
                Some(PlayerEventUpdate {
                    position_ms,
                    duration_ms,
                    status: "playing".to_string(),
                    track_finished: false,
                })
            }
            auric_audio::player::PlayerEvent::TrackFinished => {
                Some(PlayerEventUpdate {
                    position_ms: 0,
                    duration_ms: 0,
                    status: "stopped".to_string(),
                    track_finished: true,
                })
            }
            auric_audio::player::PlayerEvent::Paused => {
                Some(PlayerEventUpdate {
                    position_ms: 0,
                    duration_ms: 0,
                    status: "paused".to_string(),
                    track_finished: false,
                })
            }
            auric_audio::player::PlayerEvent::Stopped => {
                Some(PlayerEventUpdate {
                    position_ms: 0,
                    duration_ms: 0,
                    status: "stopped".to_string(),
                    track_finished: false,
                })
            }
            _ => None,
        }
    }).collect()
},
```

- [ ] **Step 5: Implement handle_tui_playback_action**

Add this function to `crates/auric-app/src/lib.rs`:

```rust
fn handle_tui_playback_action(
    app: &mut BootstrappedApp,
    action: auric_ui::PlaybackAction,
) -> Result<PaletteCommandResult> {
    match action {
        auric_ui::PlaybackAction::PlayTrack { track_index } => {
            // Build queue from current track list
            let tracks = app.db.list_tracks(10000).unwrap_or_default();
            let queue: Vec<PlaybackQueueEntry> = tracks
                .into_iter()
                .map(|t| PlaybackQueueEntry {
                    track_id: t.id,
                    path: t.path,
                    title: t.title,
                    artist: t.artist,
                    album: t.album,
                    duration_ms: t.duration_ms,
                    sample_rate: t.sample_rate,
                    channels: t.channels,
                    bit_depth: t.bit_depth,
                })
                .collect();

            if track_index >= queue.len() {
                return Ok(PaletteCommandResult::new("No track at that index", false));
            }

            app.playback_state.queue = queue;
            app.playback_state.session.current_index = Some(track_index);
            app.playback_state.session.status = PlaybackStatus::Playing;
            app.playback_state.session.position_ms = 0;

            let entry = &app.playback_state.queue[track_index];
            app.player.load(&entry.path);

            let title = entry.title.clone().unwrap_or_default();
            Ok(PaletteCommandResult::new(
                format!("Playing: {title}"),
                true,
            ))
        }
        auric_ui::PlaybackAction::TogglePause => {
            match app.playback_state.session.status {
                PlaybackStatus::Playing => {
                    app.player.pause();
                    app.playback_state.session.status = PlaybackStatus::Paused;
                    Ok(PaletteCommandResult::new("Paused", true))
                }
                PlaybackStatus::Paused => {
                    app.player.resume();
                    app.playback_state.session.status = PlaybackStatus::Playing;
                    Ok(PaletteCommandResult::new("Resumed", true))
                }
                PlaybackStatus::Stopped => {
                    // If we have a queue, play from current position
                    if let Some(idx) = app.playback_state.session.current_index {
                        if let Some(entry) = app.playback_state.queue.get(idx) {
                            app.player.load(&entry.path);
                            app.playback_state.session.status = PlaybackStatus::Playing;
                            let title = entry.title.clone().unwrap_or_default();
                            return Ok(PaletteCommandResult::new(
                                format!("Playing: {title}"),
                                true,
                            ));
                        }
                    }
                    Ok(PaletteCommandResult::new("No track to play", false))
                }
            }
        }
        auric_ui::PlaybackAction::Stop => {
            app.player.stop();
            app.playback_state.session.status = PlaybackStatus::Stopped;
            Ok(PaletteCommandResult::new("Stopped", true))
        }
        auric_ui::PlaybackAction::Next => {
            let mut events = Vec::new();
            handle_playback_transport_command(app, AppCommand::Next, &mut events)?;
            if app.playback_state.session.status == PlaybackStatus::Playing {
                if let Some(entry) = app.playback_state.current_entry() {
                    app.player.load(&entry.path);
                    let title = entry.title.clone().unwrap_or_default();
                    return Ok(PaletteCommandResult::new(format!("Playing: {title}"), true));
                }
            }
            Ok(PaletteCommandResult::new("End of queue", true))
        }
        auric_ui::PlaybackAction::Previous => {
            let mut events = Vec::new();
            handle_playback_transport_command(app, AppCommand::Previous, &mut events)?;
            if let Some(entry) = app.playback_state.current_entry() {
                if app.playback_state.session.status == PlaybackStatus::Playing {
                    app.player.load(&entry.path);
                }
                let title = entry.title.clone().unwrap_or_default();
                return Ok(PaletteCommandResult::new(format!("Track: {title}"), true));
            }
            Ok(PaletteCommandResult::new("Start of queue", true))
        }
        auric_ui::PlaybackAction::VolumeUp => {
            let new_vol = (app.playback_state.session.volume + 0.05).min(1.0);
            app.playback_state.session.volume = new_vol;
            app.player.set_volume(new_vol);
            Ok(PaletteCommandResult::new(
                format!("Volume: {}%", (new_vol * 100.0).round() as u32),
                true,
            ))
        }
        auric_ui::PlaybackAction::VolumeDown => {
            let new_vol = (app.playback_state.session.volume - 0.05).max(0.0);
            app.playback_state.session.volume = new_vol;
            app.player.set_volume(new_vol);
            Ok(PaletteCommandResult::new(
                format!("Volume: {}%", (new_vol * 100.0).round() as u32),
                true,
            ))
        }
        auric_ui::PlaybackAction::ToggleShuffle => {
            app.playback_state.session.shuffle = !app.playback_state.session.shuffle;
            let label = if app.playback_state.session.shuffle {
                "Shuffle: on"
            } else {
                "Shuffle: off"
            };
            Ok(PaletteCommandResult::new(label, true))
        }
    }
}
```

- [ ] **Step 6: Handle track finished in the event loop**

In the run_loop player event polling section, when `track_finished` is true, dispatch `PlaybackAction::Next` through the playback handler to auto-advance the queue.

- [ ] **Step 7: Build and verify**

Run: `cargo check 2>&1`
Expected: compiles.

- [ ] **Step 8: Commit**

```bash
git add crates/auric-audio/src/player.rs crates/auric-audio/src/lib.rs crates/auric-ui/src/shell.rs crates/auric-ui/src/lib.rs crates/auric-app/src/lib.rs
git commit -m "feat: wire audio player into app and TUI with keybindings"
```

---

### Task 6: Final Integration and Manual Test

**Files:**
- Possibly minor fixes across all modified files

- [ ] **Step 1: Full build and test**

Run: `cargo test 2>&1`
Expected: all tests pass.

Run: `cargo check 2>&1`
Expected: zero warnings, zero errors.

- [ ] **Step 2: Manual test**

Run: `cargo run -p auric-app`

Test the full flow:
1. Press `a`, add a music folder with real audio files
2. Wait for scan to complete
3. Select a track with `j/k`
4. Press Enter -- audio should play
5. Press Space -- should pause/resume
6. Press `n` -- next track
7. Press `N` -- previous track
8. Press `+`/`-` -- volume change
9. Press `s` -- shuffle toggle
10. Let a track finish naturally -- should auto-advance to next
11. Verify Now Playing panel shows real position/progress

- [ ] **Step 3: Fix any issues found during manual testing**

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "chore: integration fixes for audio playback"
```
