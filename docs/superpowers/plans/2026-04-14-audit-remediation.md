# Audit Remediation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix all critical, warning, and minor findings from the full codebase audit.

**Architecture:** Targeted fixes across the workspace -- no structural refactors beyond splitting the `auric-app/src/lib.rs` monolith into modules. Each task is a self-contained change that compiles and passes tests independently.

**Tech Stack:** Rust 2021 edition, rusqlite, ratatui, cpal, symphonia, thiserror, anyhow

---

## Task 1: Replace `count_table` SQL interpolation with enum (Critical)

**Files:**
- Modify: `crates/auric-library/src/db.rs:1032-1035`

- [ ] **Step 1: Write the failing test**

Add to `crates/auric-library/src/db.rs` in the `#[cfg(test)] mod tests` block:

```rust
#[test]
fn stats_returns_correct_counts_after_inserts() {
    let db = Database::open_in_memory_for_tests().unwrap();
    let stats = db.stats().unwrap();
    assert_eq!(stats.track_count, 0);
    assert_eq!(stats.playlist_count, 0);
    assert_eq!(stats.library_root_count, 0);
}
```

- [ ] **Step 2: Run test to verify it passes (baseline)**

Run: `cargo test -p auric-library stats_returns_correct_counts`
Expected: PASS (this validates the existing behavior before refactor)

- [ ] **Step 3: Replace `count_table` with `StatsTable` enum**

Replace the `count_table` function (lines 1032-1035) with:

```rust
#[derive(Debug, Clone, Copy)]
enum StatsTable {
    AppSettings,
    LibraryRoots,
    Tracks,
    ArtworkAssets,
    TrackArtwork,
    Playlists,
    PlaylistEntries,
}

impl StatsTable {
    fn as_sql(self) -> &'static str {
        match self {
            Self::AppSettings => "SELECT COUNT(*) FROM app_settings",
            Self::LibraryRoots => "SELECT COUNT(*) FROM library_roots",
            Self::Tracks => "SELECT COUNT(*) FROM tracks",
            Self::ArtworkAssets => "SELECT COUNT(*) FROM artwork_assets",
            Self::TrackArtwork => "SELECT COUNT(*) FROM track_artwork",
            Self::Playlists => "SELECT COUNT(*) FROM playlists",
            Self::PlaylistEntries => "SELECT COUNT(*) FROM playlist_entries",
        }
    }
}

fn count_table(conn: &Connection, table: StatsTable) -> Result<i64, DbError> {
    Ok(conn.query_row(table.as_sql(), [], |row| row.get(0))?)
}
```

- [ ] **Step 4: Update `stats()` callers**

Update `stats()` method (lines 1002-1029) to use the enum:

```rust
pub fn stats(&self) -> Result<DatabaseStats, DbError> {
    let settings_count = count_table(&self.conn, StatsTable::AppSettings)?;
    let library_root_count = count_table(&self.conn, StatsTable::LibraryRoots)?;
    let track_count = count_table(&self.conn, StatsTable::Tracks)?;
    let artwork_asset_count = count_table(&self.conn, StatsTable::ArtworkAssets)?;
    let track_artwork_count = count_table(&self.conn, StatsTable::TrackArtwork)?;
    let playlist_count = count_table(&self.conn, StatsTable::Playlists)?;
    let playlist_entry_count = count_table(&self.conn, StatsTable::PlaylistEntries)?;
    // ... rest unchanged
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p auric-library`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/auric-library/src/db.rs
git commit -m "fix: replace SQL string interpolation in count_table with enum

Eliminates SQL injection vector by using a StatsTable enum with
static query strings instead of format! interpolation."
```

---

## Task 2: Add `DbError::IntegrityCheck` variant and fix `quick_check` (Minor)

**Files:**
- Modify: `crates/auric-library/src/db.rs:289-302, 390-401`

- [ ] **Step 1: Add `IntegrityCheck` variant to `DbError`**

Add after the `NotFound` variant (line 301):

```rust
    #[error("integrity check failed: {0}")]
    IntegrityCheck(String),
```

- [ ] **Step 2: Update `quick_check` to use new variant**

Replace the error in `quick_check` (line 397-399):

```rust
            Err(DbError::IntegrityCheck(result))
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p auric-library`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/auric-library/src/db.rs
git commit -m "fix: use DbError::IntegrityCheck instead of NotFound for integrity failures"
```

---

## Task 3: Fix `now_ms()` silent epoch-zero fallback (Warning)

**Files:**
- Modify: `crates/auric-library/src/db.rs:1129-1134`

- [ ] **Step 1: Write the test**

Add to `crates/auric-library/src/db.rs` tests:

```rust
#[test]
fn now_ms_returns_plausible_timestamp() {
    let ts = now_ms();
    // 2024-01-01 in ms
    assert!(ts > 1_704_067_200_000, "timestamp {ts} is too small");
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test -p auric-library now_ms_returns_plausible`
Expected: PASS

- [ ] **Step 3: Replace `now_ms` with a version that panics on clock failure**

Replace `now_ms` (lines 1129-1134):

```rust
fn now_ms() -> i64 {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before UNIX_EPOCH");
    dur.as_millis() as i64
}
```

A system clock before epoch is a configuration error that should surface immediately, not silently corrupt every timestamp in the database.

- [ ] **Step 4: Run tests**

Run: `cargo test -p auric-library`
Expected: All tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/auric-library/src/db.rs
git commit -m "fix: panic on pre-epoch system clock instead of silently writing zero timestamps"
```

---

## Task 4: Log deserialization errors in `load_playback_state` (Warning)

**Files:**
- Modify: `crates/auric-app/src/lib.rs:331-341`

- [ ] **Step 1: Replace silent fallback with eprintln warning**

Replace `load_playback_state` (lines 331-341):

```rust
fn load_playback_state(db: &Database) -> Result<PlaybackState> {
    let raw = db.get_setting_json(PLAYBACK_STATE_SETTING_KEY)?;
    let mut state = match raw {
        Some(value) => serde_json::from_value::<PlaybackState>(value).unwrap_or_else(|err| {
            eprintln!("warning: failed to deserialize playback state, resetting: {err}");
            PlaybackState::default()
        }),
        None => PlaybackState::default(),
    };
    normalize_playback_state(&mut state);
    save_playback_state(db, &state)?;
    Ok(state)
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p auric-app`
Expected: All tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/auric-app/src/lib.rs
git commit -m "fix: log warning when playback state deserialization fails instead of silent fallback"
```

---

## Task 5: Log watcher registration errors (Warning)

**Files:**
- Modify: `crates/auric-library/src/watch.rs:160-169`

- [ ] **Step 1: Replace silent error discard with eprintln**

Replace the `Err(_err)` branch (lines 166-168):

```rust
                Err(err) => {
                    eprintln!(
                        "warning: could not watch root '{}': {err}",
                        root.path
                    );
                    skipped_root_count = skipped_root_count.saturating_add(1);
                }
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p auric-library`
Expected: All tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/auric-library/src/watch.rs
git commit -m "fix: log watcher registration errors instead of silently discarding them"
```

---

## Task 6: Validate path in `root add` command (Warning)

**Files:**
- Modify: `crates/auric-app/src/lib.rs:737-754`

- [ ] **Step 1: Add path validation before persisting**

In `handle_root_command`, after extracting `path` (line 740) and before `upsert_library_root` (line 748), add validation:

```rust
        "add" => {
            let path = args
                .get(1)
                .map(String::as_str)
                .ok_or_else(|| anyhow::anyhow!("usage: auric root add <path> [--watched]"))?;
            let resolved = std::path::Path::new(path);
            if !resolved.exists() {
                bail!("path does not exist: {path}");
            }
            if !resolved.is_dir() {
                bail!("path is not a directory: {path}");
            }
            let watched = args
                .iter()
                .skip(2)
                .any(|a| a == "--watched" || a == "watched");
            let row = app.db.upsert_library_root(&LibraryRoot {
                path: path.to_string(),
                watched,
            })?;
            println!(
                "root saved: {} | watched={} | {}",
                row.id, row.watched, row.path
            );
        }
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p auric-app`
Expected: All tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/auric-app/src/lib.rs
git commit -m "fix: validate path exists and is a directory before adding library root"
```

---

## Task 7: Reject path traversal in `parse_local_source_uri` (Warning)

**Files:**
- Modify: `crates/auric-audio/src/lib.rs:230-254`

- [ ] **Step 1: Write the failing test**

Add to `crates/auric-audio/src/lib.rs` tests:

```rust
#[test]
fn rejects_path_traversal_in_source_uri() {
    assert!(parse_local_source_uri("file:///music/../../../etc/passwd").is_err());
    assert!(parse_local_source_uri("/music/../../../etc/passwd").is_err());
    assert!(parse_local_source_uri("../../../etc/passwd").is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p auric-audio rejects_path_traversal`
Expected: FAIL

- [ ] **Step 3: Add `..` component rejection**

At the end of `parse_local_source_uri`, before the final `Ok(PathBuf::from(trimmed))`, add a validation helper. Replace the full function:

```rust
fn parse_local_source_uri(source_uri: &str) -> Result<PathBuf, AudioError> {
    let trimmed = source_uri.trim();
    if trimmed.is_empty() {
        return Err(AudioError::UnsupportedSourceUri(
            "empty source URI".to_string(),
        ));
    }

    let path = if let Some(rest) = trimmed.strip_prefix("file://") {
        if rest.is_empty() {
            return Err(AudioError::UnsupportedSourceUri(source_uri.to_string()));
        }
        #[cfg(windows)]
        let path_text = rest.strip_prefix('/').unwrap_or(rest);
        #[cfg(not(windows))]
        let path_text = rest;
        PathBuf::from(path_text)
    } else if trimmed.contains("://") {
        return Err(AudioError::UnsupportedSourceUri(trimmed.to_string()));
    } else {
        PathBuf::from(trimmed)
    };

    if path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(AudioError::UnsupportedSourceUri(
            format!("path traversal not allowed: {trimmed}"),
        ));
    }

    Ok(path)
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p auric-audio`
Expected: All tests pass including the new one

- [ ] **Step 5: Commit**

```bash
git add crates/auric-audio/src/lib.rs
git commit -m "fix: reject path traversal sequences in source URIs"
```

---

## Task 8: Sanitize theme name in `FsThemeStore::path_for` (Warning)

**Files:**
- Modify: `crates/auric-ui/src/theme.rs:119-121`

- [ ] **Step 1: Write the failing test**

Add to `crates/auric-ui/src/theme.rs` tests:

```rust
#[test]
fn rejects_theme_name_with_path_traversal() {
    let dir = tempdir().unwrap();
    let store = FsThemeStore::new(dir.path());
    assert!(store.load("../../etc/something").is_err());
    assert!(store.load("foo/bar").is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p auric-ui rejects_theme_name`
Expected: FAIL (it currently attempts to load the traversed path)

- [ ] **Step 3: Add name validation to `path_for`**

Replace `path_for` (lines 119-121):

```rust
    fn path_for(&self, name: &str) -> Result<PathBuf, UiError> {
        if name.contains('/') || name.contains('\\') || name.contains("..") || name.is_empty() {
            return Err(UiError::Theme(format!("invalid theme name: {name}")));
        }
        Ok(self.base_dir.join(format!("{name}.toml")))
    }
```

- [ ] **Step 4: Update callers of `path_for`**

Update `load_palette` (line 114-117):

```rust
    pub fn load_palette(&self, name: &str) -> Result<Palette, UiError> {
        let theme = self.load(name)?;
        Ok(Palette::from_theme(&theme))
    }
```

(No change needed -- `load` calls `path_for` internally.)

Update `ThemeStore::load` impl (line 125-145). Change the `path_for` call:

```rust
    fn load(&self, name: &str) -> Result<Theme, UiError> {
        let path = self.path_for(name)?;
```

Update `ThemeStore::list` impl -- `path_for` is not called in `list`, so no change needed.

- [ ] **Step 5: Run tests**

Run: `cargo test -p auric-ui`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/auric-ui/src/theme.rs
git commit -m "fix: validate theme name to prevent directory traversal"
```

---

## Task 9: Add `visualizer_*` color tokens to `Palette` (Minor)

**Files:**
- Modify: `crates/auric-ui/src/theme.rs:8-23, 25-43, 46-96`

- [ ] **Step 1: Add fields to `Palette` struct**

Add after `progress_fill` (line 22):

```rust
    pub visualizer_low: Color,
    pub visualizer_mid: Color,
    pub visualizer_high: Color,
```

- [ ] **Step 2: Add defaults**

Add in `Default::default()` (before the closing brace, after `progress_fill`):

```rust
            visualizer_low: Color::Rgb(99, 179, 237),
            visualizer_mid: Color::Rgb(79, 209, 197),
            visualizer_high: Color::Rgb(246, 173, 85),
```

- [ ] **Step 3: Add parsing in `from_theme`**

Add before the final `palette` return in `from_theme`:

```rust
        if let Some(v) = get("colors.visualizer_low") {
            palette.visualizer_low = v;
        }
        if let Some(v) = get("colors.visualizer_mid") {
            palette.visualizer_mid = v;
        }
        if let Some(v) = get("colors.visualizer_high") {
            palette.visualizer_high = v;
        }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p auric-ui`
Expected: All tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/auric-ui/src/theme.rs
git commit -m "feat: parse visualizer_low/mid/high color tokens from theme files"
```

---

## Task 10: Make `AudioEngine` fields private (Warning)

**Files:**
- Modify: `crates/auric-audio/src/lib.rs:75-78, 87-92`

- [ ] **Step 1: Make fields private and add accessors**

Replace the `AudioEngine` struct and `new` (lines 75-92):

```rust
pub struct AudioEngine {
    taps: Vec<Box<dyn AnalysisTap>>,
    dsp_chain: Vec<Box<dyn DspNode>>,
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEngine {
    pub fn new() -> Self {
        Self {
            taps: Vec::new(),
            dsp_chain: Vec::new(),
        }
    }

    pub fn add_tap(&mut self, tap: Box<dyn AnalysisTap>) {
        self.taps.push(tap);
    }

    pub fn add_dsp_node(&mut self, node: Box<dyn DspNode>) {
        self.dsp_chain.push(node);
    }
```

- [ ] **Step 2: Verify no external access to fields**

Run: `cargo build 2>&1`
Expected: Clean build (no code outside the crate accesses these fields directly)

- [ ] **Step 3: Commit**

```bash
git add crates/auric-audio/src/lib.rs
git commit -m "fix: make AudioEngine fields private with controlled accessors"
```

---

## Task 11: Add real-time safety doc comments to audio traits (Critical)

**Files:**
- Modify: `crates/auric-audio/src/lib.rs:38-45`

- [ ] **Step 1: Add safety documentation**

Replace the trait definitions (lines 38-45):

```rust
/// A node in the DSP processing chain.
///
/// # Real-time safety
///
/// Implementations of `process` (when added) MUST be real-time safe:
/// no heap allocations, no locks/mutexes, no blocking I/O, no Objective-C
/// messaging. Cross-thread communication must use lock-free primitives only.
pub trait DspNode: Send {
    fn id(&self) -> &'static str;
    fn enabled(&self) -> bool;
}

/// A tap that receives copies of audio frames for analysis (metering, FFT, etc).
///
/// # Real-time safety
///
/// `push_frame` is called from the audio callback thread. Implementations MUST
/// be allocation-free and lock-free. Use a lock-free ring buffer (e.g. `rtrb`)
/// to transfer data to a non-real-time consumer thread.
pub trait AnalysisTap: Send + Sync {
    fn push_frame(&self, _interleaved: &[f32], _format: StreamFormat) {}
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p auric-audio`
Expected: All tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/auric-audio/src/lib.rs
git commit -m "docs: add real-time safety contracts to DspNode and AnalysisTap traits"
```

---

## Task 12: Remove dead code -- unused traits, structs, and types (Minor)

**Files:**
- Modify: `crates/auric-library/src/lib.rs`
- Modify: `crates/auric-core/src/lib.rs`
- Modify: `crates/auric-audio/src/lib.rs`

- [ ] **Step 1: Remove unused traits and types from `auric-library/src/lib.rs`**

Remove the following (lines 28-72): `LibraryScanner`, `FolderWatcher`, `MetadataProvider`, `ArtworkProvider`, `PlaylistStore` traits, and `MetadataUpdate`, `ArtworkAsset` structs. Also remove `LibraryError` enum (lines 74-82) since it's only used by the removed traits.

Keep: `LibraryRoot`, `TrackRecord`, `pub mod db/scan/watch`, the `async_trait` import (check if still needed after removal -- if not, remove it too).

The file should look like:

```rust
use auric_core::TrackId;

pub mod db;
pub mod scan;
pub mod watch;

#[derive(Debug, Clone)]
pub struct LibraryRoot {
    pub path: String,
    pub watched: bool,
}

#[derive(Debug, Clone)]
pub struct TrackRecord {
    pub id: TrackId,
    pub path: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<i64>,
    pub sample_rate: Option<i64>,
    pub channels: Option<i64>,
    pub bit_depth: Option<i64>,
    pub file_mtime_ms: Option<i64>,
}
```

- [ ] **Step 2: Remove `CapabilityProbe` from `auric-core/src/lib.rs`**

Remove the trait (lines 247-249):

```rust
pub trait CapabilityProbe {
    fn probe_terminal(&self) -> TerminalCapabilities;
}
```

- [ ] **Step 3: Remove `PlaybackRequest` from `auric-audio/src/lib.rs`**

Remove the struct (lines 14-17):

```rust
#[derive(Debug, Clone)]
pub struct PlaybackRequest {
    pub track_id: TrackId,
    pub source_uri: String,
}
```

- [ ] **Step 4: Remove unused traits from `auric-ui/src/lib.rs`**

Remove `TerminalBackend`, `ImageRenderer`, `InputMapper` traits (lines 27-40) and the `UiAction` struct (lines 21-24). Remove `async_trait` import and `AppCommand`/`TerminalCapabilities` from imports if no longer used.

The file should look like:

```rust
use std::collections::BTreeMap;

pub mod shell;
pub mod theme;

pub use shell::{
    render_once_to_text, run_interactive, run_interactive_with_handlers,
    run_interactive_with_refresh, FocusPane, IconMode, PaletteCommandResult, RunOptions,
    ShellListItem, ShellSnapshot, ShellState, ShellTrackItem,
};
pub use theme::{FsThemeStore, Palette};

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub tokens: BTreeMap<String, String>,
}

pub trait ThemeStore: Send + Sync {
    fn load(&self, name: &str) -> Result<Theme, UiError>;
    fn list(&self) -> Result<Vec<String>, UiError>;
}

#[derive(Debug, thiserror::Error)]
pub enum UiError {
    #[error("terminal error: {0}")]
    Terminal(String),
    #[error("theme error: {0}")]
    Theme(String),
}
```

- [ ] **Step 5: Build and test**

Run: `cargo build 2>&1 && cargo test`
Expected: Clean build, all tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/auric-library/src/lib.rs crates/auric-core/src/lib.rs crates/auric-audio/src/lib.rs crates/auric-ui/src/lib.rs
git commit -m "chore: remove dead trait definitions and unused types

Removes LibraryScanner, FolderWatcher, MetadataProvider, ArtworkProvider,
PlaylistStore, CapabilityProbe, PlaybackRequest, TerminalBackend,
ImageRenderer, InputMapper, UiAction, MetadataUpdate, ArtworkAsset,
and LibraryError -- all defined but never implemented or used."
```

---

## Task 13: Remove unused `auric-net` dependency from `auric-app` (Minor)

**Files:**
- Modify: `crates/auric-app/Cargo.toml`
- Modify: `crates/auric-app/src/lib.rs` (if any import exists)

- [ ] **Step 1: Remove from Cargo.toml**

Remove this line from `crates/auric-app/Cargo.toml`:

```toml
auric-net = { path = "../auric-net" }
```

- [ ] **Step 2: Build**

Run: `cargo build 2>&1`
Expected: Clean build (no code imports from `auric-net`)

- [ ] **Step 3: Commit**

```bash
git add crates/auric-app/Cargo.toml
git commit -m "chore: remove unused auric-net dependency from auric-app"
```

---

## Task 14: Remove unused `async-trait` dependencies (Minor)

**Files:**
- Modify: `crates/auric-library/Cargo.toml`
- Modify: `crates/auric-ui/Cargo.toml`
- Modify: `crates/auric-net/Cargo.toml`
- Modify: `crates/auric-library/src/lib.rs` (remove import if present)

- [ ] **Step 1: Remove from library, check if still used**

After Task 12 removed the async traits from `auric-library/src/lib.rs`, the `async-trait` import is unused. Remove it from `crates/auric-library/Cargo.toml`:

```toml
async-trait.workspace = true
```

- [ ] **Step 2: Remove from auric-ui Cargo.toml**

After Task 12 removed the async traits from `auric-ui/src/lib.rs`, remove `async-trait` from `crates/auric-ui/Cargo.toml`.

- [ ] **Step 3: Build**

Run: `cargo build 2>&1`
Expected: Clean build

- [ ] **Step 4: Commit**

```bash
git add crates/auric-library/Cargo.toml crates/auric-ui/Cargo.toml
git commit -m "chore: remove unused async-trait dependency from library and ui crates"
```

---

## Task 15: Remove unused `tokio` from workspace dependencies (Minor)

**Files:**
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Remove tokio line**

Remove from `[workspace.dependencies]`:

```toml
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time"] }
```

- [ ] **Step 2: Build**

Run: `cargo build 2>&1`
Expected: Clean build

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "chore: remove unused tokio from workspace dependencies"
```

---

## Task 16: Update yanked `lofty` dependency (Dependency advisory)

**Files:**
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Check latest lofty version**

Run: `cargo search lofty --limit 1`

- [ ] **Step 2: Update version in workspace Cargo.toml**

Update the `lofty` line in `[workspace.dependencies]` to the latest non-yanked version (likely `0.23.3` or newer, or a `0.24.x` release):

```toml
lofty = "<latest>"
```

- [ ] **Step 3: Build and test**

Run: `cargo build 2>&1 && cargo test`
Expected: Clean build, all tests pass. If there are API changes, fix them.

- [ ] **Step 4: Run cargo audit to verify**

Run: `cargo audit`
Expected: The `lofty` yanked warning is gone

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: update lofty to latest non-yanked version"
```

---

## Task 17: Add `normalize_path` canonicalization (Minor)

**Files:**
- Modify: `crates/auric-library/src/scan.rs:416-423`

- [ ] **Step 1: Write the failing test**

Add to `crates/auric-library/src/scan.rs` tests:

```rust
#[test]
fn normalize_path_resolves_parent_components() {
    let dir = tempdir().unwrap();
    let sub = dir.path().join("a").join("b");
    std::fs::create_dir_all(&sub).unwrap();
    let with_dotdot = sub.join("..").join("b");
    let normalized = normalize_path(&with_dotdot).unwrap();
    let canonical = sub.to_string_lossy().to_string();
    assert_eq!(normalized, canonical);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p auric-library normalize_path_resolves`
Expected: FAIL

- [ ] **Step 3: Update `normalize_path` to canonicalize**

Replace `normalize_path` (lines 416-423):

```rust
fn normalize_path(path: &Path) -> Result<String, ScanError> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    match path.canonicalize() {
        Ok(canonical) => Ok(canonical.to_string_lossy().to_string()),
        Err(_) => Ok(path.to_string_lossy().to_string()),
    }
}
```

The fallback to non-canonical handles paths where the file exists at scan time but canonicalize fails for other reasons.

- [ ] **Step 4: Run tests**

Run: `cargo test -p auric-library`
Expected: All tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/auric-library/src/scan.rs
git commit -m "fix: canonicalize paths in normalize_path to prevent duplicate track entries"
```

---

## Task 18: Log stats fallback in `build_shell_snapshot` (Minor)

**Files:**
- Modify: `crates/auric-app/src/lib.rs:2115`

- [ ] **Step 1: Replace silent fallback with logged fallback**

Replace line 2115:

```rust
    let stats = app.db.stats().unwrap_or_else(|err| {
        eprintln!("warning: failed to load database stats: {err}");
        app.report.stats.clone()
    });
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p auric-app`
Expected: All tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/auric-app/src/lib.rs
git commit -m "fix: log warning when database stats fallback is used"
```

---

## Task 19: Add `thiserror` to `auric-core` and remove unused `thiserror` from `auric-core` dep check (Minor)

`auric-core/Cargo.toml` already has thiserror. This task is a no-op -- skip.

---

## Task 20: Refactor `Palette::from_theme` to data-driven approach (Minor)

**Files:**
- Modify: `crates/auric-ui/src/theme.rs:46-96`

- [ ] **Step 1: Replace repetitive if-let chains**

Replace `from_theme` (lines 46-96) with:

```rust
impl Palette {
    pub fn from_theme(theme: &Theme) -> Self {
        let mut palette = Self::default();
        let get = |key: &str| theme.tokens.get(key).and_then(|v| color_from_hex(v));

        let mappings: &[(&str, &mut Color)] = &mut [
            ("colors.surface_0", &mut palette.surface_0),
            ("colors.surface_1", &mut palette.surface_1),
            ("colors.surface_2", &mut palette.surface_2),
            ("colors.text", &mut palette.text),
            ("colors.text_muted", &mut palette.text_muted),
            ("colors.accent", &mut palette.accent),
            ("colors.accent_2", &mut palette.accent_2),
            ("colors.danger", &mut palette.danger),
            ("colors.warning", &mut palette.warning),
            ("colors.success", &mut palette.success),
            ("colors.border", &mut palette.border),
            ("colors.focus", &mut palette.focus),
            ("colors.selection_bg", &mut palette.selection_bg),
            ("colors.progress_fill", &mut palette.progress_fill),
            ("colors.visualizer_low", &mut palette.visualizer_low),
            ("colors.visualizer_mid", &mut palette.visualizer_mid),
            ("colors.visualizer_high", &mut palette.visualizer_high),
        ];

        for (key, field) in mappings {
            if let Some(v) = get(key) {
                **field = v;
            }
        }

        palette
    }
}
```

Note: This task depends on Task 9 (visualizer fields added first).

- [ ] **Step 2: Run tests**

Run: `cargo test -p auric-ui`
Expected: All tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/auric-ui/src/theme.rs
git commit -m "refactor: use data-driven approach for Palette::from_theme"
```

---

## Task Dependency Graph

```
Task 1 (count_table enum) ─── independent
Task 2 (DbError::IntegrityCheck) ─── independent
Task 3 (now_ms panic) ─── independent
Task 4 (log playback deser) ─── independent
Task 5 (log watcher errors) ─── independent
Task 6 (validate root add) ─── independent
Task 7 (path traversal URI) ─── independent
Task 8 (theme name validation) ─── independent
Task 9 (visualizer tokens) ─── independent
Task 10 (AudioEngine private) ─── independent
Task 11 (audio trait docs) ─── independent
Task 12 (dead code removal) ─── independent
Task 13 (remove auric-net dep) ─── after Task 12
Task 14 (remove async-trait) ─── after Task 12
Task 15 (remove tokio) ─── independent
Task 16 (update lofty) ─── independent
Task 17 (normalize_path) ─── independent
Task 18 (log stats fallback) ─── independent
Task 20 (from_theme refactor) ─── after Task 9
```

Tasks 1-12, 15-18 can all run in parallel. Task 13 and 14 depend on Task 12. Task 20 depends on Task 9.
