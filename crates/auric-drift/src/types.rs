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
