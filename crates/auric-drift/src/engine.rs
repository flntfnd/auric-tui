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

        let scores: Vec<f64> = candidates
            .iter()
            .map(|c| self.score_candidate(c, current, history, config))
            .collect();

        let idx = weighted_select_idx(&scores);
        Some(candidates[idx].clone())
    }

    const DRIFT_CAP: usize = 2000;

    // Per-step candidate window: score at most this many tracks per iteration.
    // This keeps the inner loop O(SCORE_WINDOW * DRIFT_CAP) instead of
    // O(DRIFT_CAP^2), while preserving quality via weighted sampling.
    const SCORE_WINDOW: usize = 100;

    fn drift_shuffle(&self, tracks: &[TrackSnapshot], config: &DriftConfig) -> Vec<TrackSnapshot> {
        let (drift_tracks, tail_tracks) = if tracks.len() > Self::DRIFT_CAP {
            let shuffled = fisher_yates(tracks);
            let (head, tail) = shuffled.split_at(Self::DRIFT_CAP);
            (head.to_vec(), fisher_yates(tail))
        } else {
            (tracks.to_vec(), Vec::new())
        };

        let n = drift_tracks.len();
        // Precompute freshness weights once. active[i] is a pool index.
        let tracks_pool: Vec<TrackSnapshot> = drift_tracks;
        let base_weights: Vec<f64> = tracks_pool
            .iter()
            .map(|t| self.freshness_weight(t, config))
            .collect();
        // active[i] = pool index for the i-th still-available slot
        let mut active: Vec<usize> = (0..n).collect();

        let mut result: Vec<TrackSnapshot> = Vec::with_capacity(tracks.len());
        let mut history = DriftHistory::new();
        let mut rng = rand::rng();

        while !active.is_empty() {
            // Draw a candidate window via partial Fisher-Yates over active indices.
            // For small remaining pools, evaluate everything; otherwise sample.
            let window_size = Self::SCORE_WINDOW.min(active.len());
            for i in 0..window_size {
                let j = rng.random_range(i..active.len());
                active.swap(i, j);
            }

            let scores: Vec<f64> = if result.is_empty() {
                active[..window_size]
                    .iter()
                    .map(|&pi| base_weights[pi].max(1e-9))
                    .collect()
            } else {
                let current = result.last().unwrap();
                active[..window_size]
                    .iter()
                    .map(|&pi| {
                        let t = &tracks_pool[pi];
                        let sep = self.separation_score(t, &history, config);
                        let genre = if config.genre_transition_smoothing {
                            self.genre_transition_score(current, t)
                        } else {
                            1.0
                        };
                        let audio = self.audio_flow_score(current, t, config);
                        (base_weights[pi] * sep * genre * audio).max(1e-9)
                    })
                    .collect()
            };

            // Select within the window; slot is an index into active[0..window_size]
            let slot = weighted_select_idx(&scores);
            let pool_idx = active[slot];

            // Swap-remove the chosen slot from the active list
            let last = active.len() - 1;
            active.swap(slot, last);
            active.pop();

            history.record(&tracks_pool[pool_idx]);
            result.push(tracks_pool[pool_idx].clone());
        }

        result.extend(tail_tracks);
        result
    }

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
        (freshness * separation * genre * audio).max(1e-9)
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
            if let Some(group) = by_genre.get(&genre) {
                result.extend(fisher_yates(group));
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

/// Returns the index into `scores` selected by weighted random sampling.
fn weighted_select_idx(scores: &[f64]) -> usize {
    let total: f64 = scores.iter().sum();
    if total <= 0.0 {
        return rand::rng().random_range(0..scores.len());
    }
    let mut roll = rand::rng().random_range(0.0..total);
    for (i, &s) in scores.iter().enumerate() {
        roll -= s;
        if roll <= 0.0 {
            return i;
        }
    }
    scores.len() - 1
}

fn lerp(a: f64, b: f64, weight: f64) -> f64 {
    a + (b - a) * weight
}
