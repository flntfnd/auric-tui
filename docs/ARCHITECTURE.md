# Architecture

## Product Goal

Build a cross-platform terminal audio player that pushes beyond typical CLI UX while preserving reliability in low-capability terminals.

## Hard Constraints (and what they imply)

- All terminals: UI must be capability-detected and degrade gracefully.
- All platforms: use portable abstractions with thin OS-specific adapters.
- High-resolution/lossless playback: audio path must avoid unnecessary resampling and expose device capabilities.
- Modular features: architecture must separate feature services and support runtime disable/enable where safe.

## Layered Architecture

### 1. `auric-core` (shared contracts)

Owns stable interfaces and shared types used everywhere.

Responsibilities:
- Feature registry and runtime feature states
- Event bus contracts (pub/sub messages)
- Terminal/audio capability models
- Strongly typed IDs and domain enums
- Error taxonomy conventions

This crate should avoid heavy dependencies.

### 2. `auric-audio` (real-time engine)

Owns all low-latency audio paths.

Responsibilities:
- Decoder backends (primary + fallback strategy)
- Output backends (device enumeration, sample rate negotiation)
- Mixer/transport/queue integration hooks
- DSP graph (EQ, volume normalization, replaygain)
- Analyzer taps feeding visualizers/analytics without blocking playback

Design notes:
- Keep real-time callback threads lock-light and allocation-light.
- Use message passing/ring buffers between UI/network/library and audio threads.
- Visualizer and analytics consume a copied analysis tap, never the playback-critical path.

### 3. `auric-library` (stateful media domain)

Owns persistent media data and file system synchronization.

Responsibilities:
- Directory scan/import (batch + incremental)
- Watched folders (`notify`) and reconciliation
- Playlist CRUD + smart playlists (later phase)
- Tag read/write and artwork extraction/embed
- Remote metadata/artwork orchestration (MusicBrainz, Cover Art Archive, etc.)
- SQLite persistence and migrations (recommended)

Design notes:
- All filesystem watcher events should go through a debounced reconcile queue.
- Treat remote metadata fetch as an optional provider chain with rate limiting and retry policy.
- Track provenance for metadata fields (embedded vs fetched vs user-edited).

### 4. `auric-ui` (TUI + UX layer)

Owns terminal rendering and interaction.

Responsibilities:
- Multi-pane TUI layout engine (library, queue, now playing, playlists, inspector)
- Input system (keyboard maps, mouse actions, command palette)
- Theme engine (colors, spacing, borders, typography-style tokens for terminal)
- Icon/glyph set rendering with Nerd Font-first assets and ASCII fallback
- Terminal capability detection (color depth, mouse, image protocol)
- Artwork rendering adapters (kitty, sixel, iTerm2, text fallback)
- Optional display filter pipeline for artwork rendering (e.g., pixel-art stylization)

Design notes:
- UI should render from a read-only app state snapshot.
- Input emits intents/commands, not direct mutations.
- Every mouse action must have a keyboard equivalent.
- Expensive artwork filters must run off the render hot path and should cache transformed output by source artwork hash + target size + filter params.

### 5. `auric-net` (listen-along + P2P stream)

Owns optional networking features.

Responsibilities:
- Session creation/joining and peer discovery/signaling integration
- Listen-along sync (shared transport state + clock sync)
- Optional direct audio stream to peers
- Network resilience telemetry (latency/jitter/packet loss)

Design notes:
- Split features explicitly:
  - Sync mode: lightweight, peers play local copies in lockstep (best quality)
  - Stream mode: host encodes and streams audio (for friends without local files)
- Stream mode is significantly more complex and should ship after sync mode.

### 6. `auric-app` (composition root)

Wires crates together and applies runtime feature configuration.

Responsibilities:
- Startup initialization and setup wizard
- Plugin/service registration
- Settings persistence and hot-reload where supported
- Shutdown orchestration

## Modular Features Strategy

Auric needs both compile-time and runtime modularity.

### Compile-time modularity (Cargo features)
Use for heavy optional dependencies and licensing-sensitive stacks.
Examples:
- `ffmpeg-backend`
- `image-protocol-kitty`
- `image-protocol-sixel`
- `p2p-stream`
- `analytics-export`

### Runtime modularity (user settings)
Use for behavior toggles that can be enabled/disabled without rebuilding.
Examples:
- visualizer on/off
- EQ on/off
- watched folders on/off
- remote metadata fetch on/off
- analytics on/off

Runtime toggles should transition through states:
- `Disabled`
- `Starting`
- `Enabled`
- `Degraded(reason)`
- `Stopping`

## Capability-Aware UX (required for "all terminals")

Do not assume advanced terminal support.

Capability tiers:
- Tier 0: basic text, no mouse, 16 colors
- Tier 1: 256 colors + mouse
- Tier 2: truecolor + mouse + clipboard helpers
- Tier 3: terminal image protocol available (kitty/sixel/iTerm2)

UX behavior by tier:
- Artwork panel falls back to text metadata card if no image protocol
- Animations reduce frame rate on low throughput/remote terminals
- Visualizer can switch from block waveform to compact text bars
- Mouse-only affordances are always optional enhancements
- Nerd Font icons can fall back to ASCII/text glyphs without breaking layout semantics
- Pixel-art artwork filter is optional and should be auto-disabled/degraded on low-capability or high-latency terminals when redraw cost is too high

## Data Model (minimum stable entities)

- Track
- Album
- Artist
- ArtworkAsset
- Playlist
- PlaylistEntry
- LibraryRoot (watched folder)
- PlaybackSession
- FeatureSetting
- UserActionEvent (analytics)

Recommended persistence:
- SQLite for catalog/playlists/settings/analytics
- File-based cache for artwork thumbnails and fetched metadata responses

## Metadata and Artwork Pipeline

Order of operations for enrichment:
1. Read embedded tags/artwork from file
2. Normalize and persist local metadata
3. If missing fields and remote fetch enabled, query provider chain
4. Show diff/merge UI for destructive metadata writes
5. Write selected metadata/artwork back to file (opt-in or rule-based)

Important separation:
- UI artwork filters (such as pixel-art stylization) are display-only transforms and must never be written back to audio file tags/artwork.

Provider chain (proposed first version):
- MusicBrainz (identification + canonical metadata)
- Cover Art Archive (artwork)
- Discogs (optional, later)

## Audio Backend Strategy (pragmatic)

"Full audio support" requires a backend strategy, not a single decoder.

Recommended approach:
- Default backend: `symphonia` for common formats and clean Rust integration
- Optional compatibility backend: FFmpeg (feature-gated) for wider codec coverage

This avoids blocking the project on a single stack while preserving a path to broad format support.

## P2P Streaming Architecture (phased)

### Phase A: Listen-Along Sync
- Share track identity, playback position, pause/seek, and clock offsets
- Peers with the same track play locally
- Lowest bandwidth and best audio quality

### Phase B: Direct Stream
- Host decodes source and re-encodes stream (codec TBD: Opus recommended for latency)
- Peers receive jitter-buffered audio stream
- Optional fallback relay for NAT traversal failures

Important tradeoff:
- Hi-res/lossless P2P streaming to friends is expensive and brittle across home networks.
- Real-time social listening should default to low-latency streaming (Opus) while local-sync preserves lossless quality.

## Analytics (privacy-first)

Analytics should improve UX, not surveil users.

Collect locally (opt-in):
- command usage frequency
- navigation friction (aborted actions, repeated retries)
- playback errors per backend
- scan/watcher reconciliation failures

Use cases:
- Shortcut discoverability improvements
- Default layout tuning
- Backend stability diagnostics

## Threading / Task Model (recommended)

- UI thread/task: render + input dispatch
- Audio RT thread(s): callback/output path
- App coordinator task: command handling/state updates
- Library workers: scan, metadata IO, tag writes
- Network workers: session + transport
- Background jobs: artwork cache, thumbnailing, analytics compaction

Communication:
- Message bus for commands/events
- Snapshot state store for UI rendering
- Bounded queues between producers and RT audio consumers

## Initial Implementation Slice (first shippable)

Target a narrow but solid local player before advanced features:
- Local playback (FLAC/WAV/MP3/AAC where supported)
- Queue + transport controls
- Library scan + watched folders
- Playlist CRUD
- Keyboard + mouse TUI
- Theme tokens
- Embedded metadata/artwork read/write
- Capability-aware artwork fallback

Defer until later phases:
- remote metadata fetching
- EQ presets and custom filters UI
- advanced visualizer modes
- analytics dashboards
- P2P stream mode
