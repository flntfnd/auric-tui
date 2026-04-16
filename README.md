# Auric

A cross-platform terminal audio player built in Rust. Designed to feel like a serious desktop player compressed into a terminal, not a file picker with play/pause.

Auric targets high-resolution and lossless playback, capability-aware terminal rendering, and modular features that can be toggled at runtime.

## Features

**Playback**
- FLAC, WAV, MP3, AAC, OGG Vorbis, ALAC, ADPCM, MKV/WebM audio via Symphonia
- Lock-free audio output via cpal with automatic sample rate conversion and mono/stereo upmixing
- Queue management with repeat modes (off, one, all) and shuffle
- Volume control and playback transport (play, pause, stop, next, previous)
- Session state persisted across restarts

**Library**
- Directory scanning with embedded metadata extraction (tags, duration, sample rate, bit depth, channels)
- Embedded artwork extraction and deduplication via content-hash
- Watched folders with filesystem event debouncing and incremental rescan
- Playlist CRUD with track ordering
- SQLite persistence with WAL mode and batch operations
- Browse by artist, album, or all songs with miller-column navigation

**Terminal UI**
- Multi-pane layout: library roots, browse modes, track list, now playing with artwork
- Album art display with auto-detected graphics protocol (Kitty, Sixel, iTerm2, halfblocks)
- Pixel art mode for chunky retro artwork rendering
- Real-time braille-dot spectrum visualizer driven by FFT
- Interactive seek bar with mouse click-to-seek
- Column sorting (click headers or press `o` to cycle)
- Double-click to play tracks
- Drag-and-drop folder adding (supported terminals)
- First-run welcome wizard for adding music
- Settings panel for live configuration changes
- Track info panel with artwork and full metadata
- Keyboard navigation (vim-style + arrows), mouse support, focus cycling
- Command palette with inline parameter input
- Track search/filter within the current view
- Rounded borders with polished focus indicators
- Animated transitions on track changes via tachyonfx
- Icon modes: Nerd Font glyphs with ASCII fallback
- Token-based theming with terminal-native background support

**Modular Features**
- 11 runtime feature toggles: metadata, artwork, remote metadata, watched folders, equalizer, visualizer, analytics, P2P sync, P2P stream, mouse, image artwork
- Features transition through states: Disabled, Starting, Enabled, Degraded

## Install

### From source

Requires [Rust](https://rustup.rs/) 1.79 or later and a working C compiler for the bundled SQLite.

#### macOS

Xcode Command Line Tools provides the C compiler. If you don't have Rust yet:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Then build:

```sh
git clone https://github.com/flntfnd/auric-tui.git
cd auric-tui
cargo install --path crates/auric-app
```

#### Linux

Install Rust and the system dependencies for audio (ALSA headers). The C compiler and pkg-config are also required.

**Debian / Ubuntu:**

```sh
sudo apt update && sudo apt install -y build-essential pkg-config libasound2-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**Fedora / RHEL / CentOS:**

```sh
sudo dnf install -y gcc pkg-config alsa-lib-devel
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**Arch Linux:**

```sh
sudo pacman -S base-devel alsa-lib pkg-config
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Then build:

```sh
git clone https://github.com/flntfnd/auric-tui.git
cd auric-tui
cargo install --path crates/auric-app
```

#### Windows

Install [Rust](https://rustup.rs/) (this includes the MSVC build tools prompt if Visual Studio isn't already installed). Then from PowerShell or Command Prompt:

```powershell
git clone https://github.com/flntfnd/auric-tui.git
cd auric-tui
cargo install --path crates/auric-app
```

No additional system libraries are needed on Windows. WASAPI is built into the OS and SQLite is bundled.

The binary installs to `%USERPROFILE%\.cargo\bin\auric.exe`.

#### Manual build (all platforms)

If you prefer not to install to your Cargo bin directory:

```sh
git clone https://github.com/flntfnd/auric-tui.git
cd auric-tui
cargo build --release
```

The binary is at `target/release/auric` (or `target\release\auric.exe` on Windows).

### From releases

Pre-built binaries are available on the [Releases](https://github.com/flntfnd/auric-tui/releases) page.

## Quick start

```sh
auric
```

On first launch with an empty library, a welcome panel appears. Press `a` at any time to add a music folder. Tracks are scanned and imported automatically.

## Keyboard shortcuts

### Playback

| Key | Action |
|-----|--------|
| `Enter` | Play selected track (queues current view) |
| `Space` | Play / pause toggle |
| `n` | Next track |
| `N` | Previous track |
| `+` / `=` | Volume up |
| `-` | Volume down |
| `s` | Toggle shuffle |

### Navigation

| Key | Action |
|-----|--------|
| `j` / `k` or Up / Down | Move selection |
| `h` / `l` or Left / Right | Browse: collapse / expand |
| `g` | Jump to top |
| `G` | Jump to bottom |
| `Page Up` / `Page Down` | Scroll by page |
| `Tab` / `Shift-Tab` | Cycle focus between panes |

### Library

| Key | Action |
|-----|--------|
| `a` | Add music folder |
| `i` | Track info with artwork |
| `o` | Cycle sort column |
| `r` | Refresh library |
| `/` | Search / filter tracks |

### UI

| Key | Action |
|-----|--------|
| `,` | Open settings |
| `:` or `Ctrl-p` | Command palette |
| `?` | Help overlay |
| `Esc` | Close overlay / modal |
| `q` or `Ctrl-c` | Quit |

### Mouse

| Action | Effect |
|--------|--------|
| Click track | Select |
| Double-click track | Play |
| Click column header | Sort by column |
| Click seek bar | Seek to position |
| Scroll wheel | Scroll list |
| Drag folder onto window | Add as library root |

## Configuration

Auric reads `config/default.toml` from the working directory. Key sections:

```toml
[features]
metadata = true
artwork = true
watched_folders = true
mouse = true

[library]
auto_scan_on_start = true
watch_debounce_ms = 750
scan_batch_size = 2000

[ui]
theme = "auric-dark"
color_scheme = "dark"
icon_pack = "nerd-font"
use_theme_background = false
pixel_art_artwork = false
pixel_art_cell_size = 2

[database]
path = "var/auric.db"
journal_mode = "wal"
```

## Theming

Themes live in the `themes/` directory as TOML files. Token-based, no hardcoded values.

```toml
name = "auric-dark"

[colors]
surface_0 = "#0f1115"
surface_1 = "#171a21"
text = "#e8ecf3"
accent = "#4fd1c5"
border_focused = "#90cdf4"
border_unfocused = "#1e2736"
visualizer_low = "#63b3ed"
visualizer_mid = "#4fd1c5"
visualizer_high = "#f6ad55"
```

By default, auric uses your terminal's background color. Set `use_theme_background = true` in the `[ui]` section to use the theme's background instead.

## Terminal font

Auric targets **FiraCode Nerd Font Mono** for icon support. Configure this in your terminal emulator. If Nerd Font glyphs are unavailable, set `icon_pack = "ascii"` in settings (press `,`) or in your config file.

## Album art

Auric auto-detects your terminal's graphics protocol and renders album artwork using the best available method:

1. Kitty graphics protocol
2. Sixel
3. iTerm2 inline images
4. Halfblock characters (universal fallback)

Enable pixel art mode in settings for a chunky retro look.

## Architecture

Seven workspace crates with clear boundaries:

| Crate | Role |
|-------|------|
| `auric-core` | Shared types, feature registry, event contracts |
| `auric-audio` | Playback engine with lock-free output, symphonia decoding, cpal output |
| `auric-library` | Library scan/watch, playlists, SQLite persistence |
| `auric-drift` | Intelligent shuffle algorithm and audio feature analyzer |
| `auric-ui` | TUI rendering, input handling, theming, visualizer, artwork |
| `auric-net` | Listen-along sync and P2P streaming interfaces (planned) |
| `auric-app` | Composition root, CLI, bootstrap |

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

## License

MIT OR Apache-2.0
