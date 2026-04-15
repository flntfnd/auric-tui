# Audio Playback Engine

## Overview

Add real audio playback to auric. A background player thread decodes audio with symphonia and outputs via cpal. The TUI stays responsive, receives position updates, and controls playback through keybindings.

---

## Architecture

Three layers, each with a single responsibility:

### 1. Player (`crates/auric-audio/src/player.rs`)

A background thread that owns the audio pipeline. Controlled via `PlayerCommand` sent over an `mpsc` channel. Reports state via `PlayerEvent` sent back to the caller.

**PlayerHandle** -- the caller-facing API:
- `play(path: String, start_position_ms: u64)` -- load and play a file
- `pause()` / `resume()` / `stop()`
- `seek(position_ms: u64)`
- `set_volume(volume: f32)` -- 0.0 to 1.0
- `poll_events() -> Vec<PlayerEvent>` -- drain pending events (non-blocking)
- `is_active() -> bool`

**PlayerCommand** enum (sent to thread):
- `Load { path: String, start_position_ms: u64 }`
- `Pause`
- `Resume`
- `Stop`
- `Seek { position_ms: u64 }`
- `SetVolume { volume: f32 }`
- `Shutdown`

**PlayerEvent** enum (sent from thread):
- `Playing { path: String }` -- playback started
- `Paused`
- `Resumed`
- `Stopped`
- `Position { position_ms: u64, duration_ms: u64 }` -- sent every ~250ms
- `TrackFinished` -- natural end of track, signals app to advance queue
- `Error { message: String }` -- decode/device error

**Audio pipeline inside the thread:**
1. Open file with `symphonia::default::get_probe()`
2. Get the default audio track, create a decoder
3. Open cpal output stream targeting the system default device (or configured device)
4. Decode loop: read packets, decode to samples, apply volume, write to cpal stream
5. Send `Position` events at ~250ms intervals
6. On track end, send `TrackFinished`
7. Between decodes, check the command channel for pause/stop/seek/volume changes

**Sample buffer:** `Arc<Mutex<VecDeque<f32>>>` shared between the decode loop (producer) and the cpal output callback (consumer). The decode loop fills it, cpal drains it. If the buffer is empty, cpal outputs silence. No lock-free ring buffer needed -- track-by-track playback, not gapless.

**Sample rate / channel conversion:** Use symphonia's decoded output format. If the cpal device sample rate differs, do nearest-neighbor resampling (simple, adequate for v1). Channel count mismatch: mono to stereo by duplicating, stereo to mono by averaging, other configurations by truncating or zero-padding.

**Volume:** Applied as a multiply on each sample before pushing to the buffer. `volume` is stored as `f32` 0.0-1.0.

### 2. App Integration (`crates/auric-app/src/lib.rs`)

**BootstrappedApp** gains a `player: PlayerHandle` field, initialized at startup.

**Queue management:** The app layer owns the queue (`PlaybackState.queue`). When the user presses Enter on a track:
1. Build the queue from the current view context -- all tracks visible in the current browse mode
2. Set `current_index` to the selected track's position in that queue
3. Send `PlayerCommand::Load` with the track's file path
4. On `PlayerEvent::TrackFinished`, advance `current_index` and load the next track

**View-contextual queue building:**
- Songs view: all tracks in the library (or filtered set)
- Artist view: all tracks by that artist
- Album view: all tracks in that album
- Genre view: all tracks in that genre
- Playlist view: all tracks in the playlist

Since browse modes aren't fully implemented yet (the sidebar shows Artists/Genres/Albums/Songs but only Songs is functional), the initial implementation queues from the current track list (whatever is visible after filtering). The architecture supports contextual queuing when browse modes are built out.

**Shuffle:** When enabled, randomize the queue order (using `rand`). Maintain the original order so shuffle can be toggled off to restore it.

**Repeat:** `RepeatMode::Off` stops at end of queue. `RepeatMode::One` loops the current track. `RepeatMode::All` wraps to the beginning.

**Persist state:** On pause/stop/track change, persist `PlaybackState` to the database so the app can resume position on restart.

### 3. TUI Wiring (`crates/auric-ui/src/shell.rs` + `crates/auric-app/src/lib.rs`)

**ShellSnapshot gains playback fields:**
```
pub playback_status: String,        // "playing", "paused", "stopped"
pub now_playing_title: String,
pub now_playing_artist: String,
pub now_playing_album: String,
pub now_playing_duration_ms: u64,
pub now_playing_position_ms: u64,
pub volume: f32,
pub shuffle: bool,
pub repeat_mode: String,
pub queue_length: usize,
pub queue_position: usize,          // 1-indexed for display
```

**Event loop integration:**
- The event loop calls `player.poll_events()` on each tick (alongside scan progress polling)
- Position events update `ShellState` playback fields
- TrackFinished triggers queue advancement through the command handler
- The run loop needs a new callback type or the existing command handler extended to accept internal playback commands

**Keybindings (Normal mode):**

| Key | Action |
|-----|--------|
| Enter | Play selected track, queue from current view |
| Space | Play/pause toggle |
| n | Next track in queue |
| N | Previous track in queue |
| + or = | Volume up 5% |
| - | Volume down 5% |
| s | Toggle shuffle |

These work from any pane, except Enter which only triggers playback when focused on the Tracks pane.

**Now Playing panel updates:**
- Track title, artist, album from the current queue entry
- Elapsed / total time: `01:23 / 04:56` format, updated from Position events
- Progress bar reflects real position (replace the fake static one)
- Status icon: play/pause/stop indicator
- Queue position: `3/47` style

**Status bar:** Shows playback status inline: `Playing: Track Name - Artist  01:23/04:56`

---

## Error Handling

- **Unsupported codec:** Send `PlayerEvent::Error`, skip to next track in queue. Show warning in status bar.
- **Device unavailable:** Try system default. If that fails, show error and disable playback until user action.
- **File not found:** Skip track, advance queue, show warning.
- **Empty queue after Enter:** Do nothing (shouldn't happen since Enter builds the queue).
- **Player thread panic:** The `PlayerHandle` detects the closed channel and reports the error. The app continues functioning without playback.

---

## Output Device Selection

- Default: system default output device via `cpal::default_host().default_output_device()`
- Config: `config/default.toml` gains an `[audio]` section with `output_device = "default"` or a specific device name
- The existing `AudioEngine::list_output_devices()` method can be used to enumerate available devices
- Device selection change requires restarting the player thread (stop, recreate with new device, resume)

---

## Testing

- **Player module:** Integration test that loads a short audio file, plays for 1 second, verifies Position events arrive, stops. Requires a test audio file in the repo.
- **Queue logic:** Unit tests for queue building, next/previous, shuffle, repeat mode transitions.
- **Event polling:** Unit test that verifies PlayerHandle::poll_events drains correctly.
- **Manual testing:** Full flow -- launch app, add music folder, select track, verify audio output, test all keybindings.

---

## Files

### New
- `crates/auric-audio/src/player.rs` -- PlayerHandle, PlayerCommand, PlayerEvent, player thread, decode/output loop

### Modified
- `crates/auric-audio/src/lib.rs` -- add `pub mod player`, export types
- `crates/auric-audio/Cargo.toml` -- may need to add `rand` if not already available
- `crates/auric-app/src/lib.rs` -- init player in bootstrap, wire keybindings to commands, queue management, build_shell_snapshot with playback fields, poll player events in the UI handler
- `crates/auric-ui/src/shell.rs` -- add playback fields to ShellSnapshot/ShellState, keybindings, now playing rendering with real data, player event polling in run_loop
- `crates/auric-ui/src/lib.rs` -- export new types if needed
- `crates/auric-core/src/lib.rs` -- no changes needed, existing PlaybackState/AppCommand types are sufficient

---

## Implementation Order

1. **Player module** -- PlayerHandle, PlayerCommand, PlayerEvent, decode/output thread. Get audio playing from a hardcoded path first.
2. **App integration** -- PlayerHandle in BootstrappedApp, queue building, track advancement.
3. **TUI keybindings** -- Enter/Space/n/N/+/-/s wired to player commands.
4. **Now Playing panel** -- real-time position, progress bar, track info.
5. **Event loop integration** -- poll player events in run_loop, update state.
6. **Persist and polish** -- save/restore position, volume, shuffle state. Error handling for edge cases.

Steps 1-2 are sequential. Steps 3-5 can be parallelized after step 2. Step 6 follows.
