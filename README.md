# auric-tui

A terminal-based lossless audio player with a rich TUI interface.

## Features

- **Lossless Audio Support** - Play FLAC, ALAC, WAV, and other high-quality formats
- **Modern TUI** - Beautiful terminal interface built with Ratatui
- **Spectrum Analyzer** - Real-time audio visualization
- **Album Art** - Displays embedded album artwork (in supported terminals)
- **Library Management** - Organize music by folders with multi-select filtering
- **Watched Folders** - Auto-sync folders that update when files change
- **Playlists** - Create and manage playlists
- **Themes** - Choose from Default, Dracula, or Gruvbox color schemes
- **Mouse Support** - Click to select, scroll to navigate
- **Keyboard Driven** - Full vim-style navigation

## Installation

### From source (recommended)

Requires [Rust](https://rustup.rs/) 1.70 or later.

```bash
# Clone the repository
git clone https://github.com/user/auric-tui.git
cd auric-tui

# Install
cargo install --path .
```

### From GitHub directly

```bash
cargo install --git https://github.com/user/auric-tui
```

After installation, run:

```bash
auric-tui
```

## Keybindings

### Navigation
| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Switch panels |
| `j` / `k` or `↑` / `↓` | Move selection |
| `Enter` | Play selected / Open |
| `Esc` | Cancel / Clear selection |

### Playback
| Key | Action |
|-----|--------|
| `Space` | Play / Pause |
| `[` / `]` | Seek backward / forward |
| `+` / `-` | Volume up / down |
| `s` | Toggle shuffle |
| `r` | Toggle repeat (Off → All → One) |
| `S` | Cycle sort mode |

### Library
| Key | Action |
|-----|--------|
| `o` | Load folder |
| `w` | Add watched folder |
| `d` | Remove folder / Stop watching |
| `A` | Fetch missing album art |
| `Ctrl+F` | Search tracks |

### Playlists
| Key | Action |
|-----|--------|
| `N` | New playlist |
| `a` | Add track to playlist |
| `D` | Delete playlist |

### Other
| Key | Action |
|-----|--------|
| `,` | Settings (theme, display options) |
| `?` | Help |
| `q` | Quit |

## Settings

Press `,` to open settings:

- **Theme** - Default, Dracula, or Gruvbox
- **Spectrum Analyzer** - Toggle on/off for performance
- **Album Art** - Toggle on/off

Settings are persisted automatically.

## Supported Formats

- FLAC
- ALAC (Apple Lossless)
- WAV
- MP3
- AAC
- OGG Vorbis
- And more via Symphonia

## Requirements

- A terminal with 256-color support (most modern terminals)
- For album art display: A terminal with image support (iTerm2, Kitty, WezTerm, etc.)

## Configuration

auric-tui stores its database and settings in:
- macOS: `~/.config/auric-tui/`
- Linux: `~/.config/auric-tui/`
- Windows: `%APPDATA%\auric-tui\`

## License

MIT
