# UX Design Spec (v0)

## UX Thesis

Auric should feel like a serious desktop player compressed into a terminal, not a file picker with play/pause.

Principles:
- Fast path first: common actions require zero mode confusion and minimal keystrokes
- Discoverable depth: advanced features exist behind command palette, contextual actions, and inspector panels
- Stable mental model: same commands work across views where possible
- Capability-aware delight: enhance when terminal supports it, never punish when it doesn't

## Interaction Model

Auric uses a command-driven UI with focused panes.

Core concepts:
- Focused pane: receives navigation/input (Library, Queue, Playlists, Inspector, Search, Settings)
- Global actions: playback, command palette, search, volume, layout switch
- Context actions: depend on selected item (track/album/playlist/root)
- Background jobs: scan/fetch/write operations with visible status and cancel where possible

## Primary Layout (Desktop terminal)

Three-column default:
- Left: Source browser (Library roots, playlists, filters)
- Center: Item list / search results / queue
- Right: Now Playing + artwork + metadata + mini visualizer + session status
- Bottom status line: transport, output device, format, watcher state, network state, job progress

Alternate layouts:
- Compact (80x24): tabs + single main pane + collapsible now playing
- Focus mode: one pane fullscreen (library/search/queue/visualizer)
- Remote terminal mode: reduced refresh + no artwork images + compact status

## Keyboard UX (initial conventions)

Global:
- `Space`: play/pause
- `Enter`: primary action (play/open/apply)
- `/`: search current pane
- `:` or `Ctrl-p`: command palette
- `Tab` / `Shift-Tab`: cycle panes
- `[` / `]`: previous/next track
- `,` / `.`: seek backward/forward (small)
- `<` / `>`: seek backward/forward (large)
- `-` / `=`: volume down/up
- `q`: close panel/back (never hard quit if modal is open)
- `Ctrl-c`: quit (with safe shutdown)

Pane navigation (vim-friendly + arrows):
- `j/k` and Up/Down: move selection
- `h/l` and Left/Right: collapse/expand/open
- `g/G`: top/bottom
- `PgUp/PgDn`: page jump

Bulk/context actions:
- `a`: add to queue
- `A`: append next
- `p`: add to playlist
- `e`: edit metadata (selected)
- `d`: delete/remove (with confirmation semantics)
- `r`: rescan / refresh context
- `m`: fetch missing metadata/artwork (contextual)

## Mouse UX

Mouse support is enhancement, not dependency.

Support:
- Click to focus/select
- Double-click to play/open
- Scroll lists
- Drag splitter (where terminal supports reliable reporting)
- Clickable status widgets (output device, EQ, visualizer, P2P)
- Right-click / alt-click fallback to context menu key if unsupported

Every mouse action must map to keyboard.

## Command Palette (innovation surface)

The command palette is the primary discoverability tool.

Capabilities:
- Fuzzy command search
- Context-aware commands (based on selection/focus)
- Inline parameter prompts (e.g., create playlist name)
- Shows shortcuts and feature availability state
- Can surface "disabled by settings" and offer one-step enable

Examples:
- `Enable visualizer`
- `Fetch artwork for selected album`
- `Create watched folder`
- `Start listen-along session`
- `Switch output device`
- `Toggle lossless-only filter`

## Settings UX (modularity)

Settings must expose feature modules clearly and explain performance impact.

Settings sections:
- Playback
- Library
- Metadata & Artwork
- Appearance
- Input
- Network / Social
- Analytics
- Performance

Module toggles should include:
- Current state (`Enabled`, `Disabled`, `Degraded`)
- Startup behavior (on/off)
- Runtime impact note (CPU/memory/network)
- Dependency note (e.g., "Requires image protocol support")

## Feedback and Error UX

- Non-blocking toasts for successful background actions
- Persistent job panel for long tasks (scan, metadata fetch, tag writes)
- Errors include next action, not just failure text
- Degraded state banners for optional services (e.g., metadata provider unavailable)

## Visualizer UX

Visualizer must not hijack the app.

Modes:
- Mini visualizer in now playing panel (default when enabled)
- Focus visualizer panel
- Fullscreen visualizer mode (toggle)

Performance controls:
- FPS cap
- Detail level
- Auto-reduce on low-capability terminals or remote sessions

## Artwork Display Filters

Artwork rendering can include optional display-only stylization filters.

Initial planned filter:
- Pixel-art mode (off by default)

Requirements:
- Must be a UI setting (runtime toggle)
- Must not write stylized artwork back to files or metadata tags
- Should use a small pixel size by default (high-detail pixelation suitable for complex album art)
- Must expose redraw/performance controls because repeated reprocessing can be expensive
- Should cache transformed artwork per source + size + filter settings
- Must have an immediate fallback to normal artwork rendering

## Theming UX

Theme tokens, not hard-coded color names.

Token groups:
- Surface/background layers
- Text states (default, muted, accent, danger)
- Selection and focus ring equivalents
- Borders/dividers
- Meter/graph colors (VU, visualizer, progress)
- Status severity colors

Theme switching should be live-previewable.

## Font and Iconography

Design target:
- `FiraCode Nerd Font Mono` for consistent alignment and Nerd Font icon support

Practical constraint:
- Auric cannot set the terminal font directly; users configure this in their terminal emulator

UX requirements:
- Settings must expose icon mode (`Nerd Font`, `ASCII`)
- Default to Nerd Font icons
- Fall back automatically (or via one-click setting) when glyphs render incorrectly
- Never rely on icons as the only indicator for state/action

## Accessibility / Ergonomics

- High-contrast theme presets
- Configurable keymap profiles (default, vim-heavy, minimal)
- Reduced-motion mode
- Optional confirmation prompts for destructive actions
- Consistent focus indication in color-limited terminals
