# Auric Drift Standalone Crate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port the Auric Drift shuffle algorithm and audio analyzer to a standalone, cross-platform Rust crate at `~/Dev/auric/auric-drift/` that both the desktop Swift app (via C FFI) and the TUI (as a Cargo dependency) can consume.

**Architecture:** A single Rust crate with three modules: `engine` (shuffle scoring/selection), `analyzer` (DSP feature extraction), and `ffi` (C-compatible API for Swift). The engine is pure computation. The analyzer uses `symphonia` for audio decoding and `rustfft` for spectral analysis. All CPU-bound work uses `rayon` for parallelism. The crate compiles as both a Rust library (`rlib`) and a C static library (`staticlib`).

**Tech Stack:** Rust, rayon, symphonia, rustfft, cbindgen

---

## File Structure

```
~/Dev/auric/auric-drift/
  Cargo.toml
  cbindgen.toml
  src/
    lib.rs              -- Public API, re-exports
    types.rs            -- TrackSnapshot, DriftConfig, DriftHistory, ShuffleMode, DriftFeatures
    engine.rs           -- DriftEngine: shuffle, scoring, selection
    genre.rs            -- GenreCompatibilityMatrix
    camelot.rs          -- CamelotWheel harmonic compatibility
    analyzer.rs         -- Audio feature extraction (BPM, key, energy, brightness)
    ffi.rs              -- C FFI boundary for Swift consumption
  tests/
    engine_tests.rs     -- Integration tests for shuffle behavior
    analyzer_tests.rs   -- Integration tests with test audio fixtures
    camelot_tests.rs    -- Camelot wheel compatibility tests
```

## Dependency Graph

```
auric-drift (new, standalone)
  rayon        -- parallel audio analysis
  symphonia    -- audio decoding (cross-platform)
  rustfft      -- FFT for spectral analysis
  thiserror    -- error types
  serde        -- optional serialization for config

Desktop Swift app: links auric-drift as a .a static library via C FFI
TUI auric-app: depends on auric-drift as a Cargo path dependency
```

---

### Task 1: Scaffold the crate

**Files:**
- Create: `~/Dev/auric/auric-drift/Cargo.toml`
- Create: `~/Dev/auric/auric-drift/src/lib.rs`
- Create: `~/Dev/auric/auric-drift/src/types.rs`

- [ ] **Step 1: Create the crate directory and Cargo.toml**

```bash
mkdir -p ~/Dev/auric/auric-drift/src
```

Write `~/Dev/auric/auric-drift/Cargo.toml`:

```toml
[package]
name = "auric-drift"
version = "0.1.0"
edition = "2021"
license = "MIT OR Apache-2.0"
description = "Auric Drift intelligent shuffle algorithm and audio feature analyzer"

[lib]
crate-type = ["rlib", "staticlib"]

[dependencies]
rayon = "1"
rustfft = "6"
symphonia = { version = "0.5.5", default-features = false, features = [
    "aac", "adpcm", "alac", "flac", "isomp4", "mkv",
    "mp1", "mp2", "mp3", "ogg", "pcm", "vorbis", "wav",
] }
rand = "0.9"
thiserror = "2"
serde = { version = "1", features = ["derive"], optional = true }

[features]
default = []
serde = ["dep:serde"]
ffi = []

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Write the types module**

Write `~/Dev/auric/auric-drift/src/types.rs`:

```rust
use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TrackSnapshot {
    pub id: String,
    pub artist: String,
    pub album: String,
    pub genre: Option<String>,
    pub track_number: Option<i32>,
    pub last_played_ms: Option<i64>,
    pub play_count: i32,
    pub skip_count: i32,
    pub drift_indexed: bool,
    pub drift_bpm: Option<f32>,
    pub drift_key: Option<i32>,
    pub drift_energy: Option<f32>,
    pub drift_brightness: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ShuffleMode {
    Smart,
    Random,
    Artist,
    Album,
    Genre,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DriftConfig {
    pub artist_separation: usize,
    pub album_separation: usize,
    pub genre_separation: usize,
    pub freshness_decay_hours: f64,
    pub skip_penalty_weight: f64,
    pub discovery_boost: f64,
    pub genre_transition_smoothing: bool,
    pub harmonic_mixing: bool,
    pub harmonic_weight: f64,
    pub bpm_continuity: bool,
    pub max_bpm_delta: f32,
    pub energy_smoothing: bool,
    pub max_energy_delta: f32,
    pub brightness_smoothing: bool,
    pub max_brightness_delta: f32,
}

impl Default for DriftConfig {
    fn default() -> Self {
        Self {
            artist_separation: 8,
            album_separation: 4,
            genre_separation: 3,
            freshness_decay_hours: 48.0,
            skip_penalty_weight: 0.3,
            discovery_boost: 0.15,
            genre_transition_smoothing: true,
            harmonic_mixing: true,
            harmonic_weight: 0.6,
            bpm_continuity: true,
            max_bpm_delta: 12.0,
            energy_smoothing: true,
            max_energy_delta: 0.35,
            brightness_smoothing: true,
            max_brightness_delta: 0.4,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DriftHistory {
    recent_ids: VecDeque<String>,
    recent_artists: VecDeque<String>,
    recent_albums: VecDeque<String>,
    recent_genres: VecDeque<String>,
    max_history: usize,
}

impl DriftHistory {
    pub fn new() -> Self {
        Self::with_capacity(200)
    }

    pub fn with_capacity(max: usize) -> Self {
        Self {
            recent_ids: VecDeque::with_capacity(max),
            recent_artists: VecDeque::with_capacity(max),
            recent_albums: VecDeque::with_capacity(max),
            recent_genres: VecDeque::with_capacity(max),
            max_history: max,
        }
    }

    pub fn record(&mut self, track: &TrackSnapshot) {
        self.recent_ids.push_back(track.id.clone());
        self.recent_artists.push_back(track.artist.to_lowercase());
        self.recent_albums.push_back(track.album.to_lowercase());
        self.recent_genres
            .push_back(track.genre.as_deref().unwrap_or("").to_lowercase());

        if self.recent_ids.len() > self.max_history {
            self.recent_ids.pop_front();
            self.recent_artists.pop_front();
            self.recent_albums.pop_front();
            self.recent_genres.pop_front();
        }
    }

    pub fn last_index_of_artist(&self, artist: &str, window: usize) -> Option<usize> {
        Self::last_index_in(&self.recent_artists, &artist.to_lowercase(), window)
    }

    pub fn last_index_of_album(&self, album: &str, window: usize) -> Option<usize> {
        Self::last_index_in(&self.recent_albums, &album.to_lowercase(), window)
    }

    pub fn last_index_of_genre(&self, genre: &str, window: usize) -> Option<usize> {
        Self::last_index_in(&self.recent_genres, &genre.to_lowercase(), window)
    }

    fn last_index_in(deque: &VecDeque<String>, key: &str, window: usize) -> Option<usize> {
        let len = deque.len();
        let start = len.saturating_sub(window);
        for i in (start..len).rev() {
            if deque[i] == key {
                return Some(len - 1 - i);
            }
        }
        None
    }
}

impl Default for DriftHistory {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DriftFeatures {
    pub bpm: f32,
    pub key: i32,
    pub energy: f32,
    pub brightness: f32,
    pub dynamic_range: f32,
}

#[derive(Debug, thiserror::Error)]
pub enum AnalyzerError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("decode error: {0}")]
    Decode(String),
    #[error("empty audio data")]
    EmptyAudio,
}

#[derive(Debug, Clone)]
pub struct AnalysisProgress {
    pub completed: usize,
    pub total: usize,
}
```

- [ ] **Step 3: Write the lib.rs re-exports**

Write `~/Dev/auric/auric-drift/src/lib.rs`:

```rust
pub mod types;
pub mod camelot;
pub mod genre;
pub mod engine;
pub mod analyzer;

#[cfg(feature = "ffi")]
pub mod ffi;

pub use types::{
    AnalysisProgress, AnalyzerError, DriftConfig, DriftFeatures, DriftHistory, ShuffleMode,
    TrackSnapshot,
};
pub use engine::DriftEngine;
pub use analyzer::DriftAnalyzer;
pub use camelot::CamelotWheel;
```

- [ ] **Step 4: Verify it compiles**

Run: `cd ~/Dev/auric/auric-drift && cargo check 2>&1`
Expected: Errors for missing modules (engine, analyzer, camelot, genre). That's correct at this stage.

- [ ] **Step 5: Commit**

```bash
cd ~/Dev/auric/auric-drift
git init && git add -A
git commit -m "feat: scaffold auric-drift crate with types and config"
```

---

### Task 2: Camelot wheel

**Files:**
- Create: `~/Dev/auric/auric-drift/src/camelot.rs`

- [ ] **Step 1: Write camelot tests**

Create `~/Dev/auric/auric-drift/tests/camelot_tests.rs`:

```rust
use auric_drift::CamelotWheel;

#[test]
fn same_key_returns_one() {
    assert_eq!(CamelotWheel::compatibility(0, 0), 1.0);
    assert_eq!(CamelotWheel::compatibility(15, 15), 1.0);
}

#[test]
fn relative_major_minor_is_high() {
    // C major (0) and A minor (21) share Camelot number 8
    let score = CamelotWheel::compatibility(0, 21);
    assert!(score >= 0.9, "relative major/minor should be >= 0.9, got {score}");
}

#[test]
fn adjacent_same_letter_is_high() {
    // C major (8B) and G major (9B) are adjacent
    let score = CamelotWheel::compatibility(0, 7);
    assert!(score >= 0.85, "adjacent keys should be >= 0.85, got {score}");
}

#[test]
fn distant_keys_are_low() {
    // C major (8B) and F# major (2B) are far apart
    let score = CamelotWheel::compatibility(0, 6);
    assert!(score <= 0.4, "distant keys should be <= 0.4, got {score}");
}

#[test]
fn out_of_range_returns_default() {
    assert_eq!(CamelotWheel::compatibility(25, 0), 0.5);
    assert_eq!(CamelotWheel::compatibility(-1, 0), 0.5);
}

#[test]
fn key_names_cover_all_24() {
    for i in 0..24 {
        let name = CamelotWheel::name(i);
        assert_ne!(name, "?", "key {i} should have a name");
    }
    assert_eq!(CamelotWheel::name(24), "?");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd ~/Dev/auric/auric-drift && cargo test --test camelot_tests 2>&1`
Expected: FAIL (module not found)

- [ ] **Step 3: Implement CamelotWheel**

Write `~/Dev/auric/auric-drift/src/camelot.rs`:

```rust
pub struct CamelotWheel;

struct CamelotPosition {
    number: i32,
    letter: u8, // b'A' or b'B'
}

const CAMELOT_MAP: [CamelotPosition; 24] = [
    CamelotPosition { number: 8, letter: b'B' },   // 0  C major
    CamelotPosition { number: 3, letter: b'B' },   // 1  C# major
    CamelotPosition { number: 10, letter: b'B' },  // 2  D major
    CamelotPosition { number: 5, letter: b'B' },   // 3  Eb major
    CamelotPosition { number: 12, letter: b'B' },  // 4  E major
    CamelotPosition { number: 7, letter: b'B' },   // 5  F major
    CamelotPosition { number: 2, letter: b'B' },   // 6  F# major
    CamelotPosition { number: 9, letter: b'B' },   // 7  G major
    CamelotPosition { number: 4, letter: b'B' },   // 8  Ab major
    CamelotPosition { number: 11, letter: b'B' },  // 9  A major
    CamelotPosition { number: 6, letter: b'B' },   // 10 Bb major
    CamelotPosition { number: 1, letter: b'B' },   // 11 B major
    CamelotPosition { number: 5, letter: b'A' },   // 12 C minor
    CamelotPosition { number: 12, letter: b'A' },  // 13 C# minor
    CamelotPosition { number: 7, letter: b'A' },   // 14 D minor
    CamelotPosition { number: 2, letter: b'A' },   // 15 Eb minor
    CamelotPosition { number: 9, letter: b'A' },   // 16 E minor
    CamelotPosition { number: 4, letter: b'A' },   // 17 F minor
    CamelotPosition { number: 11, letter: b'A' },  // 18 F# minor
    CamelotPosition { number: 6, letter: b'A' },   // 19 G minor
    CamelotPosition { number: 1, letter: b'A' },   // 20 Ab minor
    CamelotPosition { number: 8, letter: b'A' },   // 21 A minor
    CamelotPosition { number: 3, letter: b'A' },   // 22 Bb minor
    CamelotPosition { number: 10, letter: b'A' },  // 23 B minor
];

const KEY_NAMES: [&str; 24] = [
    "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
    "Cm", "C#m", "Dm", "Ebm", "Em", "Fm", "F#m", "Gm", "Abm", "Am", "Bbm", "Bm",
];

impl CamelotWheel {
    pub fn compatibility(from: i32, to: i32) -> f32 {
        if from < 0 || from >= 24 || to < 0 || to >= 24 {
            return 0.5;
        }

        if from == to {
            return 1.0;
        }

        let a = &CAMELOT_MAP[from as usize];
        let b = &CAMELOT_MAP[to as usize];

        if a.number == b.number && a.letter != b.letter {
            return 0.95;
        }

        let diff = (a.number - b.number).abs();
        let wrapped = diff.min(12 - diff);

        if a.letter == b.letter {
            match wrapped {
                1 => 0.9,
                2 => 0.7,
                3 => 0.5,
                4 => 0.35,
                5 => 0.25,
                _ => 0.15,
            }
        } else {
            match wrapped {
                0 => 0.95,
                1 => 0.75,
                2 => 0.55,
                _ => 0.3,
            }
        }
    }

    pub fn name(key: i32) -> &'static str {
        if key >= 0 && (key as usize) < KEY_NAMES.len() {
            KEY_NAMES[key as usize]
        } else {
            "?"
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cd ~/Dev/auric/auric-drift && cargo test --test camelot_tests 2>&1`
Expected: All pass

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat: implement CamelotWheel harmonic compatibility"
```

---

### Task 3: Genre compatibility matrix

**Files:**
- Create: `~/Dev/auric/auric-drift/src/genre.rs`

- [ ] **Step 1: Write genre tests**

Add to `~/Dev/auric/auric-drift/tests/engine_tests.rs` (create file):

```rust
use auric_drift::genre::GenreCompatibilityMatrix;

#[test]
fn same_genre_returns_one() {
    let matrix = GenreCompatibilityMatrix::new();
    assert_eq!(matrix.score("rock", "rock"), 1.0);
}

#[test]
fn intra_group_is_high() {
    let matrix = GenreCompatibilityMatrix::new();
    let score = matrix.score("rock", "indie rock");
    assert!(score >= 0.8, "intra-group should be >= 0.8, got {score}");
}

#[test]
fn cross_group_is_moderate() {
    let matrix = GenreCompatibilityMatrix::new();
    let score = matrix.score("rock", "metal");
    assert!(score >= 0.5 && score <= 0.8, "cross-group should be 0.5-0.8, got {score}");
}

#[test]
fn unknown_genres_get_low_default() {
    let matrix = GenreCompatibilityMatrix::new();
    let score = matrix.score("noise", "field recording");
    assert!(score <= 0.5, "unknown pair should be <= 0.5, got {score}");
}

#[test]
fn case_insensitive() {
    let matrix = GenreCompatibilityMatrix::new();
    assert_eq!(matrix.score("Rock", "ROCK"), 1.0);
    assert_eq!(matrix.score("Hip Hop", "hip hop"), 1.0);
}

#[test]
fn symmetric() {
    let matrix = GenreCompatibilityMatrix::new();
    assert_eq!(matrix.score("rock", "blues"), matrix.score("blues", "rock"));
}
```

- [ ] **Step 2: Implement GenreCompatibilityMatrix**

Write `~/Dev/auric/auric-drift/src/genre.rs`:

```rust
use std::collections::HashMap;

pub struct GenreCompatibilityMatrix {
    matrix: HashMap<String, HashMap<String, f64>>,
}

impl GenreCompatibilityMatrix {
    pub fn new() -> Self {
        let mut m: HashMap<String, HashMap<String, f64>> = HashMap::new();

        let groups: &[(&[&str], f64)] = &[
            (&["rock", "alternative", "indie", "indie rock", "punk", "post-punk",
              "grunge", "garage rock", "psychedelic rock", "shoegaze"], 0.85),
            (&["metal", "heavy metal", "death metal", "black metal", "thrash metal",
              "doom metal", "progressive metal", "metalcore", "nu metal"], 0.85),
            (&["electronic", "house", "techno", "trance", "drum and bass", "dubstep",
              "ambient", "idm", "electro", "synthwave", "edm"], 0.8),
            (&["hip hop", "hip-hop", "rap", "trap", "grime", "boom bap"], 0.85),
            (&["r&b", "rnb", "soul", "neo soul", "motown", "funk", "disco"], 0.85),
            (&["jazz", "bebop", "fusion", "smooth jazz", "free jazz", "swing",
              "bossa nova", "latin jazz"], 0.8),
            (&["classical", "baroque", "romantic", "contemporary classical",
              "orchestral", "chamber music", "opera"], 0.8),
            (&["country", "americana", "bluegrass", "folk", "singer-songwriter",
              "acoustic"], 0.8),
            (&["pop", "synth pop", "dance pop", "electropop", "art pop",
              "dream pop", "indie pop", "power pop"], 0.85),
            (&["blues", "delta blues", "chicago blues", "blues rock"], 0.85),
            (&["reggae", "dub", "ska", "dancehall"], 0.85),
            (&["world", "afrobeat", "latin", "salsa", "cumbia", "samba",
              "flamenco", "celtic"], 0.75),
        ];

        let cross: &[(&str, &str, f64)] = &[
            ("rock", "metal", 0.7), ("rock", "blues", 0.75), ("rock", "pop", 0.65),
            ("rock", "country", 0.5), ("metal", "punk", 0.6), ("electronic", "pop", 0.6),
            ("electronic", "hip hop", 0.55), ("electronic", "ambient", 0.8),
            ("hip hop", "r&b", 0.8), ("hip hop", "pop", 0.55), ("r&b", "pop", 0.7),
            ("r&b", "jazz", 0.65), ("jazz", "blues", 0.7), ("jazz", "classical", 0.5),
            ("jazz", "soul", 0.7), ("folk", "indie", 0.65), ("folk", "country", 0.75),
            ("blues", "soul", 0.75), ("blues", "country", 0.55),
            ("reggae", "hip hop", 0.5), ("reggae", "world", 0.6),
            ("classical", "ambient", 0.45), ("country", "pop", 0.5),
            ("singer-songwriter", "indie", 0.7), ("singer-songwriter", "pop", 0.6),
            ("dream pop", "shoegaze", 0.9), ("dream pop", "ambient", 0.7),
            ("synthwave", "synth pop", 0.85), ("post-punk", "synth pop", 0.65),
            ("funk", "disco", 0.8), ("funk", "hip hop", 0.65),
            ("disco", "house", 0.75), ("soul", "gospel", 0.7),
        ];

        for (genres, intra) in groups {
            for a in *genres {
                for b in *genres {
                    if a != b {
                        m.entry(a.to_string())
                            .or_default()
                            .insert(b.to_string(), *intra);
                    }
                }
            }
        }

        for (a, b, score) in cross {
            m.entry(a.to_string()).or_default().insert(b.to_string(), *score);
            m.entry(b.to_string()).or_default().insert(a.to_string(), *score);
        }

        Self { matrix: m }
    }

    pub fn score(&self, from: &str, to: &str) -> f64 {
        let a = from.to_lowercase();
        let b = to.to_lowercase();
        let a = a.trim();
        let b = b.trim();

        if a == b {
            return 1.0;
        }

        if let Some(direct) = self.matrix.get(a).and_then(|row| row.get(b)) {
            return *direct;
        }

        // Fuzzy token overlap fallback
        let a_tokens: Vec<&str> = a.split_whitespace().collect();
        let b_tokens: Vec<&str> = b.split_whitespace().collect();

        for (key, scores) in &self.matrix {
            let key_tokens: Vec<&str> = key.split_whitespace().collect();
            if a_tokens.iter().any(|t| key_tokens.contains(t)) {
                for (target, score) in scores {
                    let target_tokens: Vec<&str> = target.split_whitespace().collect();
                    if b_tokens.iter().any(|t| target_tokens.contains(t)) {
                        return score * 0.8;
                    }
                }
            }
        }

        0.4
    }
}

impl Default for GenreCompatibilityMatrix {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cd ~/Dev/auric/auric-drift && cargo test --test engine_tests 2>&1`
Expected: All pass

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat: implement genre compatibility matrix"
```

---

### Task 4: Drift engine (shuffle + scoring)

**Files:**
- Create: `~/Dev/auric/auric-drift/src/engine.rs`

- [ ] **Step 1: Write engine tests**

Append to `~/Dev/auric/auric-drift/tests/engine_tests.rs`:

```rust
use auric_drift::{DriftConfig, DriftEngine, DriftHistory, ShuffleMode, TrackSnapshot};

fn make_track(id: &str, artist: &str, album: &str, genre: &str) -> TrackSnapshot {
    TrackSnapshot {
        id: id.to_string(),
        artist: artist.to_string(),
        album: album.to_string(),
        genre: if genre.is_empty() { None } else { Some(genre.to_string()) },
        track_number: None,
        last_played_ms: None,
        play_count: 0,
        skip_count: 0,
        drift_indexed: false,
        drift_bpm: None,
        drift_key: None,
        drift_energy: None,
        drift_brightness: None,
    }
}

fn make_tracks(n: usize) -> Vec<TrackSnapshot> {
    (0..n)
        .map(|i| make_track(
            &format!("t{i}"),
            &format!("Artist {}", i % 5),
            &format!("Album {}", i % 10),
            &["rock", "jazz", "electronic", "pop", "blues"][i % 5],
        ))
        .collect()
}

#[test]
fn shuffle_preserves_all_tracks() {
    let engine = DriftEngine::new();
    let tracks = make_tracks(100);
    let result = engine.shuffle(&tracks, ShuffleMode::Smart, &DriftConfig::default());
    assert_eq!(result.len(), tracks.len());
    for t in &tracks {
        assert!(result.iter().any(|r| r.id == t.id), "missing track {}", t.id);
    }
}

#[test]
fn random_shuffle_preserves_all_tracks() {
    let engine = DriftEngine::new();
    let tracks = make_tracks(50);
    let result = engine.shuffle(&tracks, ShuffleMode::Random, &DriftConfig::default());
    assert_eq!(result.len(), 50);
}

#[test]
fn artist_shuffle_groups_by_artist() {
    let engine = DriftEngine::new();
    let tracks = make_tracks(20);
    let result = engine.shuffle(&tracks, ShuffleMode::Artist, &DriftConfig::default());
    assert_eq!(result.len(), 20);
}

#[test]
fn album_shuffle_preserves_track_order_within_album() {
    let engine = DriftEngine::new();
    let mut tracks = vec![
        make_track("a1", "X", "Album A", "rock"),
        make_track("a2", "X", "Album A", "rock"),
        make_track("b1", "Y", "Album B", "jazz"),
    ];
    tracks[0].track_number = Some(1);
    tracks[1].track_number = Some(2);
    tracks[2].track_number = Some(1);

    let result = engine.shuffle(&tracks, ShuffleMode::Album, &DriftConfig::default());
    assert_eq!(result.len(), 3);

    // Find the Album A tracks in result and verify order
    let album_a: Vec<&TrackSnapshot> = result.iter().filter(|t| t.album == "Album A").collect();
    assert_eq!(album_a.len(), 2);
    assert!(album_a[0].track_number <= album_a[1].track_number);
}

#[test]
fn single_track_returns_unchanged() {
    let engine = DriftEngine::new();
    let tracks = vec![make_track("only", "Solo", "Single", "pop")];
    let result = engine.shuffle(&tracks, ShuffleMode::Smart, &DriftConfig::default());
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, "only");
}

#[test]
fn empty_returns_empty() {
    let engine = DriftEngine::new();
    let result = engine.shuffle(&[], ShuffleMode::Smart, &DriftConfig::default());
    assert!(result.is_empty());
}

#[test]
fn next_track_avoids_same_artist_when_possible() {
    let engine = DriftEngine::new();
    let current = make_track("c", "Same Artist", "A1", "rock");
    let candidates = vec![
        make_track("s1", "Same Artist", "A2", "rock"),
        make_track("d1", "Different Artist", "B1", "rock"),
        make_track("d2", "Another Artist", "C1", "rock"),
    ];
    let mut history = DriftHistory::new();
    history.record(&current);

    // Run multiple times to check tendency (probabilistic)
    let mut different_count = 0;
    for _ in 0..50 {
        if let Some(next) = engine.next_track(&current, &candidates, &history, &DriftConfig::default()) {
            if next.artist != "Same Artist" {
                different_count += 1;
            }
        }
    }
    assert!(different_count > 30, "should prefer different artists, got {different_count}/50");
}

#[test]
fn large_collection_stays_within_time_budget() {
    let engine = DriftEngine::new();
    let tracks = make_tracks(5000);
    let start = std::time::Instant::now();
    let result = engine.shuffle(&tracks, ShuffleMode::Smart, &DriftConfig::default());
    let elapsed = start.elapsed();
    assert_eq!(result.len(), 5000);
    assert!(elapsed.as_secs() < 10, "5000-track shuffle took {elapsed:?}, should be < 10s");
}
```

- [ ] **Step 2: Implement DriftEngine**

Write `~/Dev/auric/auric-drift/src/engine.rs`:

```rust
use crate::camelot::CamelotWheel;
use crate::genre::GenreCompatibilityMatrix;
use crate::types::{DriftConfig, DriftHistory, ShuffleMode, TrackSnapshot};
use rand::Rng;

pub struct DriftEngine {
    genre_matrix: GenreCompatibilityMatrix,
}

impl DriftEngine {
    pub fn new() -> Self {
        Self {
            genre_matrix: GenreCompatibilityMatrix::new(),
        }
    }

    pub fn shuffle(
        &self,
        tracks: &[TrackSnapshot],
        mode: ShuffleMode,
        config: &DriftConfig,
    ) -> Vec<TrackSnapshot> {
        if tracks.len() <= 1 {
            return tracks.to_vec();
        }

        match mode {
            ShuffleMode::Smart => self.drift_shuffle(tracks, config),
            ShuffleMode::Random => fisher_yates(tracks),
            ShuffleMode::Artist => self.artist_grouped_shuffle(tracks),
            ShuffleMode::Album => self.album_grouped_shuffle(tracks),
            ShuffleMode::Genre => self.genre_grouped_shuffle(tracks),
        }
    }

    pub fn next_track(
        &self,
        current: &TrackSnapshot,
        candidates: &[TrackSnapshot],
        history: &DriftHistory,
        config: &DriftConfig,
    ) -> Option<TrackSnapshot> {
        if candidates.is_empty() {
            return None;
        }
        if candidates.len() == 1 {
            return Some(candidates[0].clone());
        }

        let scored: Vec<(TrackSnapshot, f64)> = candidates
            .iter()
            .map(|c| {
                let score = self.score_candidate(c, current, history, config);
                (c.clone(), score)
            })
            .collect();

        Some(weighted_select(&scored).clone())
    }

    // -- Drift shuffle (full algorithm) --

    const DRIFT_CAP: usize = 2000;

    fn drift_shuffle(&self, tracks: &[TrackSnapshot], config: &DriftConfig) -> Vec<TrackSnapshot> {
        let (drift_tracks, tail_tracks) = if tracks.len() > Self::DRIFT_CAP {
            let shuffled = fisher_yates(tracks);
            let (head, tail) = shuffled.split_at(Self::DRIFT_CAP);
            (head.to_vec(), fisher_yates(tail))
        } else {
            (tracks.to_vec(), Vec::new())
        };

        let mut remaining: Vec<(TrackSnapshot, f64)> = drift_tracks
            .into_iter()
            .map(|t| {
                let w = self.freshness_weight(&t, config);
                (t, w)
            })
            .collect();

        let mut result = Vec::with_capacity(tracks.len());
        let mut history = DriftHistory::new();

        while !remaining.is_empty() {
            if result.is_empty() {
                let scored: Vec<(TrackSnapshot, f64)> =
                    remaining.iter().map(|(t, s)| (t.clone(), *s)).collect();
                let selected = weighted_select(&scored);
                history.record(selected);
                result.push(selected.clone());
                remaining.retain(|(t, _)| t.id != selected.id);
                continue;
            }

            let current = result.last().unwrap();

            let scored: Vec<(TrackSnapshot, f64)> = remaining
                .iter()
                .map(|(t, base_weight)| {
                    let sep = self.separation_score(t, &history, config);
                    let genre = if config.genre_transition_smoothing {
                        self.genre_transition_score(current, t)
                    } else {
                        1.0
                    };
                    let audio = self.audio_flow_score(current, t, config);
                    (t.clone(), base_weight * sep * genre * audio)
                })
                .collect();

            let selected = weighted_select(&scored);
            history.record(selected);
            result.push(selected.clone());
            remaining.retain(|(t, _)| t.id != selected.id);
        }

        result.extend(tail_tracks);
        result
    }

    // -- Scoring --

    fn score_candidate(
        &self,
        candidate: &TrackSnapshot,
        current: &TrackSnapshot,
        history: &DriftHistory,
        config: &DriftConfig,
    ) -> f64 {
        let freshness = self.freshness_weight(candidate, config);
        let separation = self.separation_score(candidate, history, config);
        let genre = if config.genre_transition_smoothing {
            self.genre_transition_score(current, candidate)
        } else {
            1.0
        };
        let audio = self.audio_flow_score(current, candidate, config);
        freshness * separation * genre * audio
    }

    fn freshness_weight(&self, track: &TrackSnapshot, config: &DriftConfig) -> f64 {
        let mut weight = 1.0;

        if let Some(last_ms) = track.last_played_ms {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock is before UNIX_EPOCH")
                .as_millis() as i64;
            let hours_since = (now_ms - last_ms) as f64 / 3_600_000.0;
            let decay = 1.0 - (-hours_since / config.freshness_decay_hours).exp();
            weight *= decay.max(0.05);
        }

        if track.skip_count > 0 {
            let penalty = 1.0 / (1.0 + config.skip_penalty_weight * track.skip_count as f64);
            weight *= penalty;
        }

        if track.play_count == 0 {
            weight *= 1.0 + config.discovery_boost;
        }

        weight
    }

    fn separation_score(
        &self,
        track: &TrackSnapshot,
        history: &DriftHistory,
        config: &DriftConfig,
    ) -> f64 {
        let mut score = 1.0;

        if let Some(dist) = history.last_index_of_artist(&track.artist, config.artist_separation) {
            let penalty = (dist + 1) as f64 / config.artist_separation as f64;
            score *= penalty;
        }

        if let Some(dist) = history.last_index_of_album(&track.album, config.album_separation) {
            let penalty = (dist + 1) as f64 / config.album_separation as f64;
            score *= penalty;
        }

        if let Some(genre) = &track.genre {
            if let Some(dist) = history.last_index_of_genre(genre, config.genre_separation) {
                let penalty = (dist + 1) as f64 / config.genre_separation as f64;
                score *= penalty;
            }
        }

        score
    }

    fn genre_transition_score(&self, from: &TrackSnapshot, to: &TrackSnapshot) -> f64 {
        let from_genre = from.genre.as_deref().unwrap_or("");
        let to_genre = to.genre.as_deref().unwrap_or("");

        if from_genre.is_empty() || to_genre.is_empty() {
            return 0.8;
        }

        self.genre_matrix.score(from_genre, to_genre)
    }

    fn audio_flow_score(
        &self,
        from: &TrackSnapshot,
        to: &TrackSnapshot,
        config: &DriftConfig,
    ) -> f64 {
        if !from.drift_indexed || !to.drift_indexed {
            return 1.0;
        }

        let mut score = 1.0;

        if config.harmonic_mixing {
            if let (Some(fk), Some(tk)) = (from.drift_key, to.drift_key) {
                let compat = CamelotWheel::compatibility(fk, tk) as f64;
                score *= lerp(1.0, compat, config.harmonic_weight);
            }
        }

        if config.bpm_continuity {
            if let (Some(fb), Some(tb)) = (from.drift_bpm, to.drift_bpm) {
                if fb > 0.0 && tb > 0.0 {
                    let delta = (fb - tb).abs();
                    let half_delta = (fb - tb * 2.0).abs();
                    let double_delta = (fb * 2.0 - tb).abs();
                    let effective = delta.min(half_delta).min(double_delta);

                    if effective > config.max_bpm_delta {
                        let overshoot = (effective - config.max_bpm_delta) as f64;
                        score *= (1.0 - overshoot / 40.0).max(0.2);
                    }
                }
            }
        }

        if config.energy_smoothing {
            if let (Some(fe), Some(te)) = (from.drift_energy, to.drift_energy) {
                let delta = (fe - te).abs();
                if delta > config.max_energy_delta {
                    let overshoot = (delta - config.max_energy_delta) as f64;
                    score *= (1.0 - overshoot / 0.5).max(0.3);
                }
            }
        }

        if config.brightness_smoothing {
            if let (Some(fb), Some(tb)) = (from.drift_brightness, to.drift_brightness) {
                let delta = (fb - tb).abs();
                if delta > config.max_brightness_delta {
                    let overshoot = (delta - config.max_brightness_delta) as f64;
                    score *= (1.0 - overshoot / 0.5).max(0.4);
                }
            }
        }

        score
    }

    // -- Grouped shuffles --

    fn artist_grouped_shuffle(&self, tracks: &[TrackSnapshot]) -> Vec<TrackSnapshot> {
        let mut by_artist: std::collections::HashMap<String, Vec<TrackSnapshot>> =
            std::collections::HashMap::new();
        for t in tracks {
            by_artist.entry(t.artist.clone()).or_default().push(t.clone());
        }
        let mut groups: Vec<Vec<TrackSnapshot>> =
            by_artist.into_values().map(|g| fisher_yates(&g)).collect();
        groups = fisher_yates_generic(groups);
        groups.into_iter().flatten().collect()
    }

    fn album_grouped_shuffle(&self, tracks: &[TrackSnapshot]) -> Vec<TrackSnapshot> {
        let mut by_album: std::collections::HashMap<String, Vec<TrackSnapshot>> =
            std::collections::HashMap::new();
        for t in tracks {
            by_album.entry(t.album.clone()).or_default().push(t.clone());
        }
        let mut groups: Vec<Vec<TrackSnapshot>> = by_album
            .into_values()
            .map(|mut g| {
                g.sort_by_key(|t| t.track_number.unwrap_or(0));
                g
            })
            .collect();
        groups = fisher_yates_generic(groups);
        groups.into_iter().flatten().collect()
    }

    fn genre_grouped_shuffle(&self, tracks: &[TrackSnapshot]) -> Vec<TrackSnapshot> {
        let mut by_genre: std::collections::HashMap<String, Vec<TrackSnapshot>> =
            std::collections::HashMap::new();
        for t in tracks {
            let genre = t.genre.clone().unwrap_or_else(|| "Unknown".to_string());
            by_genre.entry(genre).or_default().push(t.clone());
        }

        let genre_names: Vec<String> = by_genre.keys().cloned().collect();
        let ordered = self.order_genres_by_compatibility(&genre_names);

        let mut result = Vec::with_capacity(tracks.len());
        for genre in ordered {
            if let Some(tracks) = by_genre.get(&genre) {
                result.extend(fisher_yates(tracks));
            }
        }
        result
    }

    fn order_genres_by_compatibility(&self, genres: &[String]) -> Vec<String> {
        if genres.len() <= 2 {
            return fisher_yates_generic(genres.to_vec());
        }

        let mut remaining: std::collections::HashSet<String> = genres.iter().cloned().collect();
        let mut ordered = Vec::with_capacity(genres.len());

        let mut rng = rand::rng();
        let start_idx = rng.random_range(0..genres.len());
        let start = genres[start_idx].clone();
        ordered.push(start.clone());
        remaining.remove(&start);

        while !remaining.is_empty() {
            let current = ordered.last().unwrap();
            let best = remaining
                .iter()
                .max_by(|a, b| {
                    self.genre_matrix
                        .score(current, a)
                        .partial_cmp(&self.genre_matrix.score(current, b))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .cloned();
            if let Some(best) = best {
                ordered.push(best.clone());
                remaining.remove(&best);
            } else {
                break;
            }
        }

        ordered
    }
}

impl Default for DriftEngine {
    fn default() -> Self {
        Self::new()
    }
}

// -- Utilities --

fn fisher_yates(tracks: &[TrackSnapshot]) -> Vec<TrackSnapshot> {
    fisher_yates_generic(tracks.to_vec())
}

fn fisher_yates_generic<T>(mut items: Vec<T>) -> Vec<T> {
    let mut rng = rand::rng();
    for i in (1..items.len()).rev() {
        let j = rng.random_range(0..=i);
        items.swap(i, j);
    }
    items
}

fn weighted_select<'a>(items: &'a [(TrackSnapshot, f64)]) -> &'a TrackSnapshot {
    let total: f64 = items.iter().map(|(_, s)| s).sum();
    if total <= 0.0 {
        return &items[rand::rng().random_range(0..items.len())].0;
    }

    let mut roll = rand::rng().random_range(0.0..total);
    for (track, score) in items {
        roll -= score;
        if roll <= 0.0 {
            return track;
        }
    }
    &items.last().unwrap().0
}

fn lerp(a: f64, b: f64, weight: f64) -> f64 {
    a + (b - a) * weight
}
```

- [ ] **Step 3: Run tests**

Run: `cd ~/Dev/auric/auric-drift && cargo test 2>&1`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat: implement DriftEngine with all shuffle modes and scoring"
```

---

### Task 5: Audio analyzer (parallel, cross-platform)

**Files:**
- Create: `~/Dev/auric/auric-drift/src/analyzer.rs`

- [ ] **Step 1: Write analyzer tests**

Create `~/Dev/auric/auric-drift/tests/analyzer_tests.rs`:

```rust
use auric_drift::analyzer::DriftAnalyzer;
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn rejects_nonexistent_file() {
    let analyzer = DriftAnalyzer::new();
    let result = analyzer.analyze_file(&PathBuf::from("/definitely/not/a/file.flac"));
    assert!(result.is_err());
}

#[test]
fn normalize_returns_clamped_values() {
    // Energy, brightness, dynamic_range should all be in 0.0..=1.0
    // BPM should be in 60..=200 range after normalization
    // Key should be 0..23
    // These are tested implicitly through real file analysis,
    // but we can test the clamping logic directly.
    assert_eq!(auric_drift::analyzer::clamp_normalize(50.0, 0.0, 100.0), 0.5);
    assert_eq!(auric_drift::analyzer::clamp_normalize(-10.0, 0.0, 100.0), 0.0);
    assert_eq!(auric_drift::analyzer::clamp_normalize(200.0, 0.0, 100.0), 1.0);
}

#[test]
fn batch_analyze_empty_returns_empty() {
    let analyzer = DriftAnalyzer::new();
    let results = analyzer.analyze_batch(&[], None);
    assert!(results.is_empty());
}
```

- [ ] **Step 2: Implement DriftAnalyzer**

Write `~/Dev/auric/auric-drift/src/analyzer.rs`:

```rust
use crate::types::{AnalysisProgress, AnalyzerError, DriftFeatures};
use rayon::prelude::*;
use rustfft::{num_complex::Complex, FftPlanner};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

const FFT_SIZE: usize = 4096;
const HOP_SIZE: usize = 2048;
const MAX_ANALYZE_SECONDS: usize = 90;

pub struct DriftAnalyzer {
    _private: (),
}

impl DriftAnalyzer {
    pub fn new() -> Self {
        Self { _private: () }
    }

    pub fn analyze_file(&self, path: &Path) -> Result<DriftFeatures, AnalyzerError> {
        let samples = load_mono_samples(path)?;
        if samples.is_empty() {
            return Err(AnalyzerError::EmptyAudio);
        }

        let sample_rate = detect_sample_rate(path)?;

        let energy = compute_energy(&samples);
        let dynamic_range = compute_dynamic_range(&samples);
        let brightness = compute_spectral_centroid(&samples, sample_rate);
        let bpm = detect_bpm(&samples, sample_rate);
        let key = detect_key(&samples, sample_rate);

        Ok(DriftFeatures {
            bpm,
            key,
            energy,
            brightness,
            dynamic_range,
        })
    }

    pub fn analyze_batch(
        &self,
        paths: &[&Path],
        progress_callback: Option<Arc<dyn Fn(AnalysisProgress) + Send + Sync>>,
    ) -> Vec<(usize, Result<DriftFeatures, AnalyzerError>)> {
        if paths.is_empty() {
            return Vec::new();
        }

        let completed = Arc::new(AtomicUsize::new(0));
        let total = paths.len();

        paths
            .par_iter()
            .enumerate()
            .map(|(idx, path)| {
                let result = self.analyze_file(path);

                let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(cb) = &progress_callback {
                    cb(AnalysisProgress {
                        completed: done,
                        total,
                    });
                }

                (idx, result)
            })
            .collect()
    }
}

impl Default for DriftAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

// -- Audio loading via Symphonia --

fn detect_sample_rate(path: &Path) -> Result<f32, AnalyzerError> {
    let file = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| AnalyzerError::UnsupportedFormat(e.to_string()))?;

    let track = probed
        .format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| AnalyzerError::UnsupportedFormat("no audio track found".to_string()))?;

    track
        .codec_params
        .sample_rate
        .map(|sr| sr as f32)
        .ok_or_else(|| AnalyzerError::Decode("no sample rate in codec params".to_string()))
}

fn load_mono_samples(path: &Path) -> Result<Vec<f32>, AnalyzerError> {
    let file = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| AnalyzerError::UnsupportedFormat(e.to_string()))?;

    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| AnalyzerError::UnsupportedFormat("no audio track".to_string()))?;

    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
    let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2);
    let max_samples = sample_rate as usize * MAX_ANALYZE_SECONDS;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| AnalyzerError::Decode(e.to_string()))?;

    let mut mono_samples = Vec::with_capacity(max_samples);

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(_) => break,
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let spec = *decoded.spec();
        let n_frames = decoded.frames();
        let mut sample_buf = SampleBuffer::<f32>::new(n_frames as u64, spec);
        sample_buf.copy_interleaved_ref(decoded);
        let interleaved = sample_buf.samples();

        let ch = channels.max(1);
        for frame_idx in 0..n_frames {
            let offset = frame_idx * ch;
            if offset >= interleaved.len() {
                break;
            }
            let mut sum = 0.0f32;
            for c in 0..ch.min(interleaved.len() - offset) {
                sum += interleaved[offset + c];
            }
            mono_samples.push(sum / ch as f32);
        }

        if mono_samples.len() >= max_samples {
            mono_samples.truncate(max_samples);
            break;
        }
    }

    Ok(mono_samples)
}

// -- DSP functions --

fn compute_energy(samples: &[f32]) -> f32 {
    let sum_sq: f64 = samples.iter().map(|s| (*s as f64) * (*s as f64)).sum();
    let rms = (sum_sq / samples.len().max(1) as f64).sqrt() as f32;
    let db = 20.0 * rms.max(1e-10).log10();
    clamp_normalize(db, -60.0, 0.0)
}

fn compute_dynamic_range(samples: &[f32]) -> f32 {
    let segment_size = (samples.len() / 100).max(1024);
    let mut segment_rms: Vec<f32> = Vec::new();

    let mut offset = 0;
    while offset + segment_size <= samples.len() {
        let segment = &samples[offset..offset + segment_size];
        let sum_sq: f64 = segment.iter().map(|s| (*s as f64) * (*s as f64)).sum();
        let rms = (sum_sq / segment_size as f64).sqrt() as f32;
        if rms > 1e-8 {
            segment_rms.push(rms);
        }
        offset += segment_size;
    }

    if segment_rms.len() < 5 {
        return 0.5;
    }

    segment_rms.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let low = segment_rms[segment_rms.len() / 10];
    let high = segment_rms[segment_rms.len() * 9 / 10];

    if low <= 1e-10 {
        return 1.0;
    }

    let range_db = 20.0 * (high / low).log10();
    clamp_normalize(range_db, 0.0, 40.0)
}

fn compute_spectral_centroid(samples: &[f32], sample_rate: f32) -> f32 {
    let half_fft = FFT_SIZE / 2;
    let bin_width = sample_rate / FFT_SIZE as f32;
    let window = hann_window(FFT_SIZE);

    let mut centroid_sum = 0.0f64;
    let mut frame_count = 0u64;

    let mut offset = 0;
    while offset + FFT_SIZE <= samples.len() {
        let magnitudes = compute_fft_magnitudes(&samples[offset..offset + FFT_SIZE], &window);

        let mut weighted_sum = 0.0f64;
        let mut mag_sum = 0.0f64;
        for bin in 1..half_fft {
            let freq = bin as f64 * bin_width as f64;
            weighted_sum += freq * magnitudes[bin] as f64;
            mag_sum += magnitudes[bin] as f64;
        }

        if mag_sum > 1e-10 {
            centroid_sum += weighted_sum / mag_sum;
            frame_count += 1;
        }

        offset += HOP_SIZE * 4;
    }

    if frame_count == 0 {
        return 0.5;
    }

    let avg = (centroid_sum / frame_count as f64) as f32;
    let nyquist = sample_rate / 2.0;
    clamp_normalize(avg, 200.0, nyquist * 0.6)
}

fn detect_bpm(samples: &[f32], sample_rate: f32) -> f32 {
    let half_fft = FFT_SIZE / 2;
    let window = hann_window(FFT_SIZE);

    let mut onset_strength: Vec<f32> = Vec::new();
    let mut prev_magnitudes = vec![0.0f32; half_fft];

    let mut offset = 0;
    while offset + FFT_SIZE <= samples.len() {
        let mut magnitudes = compute_fft_magnitudes(&samples[offset..offset + FFT_SIZE], &window);
        // Use sqrt magnitudes for onset detection
        for m in &mut magnitudes {
            *m = m.sqrt();
        }

        let mut flux = 0.0f32;
        for bin in 0..half_fft {
            let diff = magnitudes[bin] - prev_magnitudes[bin];
            if diff > 0.0 {
                flux += diff;
            }
        }
        onset_strength.push(flux);
        prev_magnitudes = magnitudes;

        offset += HOP_SIZE;
    }

    if onset_strength.len() < 16 {
        return 120.0;
    }

    let onset_rate = sample_rate / HOP_SIZE as f32;
    let min_bpm: f32 = 60.0;
    let max_bpm: f32 = 200.0;
    let min_lag = (onset_rate * 60.0 / max_bpm) as usize;
    let max_lag = ((onset_rate * 60.0 / min_bpm) as usize).min(onset_strength.len() / 2);

    if min_lag >= max_lag {
        return 120.0;
    }

    let mut best_lag = min_lag;
    let mut best_corr = f64::NEG_INFINITY;

    for lag in min_lag..=max_lag {
        let n = onset_strength.len() - lag;
        let corr: f64 = (0..n)
            .map(|i| onset_strength[i] as f64 * onset_strength[i + lag] as f64)
            .sum::<f64>()
            / n as f64;

        if corr > best_corr {
            best_corr = corr;
            best_lag = lag;
        }
    }

    let bpm = onset_rate * 60.0 / best_lag as f32;

    if bpm > 160.0 {
        bpm / 2.0
    } else if bpm < 70.0 {
        bpm * 2.0
    } else {
        bpm
    }
}

fn detect_key(samples: &[f32], sample_rate: f32) -> i32 {
    let half_fft = FFT_SIZE / 2;
    let bin_width = sample_rate / FFT_SIZE as f32;
    let window = hann_window(FFT_SIZE);

    let mut chromagram = [0.0f64; 12];
    let mut frame_count = 0u64;

    let mut offset = 0;
    while offset + FFT_SIZE <= samples.len() {
        let magnitudes = compute_fft_magnitudes(&samples[offset..offset + FFT_SIZE], &window);

        for bin in 1..half_fft {
            let freq = bin as f32 * bin_width;
            if freq < 65.0 || freq > 2000.0 {
                continue;
            }

            let note_num = 12.0 * (freq / 440.0).log2() + 69.0;
            let pitch_class = (note_num.round() as i32).rem_euclid(12) as usize;
            chromagram[pitch_class] += magnitudes[bin] as f64;
        }
        frame_count += 1;

        offset += HOP_SIZE * 4;
    }

    if frame_count == 0 {
        return 0;
    }

    for c in &mut chromagram {
        *c /= frame_count as f64;
    }

    match_key_profile(&chromagram)
}

fn match_key_profile(chromagram: &[f64; 12]) -> i32 {
    // Krumhansl-Kessler key profiles
    let major: [f64; 12] = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
    let minor: [f64; 12] = [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];

    let mut best_key = 0i32;
    let mut best_corr = f64::NEG_INFINITY;

    for root in 0..12 {
        let mut rotated = [0.0f64; 12];
        for i in 0..12 {
            rotated[i] = chromagram[(i + root) % 12];
        }

        let major_corr = pearson(&rotated, &major);
        let minor_corr = pearson(&rotated, &minor);

        if major_corr > best_corr {
            best_corr = major_corr;
            best_key = root as i32;
        }
        if minor_corr > best_corr {
            best_corr = minor_corr;
            best_key = root as i32 + 12;
        }
    }

    best_key
}

fn pearson(x: &[f64; 12], y: &[f64; 12]) -> f64 {
    let n = 12.0;
    let sum_x: f64 = x.iter().sum();
    let sum_y: f64 = y.iter().sum();
    let sum_xy: f64 = x.iter().zip(y).map(|(a, b)| a * b).sum();
    let sum_x2: f64 = x.iter().map(|a| a * a).sum();
    let sum_y2: f64 = y.iter().map(|a| a * a).sum();

    let num = n * sum_xy - sum_x * sum_y;
    let den = ((n * sum_x2 - sum_x * sum_x) * (n * sum_y2 - sum_y * sum_y)).sqrt();
    if den < 1e-10 {
        return 0.0;
    }
    num / den
}

// -- FFT --

fn compute_fft_magnitudes(frame: &[f32], window: &[f32]) -> Vec<f32> {
    let n = frame.len();
    let half = n / 2;

    let mut windowed: Vec<Complex<f32>> = frame
        .iter()
        .zip(window)
        .map(|(s, w)| Complex::new(s * w, 0.0))
        .collect();

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(n);
    fft.process(&mut windowed);

    windowed[..half]
        .iter()
        .map(|c| c.norm_sqr())
        .collect()
}

fn hann_window(size: usize) -> Vec<f32> {
    (0..size)
        .map(|i| {
            let t = std::f32::consts::PI * 2.0 * i as f32 / size as f32;
            0.5 * (1.0 - t.cos())
        })
        .collect()
}

// -- Utilities --

pub fn clamp_normalize(value: f32, min: f32, max: f32) -> f32 {
    ((value - min) / (max - min)).clamp(0.0, 1.0)
}
```

- [ ] **Step 3: Run tests**

Run: `cd ~/Dev/auric/auric-drift && cargo test 2>&1`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat: implement DriftAnalyzer with parallel batch analysis via rayon"
```

---

### Task 6: C FFI layer for Swift consumption

**Files:**
- Create: `~/Dev/auric/auric-drift/src/ffi.rs`
- Create: `~/Dev/auric/auric-drift/cbindgen.toml`

- [ ] **Step 1: Write the FFI module**

Write `~/Dev/auric/auric-drift/src/ffi.rs`:

```rust
//! C-compatible FFI for consuming auric-drift from Swift/Objective-C.
//!
//! Build with: cargo build --release --features ffi
//! Generate header with: cbindgen --config cbindgen.toml --crate auric-drift -o include/auric_drift.h

use crate::types::{DriftConfig, DriftFeatures, ShuffleMode, TrackSnapshot};
use crate::engine::DriftEngine;
use crate::analyzer::DriftAnalyzer;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;

#[repr(C)]
pub struct CDriftFeatures {
    pub bpm: f32,
    pub key: i32,
    pub energy: f32,
    pub brightness: f32,
    pub dynamic_range: f32,
    pub error: *mut c_char,
}

#[repr(C)]
pub struct CDriftConfig {
    pub artist_separation: u32,
    pub album_separation: u32,
    pub genre_separation: u32,
    pub freshness_decay_hours: f64,
    pub skip_penalty_weight: f64,
    pub discovery_boost: f64,
    pub genre_transition_smoothing: bool,
    pub harmonic_mixing: bool,
    pub harmonic_weight: f64,
    pub bpm_continuity: bool,
    pub max_bpm_delta: f32,
    pub energy_smoothing: bool,
    pub max_energy_delta: f32,
    pub brightness_smoothing: bool,
    pub max_brightness_delta: f32,
}

impl From<CDriftConfig> for DriftConfig {
    fn from(c: CDriftConfig) -> Self {
        Self {
            artist_separation: c.artist_separation as usize,
            album_separation: c.album_separation as usize,
            genre_separation: c.genre_separation as usize,
            freshness_decay_hours: c.freshness_decay_hours,
            skip_penalty_weight: c.skip_penalty_weight,
            discovery_boost: c.discovery_boost,
            genre_transition_smoothing: c.genre_transition_smoothing,
            harmonic_mixing: c.harmonic_mixing,
            harmonic_weight: c.harmonic_weight,
            bpm_continuity: c.bpm_continuity,
            max_bpm_delta: c.max_bpm_delta,
            energy_smoothing: c.energy_smoothing,
            max_energy_delta: c.max_energy_delta,
            brightness_smoothing: c.brightness_smoothing,
            max_brightness_delta: c.max_brightness_delta,
        }
    }
}

/// Analyze a single audio file. Caller must free the error string with `auric_drift_free_string`.
#[no_mangle]
pub extern "C" fn auric_drift_analyze_file(path: *const c_char) -> CDriftFeatures {
    let path = match unsafe { CStr::from_ptr(path) }.to_str() {
        Ok(s) => s,
        Err(_) => {
            return CDriftFeatures {
                bpm: 0.0, key: 0, energy: 0.0, brightness: 0.0, dynamic_range: 0.0,
                error: CString::new("invalid UTF-8 path").unwrap().into_raw(),
            };
        }
    };

    let analyzer = DriftAnalyzer::new();
    match analyzer.analyze_file(std::path::Path::new(path)) {
        Ok(f) => CDriftFeatures {
            bpm: f.bpm, key: f.key, energy: f.energy,
            brightness: f.brightness, dynamic_range: f.dynamic_range,
            error: ptr::null_mut(),
        },
        Err(e) => CDriftFeatures {
            bpm: 0.0, key: 0, energy: 0.0, brightness: 0.0, dynamic_range: 0.0,
            error: CString::new(e.to_string()).unwrap().into_raw(),
        },
    }
}

/// Return the default DriftConfig.
#[no_mangle]
pub extern "C" fn auric_drift_default_config() -> CDriftConfig {
    let d = DriftConfig::default();
    CDriftConfig {
        artist_separation: d.artist_separation as u32,
        album_separation: d.album_separation as u32,
        genre_separation: d.genre_separation as u32,
        freshness_decay_hours: d.freshness_decay_hours,
        skip_penalty_weight: d.skip_penalty_weight,
        discovery_boost: d.discovery_boost,
        genre_transition_smoothing: d.genre_transition_smoothing,
        harmonic_mixing: d.harmonic_mixing,
        harmonic_weight: d.harmonic_weight,
        bpm_continuity: d.bpm_continuity,
        max_bpm_delta: d.max_bpm_delta,
        energy_smoothing: d.energy_smoothing,
        max_energy_delta: d.max_energy_delta,
        brightness_smoothing: d.brightness_smoothing,
        max_brightness_delta: d.max_brightness_delta,
    }
}

/// Free a string allocated by the FFI layer.
#[no_mangle]
pub extern "C" fn auric_drift_free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe { drop(CString::from_raw(s)) };
    }
}
```

- [ ] **Step 2: Write cbindgen.toml**

Write `~/Dev/auric/auric-drift/cbindgen.toml`:

```toml
language = "C"
include_guard = "AURIC_DRIFT_H"
autogen_warning = "/* This file is auto-generated by cbindgen. Do not edit. */"

[export]
include = ["CDriftFeatures", "CDriftConfig"]

[fn]
prefix = "AURIC_DRIFT_EXPORT"
```

- [ ] **Step 3: Build with FFI feature**

Run: `cd ~/Dev/auric/auric-drift && cargo build --features ffi 2>&1`
Expected: Clean build

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat: add C FFI layer for Swift consumption"
```

---

### Task 7: Wire into auric-tui as a Cargo dependency

**Files:**
- Modify: `~/Dev/auric-tui-v2/Cargo.toml`
- Modify: `~/Dev/auric-tui-v2/crates/auric-app/Cargo.toml`

- [ ] **Step 1: Add auric-drift to workspace dependencies**

Add to `~/Dev/auric-tui-v2/Cargo.toml` under `[workspace.dependencies]`:

```toml
auric-drift = { path = "../auric/auric-drift", features = ["serde"] }
```

- [ ] **Step 2: Add to auric-app**

Add to `~/Dev/auric-tui-v2/crates/auric-app/Cargo.toml` under `[dependencies]`:

```toml
auric-drift.workspace = true
```

- [ ] **Step 3: Verify it builds**

Run: `cd ~/Dev/auric-tui-v2 && cargo build 2>&1`
Expected: Clean build

- [ ] **Step 4: Commit**

```bash
cd ~/Dev/auric-tui-v2
git add Cargo.toml Cargo.lock crates/auric-app/Cargo.toml
git commit -m "feat: add auric-drift as workspace dependency"
```

---

## Task Dependency Graph

```
Task 1 (scaffold) ─── prerequisite for all
  ├── Task 2 (camelot) ─── independent
  ├── Task 3 (genre matrix) ─── independent
  │
  ├── Task 4 (engine) ─── depends on Tasks 2, 3
  ├── Task 5 (analyzer) ─── independent
  │
  ├── Task 6 (FFI) ─── depends on Tasks 4, 5
  └── Task 7 (TUI wiring) ─── depends on Task 4
```

Tasks 2, 3, 5 can run in parallel after Task 1. Task 4 needs 2 and 3. Tasks 6 and 7 are independent of each other but need 4 and 5.
