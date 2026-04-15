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
