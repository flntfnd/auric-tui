use std::path::PathBuf;

use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Playlist {
    pub id: Uuid,
    pub name: String,
    pub track_ids: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    #[allow(dead_code)]
    pub file_path: Option<PathBuf>,
}

impl Playlist {
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            track_ids: Vec::new(),
            created_at: now,
            modified_at: now,
            file_path: None,
        }
    }

    pub fn add_track(&mut self, track_id: Uuid) {
        if !self.track_ids.contains(&track_id) {
            self.track_ids.push(track_id);
            self.modified_at = Utc::now();
        }
    }

    pub fn len(&self) -> usize {
        self.track_ids.len()
    }
}

#[derive(Debug, Clone)]
pub struct LoadedFolder {
    pub path: PathBuf,
    pub name: String,
    pub track_count: usize,
    pub added_at: DateTime<Utc>,
    pub is_watched: bool,
}

impl LoadedFolder {
    pub fn new(path: PathBuf) -> Self {
        Self::with_watched(path, false)
    }

    pub fn new_watched(path: PathBuf) -> Self {
        Self::with_watched(path, true)
    }

    fn with_watched(path: PathBuf, is_watched: bool) -> Self {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();

        Self {
            path,
            name,
            track_count: 0,
            added_at: Utc::now(),
            is_watched,
        }
    }
}
