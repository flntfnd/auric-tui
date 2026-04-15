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
