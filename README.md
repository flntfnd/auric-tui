# Auric

A cross-platform terminal audio player built in Rust. Designed to feel like a serious desktop player compressed into a terminal -- not a file picker with play/pause.

Auric targets high-resolution and lossless playback, capability-aware terminal rendering, and modular features that can be toggled at runtime.

```
+---------------------+-------------------------------+---------------------+
| Library             | Tracks                        | Now Playing         |
|                     |                               |                     |
| > Roots             |  # | Title       | Artist    | Track Title         |
|   /music/flac       |  1 | Intro       | Artist A  | Artist - Album      |
|   /music/vinyl-rips |  2 | Deep Cut    | Artist B  |                     |
|                     |  3 | Closer      | Artist A  | 00:00 ====---- 4:32 |
| > Browse            |  4 | Interlude   | Artist C  |                     |
|   All Tracks        |  5 | Opener      | Artist D  | |||||||||||||||      |
|   Albums            |                               |                     |
|   Artists            |                               |                     |
|                     |                               |                     |
| > Playlists         |                               |                     |
|   Road Trip         |                               |                     |
|   Late Night        |                               |                     |
+---------------------+-------------------------------+---------------------+
| > Playing | FLAC 96kHz/24bit | Vol: 100% | Watched: 2 roots              |
+---------------------+-------------------------------+---------------------+
```

## Features

**Playback**
- FLAC, WAV, MP3, AAC, OGG Vorbis, ALAC, ADPCM, MKV/WebM audio via Symphonia
- Device enumeration and output via cpal
- Queue management with repeat modes (off, one, all) and shuffle
- Volume control and playback transport (play, pause, stop, seek, next, previous)
- Session state persisted across restarts

**Library**
- Directory scanning with embedded metadata extraction (tags, duration, sample rate, bit depth, channels)
- Embedded artwork extraction and deduplication via content-hash
- Watched folders with filesystem event debouncing and incremental rescan
- Playlist CRUD with track ordering
- SQLite persistence with WAL mode and batch operations

**Terminal UI**
- Three-pane layout: library browser, track list, now playing
- Keyboard navigation (vim-style + arrows), mouse support, focus cycling
- Command palette with inline parameter input
- Track search/filter within the current view
- Icon modes: Nerd Font glyphs with ASCII fallback
- Token-based theming (dark and light themes included)

**Modular Features**
- 11 runtime feature toggles: metadata, artwork, remote metadata, watched folders, equalizer, visualizer, analytics, P2P sync, P2P stream, mouse, image artwork
- Features transition through states: Disabled, Starting, Enabled, Degraded

## Install

### From source

Requires [Rust](https://rustup.rs/) 1.79 or later.

```sh
git clone https://github.com/flntfnd/auric-tui.git
cd auric-tui
cargo build --release
```

The binary is at `target/release/auric`.

To install it to your Cargo bin directory:

```sh
cargo install --path crates/auric-app
```

### From releases

Pre-built binaries for macOS (Apple Silicon and Intel) and Linux (x86_64) are available on the [Releases](https://github.com/flntfnd/auric-tui/releases) page.

```sh
# macOS (Apple Silicon)
curl -L https://github.com/flntfnd/auric-tui/releases/latest/download/auric-aarch64-apple-darwin.tar.gz | tar xz
sudo mv auric /usr/local/bin/

# macOS (Intel)
curl -L https://github.com/flntfnd/auric-tui/releases/latest/download/auric-x86_64-apple-darwin.tar.gz | tar xz
sudo mv auric /usr/local/bin/

# Linux (x86_64)
curl -L https://github.com/flntfnd/auric-tui/releases/latest/download/auric-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo mv auric /usr/local/bin/
```

## Quick start

```sh
# Add a music directory
auric root add /path/to/music

# Scan it
auric scan

# Launch the TUI
auric ui
```

## Usage

```
auric <command> [args]

Commands:
  root       Manage library roots (list, add <path>)
  scan       Scan library roots for audio files
  watch      Watch library roots for filesystem changes
  track      Query tracks (list, get, search)
  playlist   Manage playlists (list, create, rename, delete, tracks, add, remove, clear, load)
  playback   Playback transport (status, play, pause, stop, next, prev, seek, volume, repeat, shuffle, queue)
  audio      Audio device info (devices, inspect <path>)
  artwork    Artwork cache info (stats, lookup, list)
  feature    Feature toggles (list, enable, disable)
  ui         Launch the interactive TUI
```

## Keyboard shortcuts

| Key | Action |
|-----|--------|
| `Tab` / `Shift-Tab` | Cycle focus between panes |
| `j` / `k` or Up / Down | Move selection |
| `g` / `G` | Jump to top / bottom |
| `Space` | Play / pause |
| `/` | Search / filter tracks |
| `:` | Open command palette |
| `?` | Toggle help overlay |
| `Esc` | Close overlay / exit filter |
| `q` / `Ctrl-c` | Quit |

## Configuration

Auric reads `config/default.toml` from the working directory. Key sections:

```toml
[features]
metadata = true          # Read embedded tags
artwork = true           # Extract embedded artwork
watched_folders = true   # Enable filesystem watching
visualizer = false       # Audio visualizer (planned)
mouse = true             # Mouse input support

[library]
auto_scan_on_start = true
watch_debounce_ms = 750
scan_batch_size = 2000

[ui]
theme = "auric-dark"     # auric-dark | auric-light
icon_pack = "nerd-font"  # nerd-font | ascii
refresh_hz = 30

[database]
path = "var/auric.db"
journal_mode = "wal"
```

## Theming

Themes live in the `themes/` directory as TOML files. Token-based -- no hardcoded values.

```toml
name = "auric-dark"

[colors]
surface_0 = "#0f1115"
surface_1 = "#171a21"
text = "#e8ecf3"
accent = "#4fd1c5"
visualizer_low = "#63b3ed"
visualizer_mid = "#4fd1c5"
visualizer_high = "#f6ad55"

[layout]
padding_x = 1

[motion]
tick_ms = 33
```

## Terminal font

Auric targets **FiraCode Nerd Font Mono** for icon support. Configure this in your terminal emulator. If Nerd Font glyphs are unavailable, set `icon_pack = "ascii"` in your config or toggle via the feature system.

## Architecture

Six workspace crates with clear boundaries:

| Crate | Role |
|-------|------|
| `auric-core` | Shared types, feature registry, event contracts |
| `auric-audio` | Playback engine, decoder/output backends, DSP chain |
| `auric-library` | Library scan/watch, playlists, SQLite persistence |
| `auric-ui` | TUI rendering, input handling, theming |
| `auric-net` | Listen-along sync and P2P streaming interfaces (planned) |
| `auric-app` | Composition root, CLI, bootstrap |

See `docs/ARCHITECTURE.md`, `docs/ROADMAP.md`, and `docs/UX.md` for detailed design documentation.

## Supported formats

Via Symphonia (pure Rust, no system dependencies):

| Format | Container |
|--------|-----------|
| FLAC | .flac |
| WAV / PCM | .wav |
| MP3 (MPEG Layer 1/2/3) | .mp3 |
| AAC | .m4a, .mp4 |
| Vorbis | .ogg |
| ALAC | .m4a |
| ADPCM | .wav |
| Opus | .ogg, .webm |
| Audio in MKV/WebM | .mkv, .webm |

## Status

Phase 0/1. Local playback, library management, and TUI are functional. Playback engine is wired for device enumeration and format inspection -- stream output is next.

See the [Roadmap](docs/ROADMAP.md) for planned phases including metadata enrichment, DSP/visualizer, analytics, and P2P social listening.

## License

MIT OR Apache-2.0
