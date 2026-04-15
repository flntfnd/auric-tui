# Roadmap

## Phase 0: Foundation (current)

Goals:
- Establish crate boundaries and shared contracts
- Define feature toggles and capability detection model
- Document architecture and scope decisions

Exit criteria:
- Workspace scaffold exists
- Core interfaces exist for playback/library/UI/network
- Implementation plan agreed

## Phase 1: Local Playback MVP (UX-first)

Deliver:
- TUI shell with panels (library, queue, now playing)
- Keyboard shortcuts and mouse support
- Audio playback transport (play/pause/seek/next/prev/volume)
- Queue management
- Library scan/import of directories
- Watched folder updates with debounce/reconcile
- Playlist create/edit/delete
- Embedded metadata and artwork read
- Theme system (token-based) and settings screen (feature toggles)

Acceptance checks:
- Works in macOS/Linux/Windows terminals (at least Tier 0-2 capability modes)
- Handles large libraries without UI stalls (incremental loading)
- Playback continues during heavy scans

## Phase 2: Metadata + Artwork Enrichment

Deliver:
- MusicBrainz identification and metadata fetch
- Cover Art Archive integration
- Metadata diff/merge flow before writing tags
- Artwork cache and thumbnail pipeline
- Retry/rate-limit policies and provider health states

Acceptance checks:
- Missing album art can be fetched and embedded
- User edits are not overwritten silently
- Provider outages degrade gracefully

## Phase 3: DSP and Visualizer

Deliver:
- Equalizer engine (10-band minimum) with presets
- ReplayGain integration refinement
- Visualizer pipeline + multiple render modes
- Optional artwork pixel-art display filter (display-only, cacheable, off by default)
- Performance controls (FPS, complexity, disable on low capability terminals)

Acceptance checks:
- EQ changes do not glitch playback
- Visualizer can be disabled entirely and releases resources
- Pixel-art artwork mode never modifies file metadata/artwork and can be disabled without affecting normal artwork rendering

## Phase 4: Analytics + UX Optimization

Deliver:
- Local analytics collection (opt-in)
- Error and usage instrumentation
- UX insights screen/report (local)
- Shortcut suggestion system based on usage

Acceptance checks:
- Analytics off means no event collection
- Export/removal tools work and are obvious

## Phase 5: Social Listening (P2P)

Phase 5A (recommended first):
- Session creation/join
- Listen-along sync (track/position/transport)
- Clock sync and drift correction

Phase 5B:
- Direct audio stream mode (Opus, low latency)
- Jitter buffer and adaptive quality
- Relay fallback support

Acceptance checks:
- Sync mode remains stable under jitter
- Stream mode degrades quality before causing stalls

## Cross-Cutting Workstreams (all phases)

- Accessibility and input ergonomics
- Terminal capability testing matrix
- Performance profiling and memory budgets
- Plugin/module disable paths
- Configuration migration/versioning
- Crash recovery and state persistence
