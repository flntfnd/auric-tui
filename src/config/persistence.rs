use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::library::{LoadedFolder, Playlist, Track};
use crate::library::track::AudioFormat;
use super::theme::ThemePreset;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortMode {
    #[default]
    ArtistAlbum,
    Album,
    Title,
    DateAdded,
    Duration,
    Path,
}

impl SortMode {
    pub fn next(self) -> Self {
        match self {
            Self::ArtistAlbum => Self::Album,
            Self::Album => Self::Title,
            Self::Title => Self::DateAdded,
            Self::DateAdded => Self::Duration,
            Self::Duration => Self::Path,
            Self::Path => Self::ArtistAlbum,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::ArtistAlbum => "Artist/Album",
            Self::Album => "Album",
            Self::Title => "Title",
            Self::DateAdded => "Date Added",
            Self::Duration => "Duration",
            Self::Path => "Path",
        }
    }

    fn to_db(&self) -> i32 {
        match self {
            Self::ArtistAlbum => 0,
            Self::Album => 1,
            Self::Title => 2,
            Self::DateAdded => 3,
            Self::Duration => 4,
            Self::Path => 5,
        }
    }

    fn from_db(val: i32) -> Self {
        match val {
            0 => Self::ArtistAlbum,
            1 => Self::Album,
            2 => Self::Title,
            3 => Self::DateAdded,
            4 => Self::Duration,
            5 => Self::Path,
            _ => Self::ArtistAlbum,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Off => "─",
            Self::All => "↻",
            Self::One => "↺",
        }
    }

    fn to_db(&self) -> i32 {
        match self {
            Self::Off => 0,
            Self::All => 1,
            Self::One => 2,
        }
    }

    fn from_db(val: i32) -> Self {
        match val {
            0 => Self::Off,
            1 => Self::All,
            2 => Self::One,
            _ => Self::Off,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub sort_mode: SortMode,
    pub theme: ThemePreset,
    pub spectrum_enabled: bool,
    pub show_album_art: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            volume: 0.8,
            shuffle: false,
            repeat: RepeatMode::Off,
            sort_mode: SortMode::ArtistAlbum,
            theme: ThemePreset::Default,
            spectrum_enabled: true,
            show_album_art: true,
        }
    }
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new() -> Result<Self> {
        let db_path = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?
            .join("auric-tui");

        std::fs::create_dir_all(&db_path)?;
        let db_file = db_path.join("library.db");

        let conn = Connection::open(&db_file)?;
        let db = Self { conn };
        db.init_schema()?;
        db.run_migrations()?;
        Ok(db)
    }

    fn run_migrations(&self) -> Result<()> {
        // Migration: Add is_watched column if it doesn't exist
        let has_is_watched: bool = self.conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('folders') WHERE name='is_watched'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count > 0)
            .unwrap_or(false);

        if !has_is_watched {
            self.conn.execute(
                "ALTER TABLE folders ADD COLUMN is_watched INTEGER DEFAULT 0",
                [],
            )?;
        }

        Ok(())
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            -- Config table (key-value store)
            CREATE TABLE IF NOT EXISTS config (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            -- Folders table
            CREATE TABLE IF NOT EXISTS folders (
                id INTEGER PRIMARY KEY,
                path TEXT UNIQUE NOT NULL,
                name TEXT NOT NULL,
                track_count INTEGER DEFAULT 0,
                added_at TEXT NOT NULL,
                is_watched INTEGER DEFAULT 0
            );

            -- Tracks table
            CREATE TABLE IF NOT EXISTS tracks (
                id TEXT PRIMARY KEY,
                path TEXT UNIQUE NOT NULL,
                title TEXT NOT NULL,
                artist TEXT NOT NULL,
                album TEXT NOT NULL,
                album_artist TEXT,
                track_number INTEGER,
                disc_number INTEGER,
                duration_ms INTEGER NOT NULL,
                format TEXT NOT NULL,
                added_at TEXT NOT NULL,
                folder_id INTEGER,
                FOREIGN KEY (folder_id) REFERENCES folders(id) ON DELETE CASCADE
            );

            -- Playlists table
            CREATE TABLE IF NOT EXISTS playlists (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                created_at TEXT NOT NULL,
                modified_at TEXT NOT NULL
            );

            -- Playlist tracks (many-to-many)
            CREATE TABLE IF NOT EXISTS playlist_tracks (
                playlist_id TEXT NOT NULL,
                track_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                PRIMARY KEY (playlist_id, track_id),
                FOREIGN KEY (playlist_id) REFERENCES playlists(id) ON DELETE CASCADE,
                FOREIGN KEY (track_id) REFERENCES tracks(id) ON DELETE CASCADE
            );

            -- Session state
            CREATE TABLE IF NOT EXISTS session (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                current_track_id TEXT,
                position_ms INTEGER DEFAULT 0
            );

            -- Queue table
            CREATE TABLE IF NOT EXISTS queue (
                position INTEGER PRIMARY KEY,
                track_id TEXT NOT NULL,
                FOREIGN KEY (track_id) REFERENCES tracks(id) ON DELETE CASCADE
            );

            -- Indexes for fast queries
            CREATE INDEX IF NOT EXISTS idx_tracks_artist ON tracks(artist);
            CREATE INDEX IF NOT EXISTS idx_tracks_album ON tracks(album);
            CREATE INDEX IF NOT EXISTS idx_tracks_title ON tracks(title);
            CREATE INDEX IF NOT EXISTS idx_tracks_added ON tracks(added_at);
            CREATE INDEX IF NOT EXISTS idx_tracks_folder ON tracks(folder_id);

            -- Initialize session row if not exists
            INSERT OR IGNORE INTO session (id) VALUES (1);
            "
        )?;
        Ok(())
    }

    // ========== Config Operations ==========

    pub fn load_config(&self) -> Result<AppConfig> {
        let mut config = AppConfig::default();

        if let Some(v) = self.get_config_value("volume")? {
            config.volume = v.parse().unwrap_or(0.8);
        }
        if let Some(v) = self.get_config_value("shuffle")? {
            config.shuffle = v == "1";
        }
        if let Some(v) = self.get_config_value("repeat")? {
            config.repeat = RepeatMode::from_db(v.parse().unwrap_or(0));
        }
        if let Some(v) = self.get_config_value("sort_mode")? {
            config.sort_mode = SortMode::from_db(v.parse().unwrap_or(0));
        }
        if let Some(v) = self.get_config_value("theme")? {
            config.theme = match v.as_str() {
                "dracula" => ThemePreset::Dracula,
                "gruvbox" => ThemePreset::Gruvbox,
                _ => ThemePreset::Default,
            };
        }
        if let Some(v) = self.get_config_value("spectrum_enabled")? {
            config.spectrum_enabled = v != "0";
        }
        if let Some(v) = self.get_config_value("show_album_art")? {
            config.show_album_art = v != "0";
        }

        Ok(config)
    }

    pub fn save_config(&self, config: &AppConfig) -> Result<()> {
        self.set_config_value("volume", &config.volume.to_string())?;
        self.set_config_value("shuffle", if config.shuffle { "1" } else { "0" })?;
        self.set_config_value("repeat", &config.repeat.to_db().to_string())?;
        self.set_config_value("sort_mode", &config.sort_mode.to_db().to_string())?;
        self.set_config_value("theme", match config.theme {
            ThemePreset::Default => "default",
            ThemePreset::Dracula => "dracula",
            ThemePreset::Gruvbox => "gruvbox",
        })?;
        self.set_config_value("spectrum_enabled", if config.spectrum_enabled { "1" } else { "0" })?;
        self.set_config_value("show_album_art", if config.show_album_art { "1" } else { "0" })?;
        Ok(())
    }

    fn get_config_value(&self, key: &str) -> Result<Option<String>> {
        let result = self.conn
            .query_row(
                "SELECT value FROM config WHERE key = ?",
                [key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(result)
    }

    fn set_config_value(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO config (key, value) VALUES (?, ?)",
            params![key, value],
        )?;
        Ok(())
    }

    // ========== Folder Operations ==========

    pub fn load_folders(&self) -> Result<Vec<LoadedFolder>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, name, track_count, added_at, is_watched FROM folders ORDER BY is_watched DESC, name"
        )?;

        let folders = stmt.query_map([], |row| {
            let path: String = row.get(0)?;
            let name: String = row.get(1)?;
            let track_count: usize = row.get::<_, i64>(2)? as usize;
            let added_at: String = row.get(3)?;
            let is_watched: bool = row.get::<_, i64>(4)? != 0;

            Ok(LoadedFolder {
                path: PathBuf::from(path),
                name,
                track_count,
                added_at: DateTime::parse_from_rfc3339(&added_at)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                is_watched,
            })
        })?;

        folders.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn add_folder(&self, folder: &LoadedFolder) -> Result<i64> {
        self.conn.execute(
            "INSERT OR IGNORE INTO folders (path, name, track_count, added_at, is_watched) VALUES (?, ?, ?, ?, ?)",
            params![
                folder.path.to_string_lossy(),
                folder.name,
                folder.track_count as i64,
                folder.added_at.to_rfc3339(),
                if folder.is_watched { 1 } else { 0 },
            ],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    pub fn set_folder_watched(&self, path: &PathBuf, is_watched: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE folders SET is_watched = ? WHERE path = ?",
            params![if is_watched { 1 } else { 0 }, path.to_string_lossy()],
        )?;
        Ok(())
    }

    pub fn update_folder_track_count(&self, path: &PathBuf, count: usize) -> Result<()> {
        self.conn.execute(
            "UPDATE folders SET track_count = ? WHERE path = ?",
            params![count as i64, path.to_string_lossy()],
        )?;
        Ok(())
    }

    pub fn remove_folder(&self, path: &PathBuf) -> Result<()> {
        // First delete tracks from this folder
        self.conn.execute(
            "DELETE FROM tracks WHERE folder_id = (SELECT id FROM folders WHERE path = ?)",
            params![path.to_string_lossy()],
        )?;

        // Then delete the folder
        self.conn.execute(
            "DELETE FROM folders WHERE path = ?",
            params![path.to_string_lossy()],
        )?;

        Ok(())
    }

    fn get_folder_id(&self, path: &PathBuf) -> Result<Option<i64>> {
        let result = self.conn
            .query_row(
                "SELECT id FROM folders WHERE path = ?",
                params![path.to_string_lossy()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(result)
    }

    // ========== Track Operations ==========

    pub fn add_track(&self, track: &Track, folder_path: &PathBuf) -> Result<()> {
        let folder_id = self.get_folder_id(folder_path)?;

        self.conn.execute(
            "INSERT OR REPLACE INTO tracks
             (id, path, title, artist, album, album_artist, track_number, disc_number,
              duration_ms, format, added_at, folder_id)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                track.id.to_string(),
                track.path.to_string_lossy(),
                track.title,
                track.artist,
                track.album,
                track.album_artist,
                track.track_number.map(|n| n as i64),
                track.disc_number.map(|n| n as i64),
                track.duration.as_millis() as i64,
                track.format.extension(),
                track.added_at.to_rfc3339(),
                folder_id,
            ],
        )?;
        Ok(())
    }

    pub fn load_all_tracks(&self) -> Result<Vec<Track>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, title, artist, album, album_artist, track_number,
                    disc_number, duration_ms, format, added_at
             FROM tracks"
        )?;

        let tracks = stmt.query_map([], |row| self.row_to_track(row))?;
        tracks.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Remove a track by its file path and return the track ID if found
    pub fn remove_track_by_path(&self, path: &PathBuf) -> Result<Option<Uuid>> {
        // First get the track ID
        let id: Option<String> = self.conn
            .query_row(
                "SELECT id FROM tracks WHERE path = ?",
                params![path.to_string_lossy()],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(id_str) = id {
            // Delete the track
            self.conn.execute(
                "DELETE FROM tracks WHERE path = ?",
                params![path.to_string_lossy()],
            )?;
            return Ok(Uuid::parse_str(&id_str).ok());
        }

        Ok(None)
    }

    fn row_to_track(&self, row: &rusqlite::Row) -> rusqlite::Result<Track> {
        let id_str: String = row.get(0)?;
        let path_str: String = row.get(1)?;
        let title: String = row.get(2)?;
        let artist: String = row.get(3)?;
        let album: String = row.get(4)?;
        let album_artist: Option<String> = row.get(5)?;
        let track_number: Option<i64> = row.get(6)?;
        let disc_number: Option<i64> = row.get(7)?;
        let duration_ms: i64 = row.get(8)?;
        let format_str: String = row.get(9)?;
        let added_at_str: String = row.get(10)?;

        Ok(Track {
            id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4()),
            path: PathBuf::from(path_str),
            title,
            artist,
            album,
            album_artist,
            track_number: track_number.map(|n| n as u32),
            disc_number: disc_number.map(|n| n as u32),
            duration: Duration::from_millis(duration_ms as u64),
            format: AudioFormat::from_extension(&format_str),
            added_at: DateTime::parse_from_rfc3339(&added_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            album_art: None,      // Album art loaded on demand
            album_art_data: None,
        })
    }

    // ========== Playlist Operations ==========

    pub fn load_playlists(&self) -> Result<Vec<Playlist>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, created_at, modified_at FROM playlists ORDER BY name"
        )?;

        let playlists = stmt.query_map([], |row| {
            let id_str: String = row.get(0)?;
            let name: String = row.get(1)?;
            let created_at_str: String = row.get(2)?;
            let modified_at_str: String = row.get(3)?;

            Ok((id_str, name, created_at_str, modified_at_str))
        })?;

        let mut result = Vec::new();
        for playlist_result in playlists {
            let (id_str, name, created_at_str, modified_at_str) = playlist_result?;
            let id = Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4());

            // Load track IDs for this playlist
            let track_ids = self.load_playlist_tracks(&id)?;

            result.push(Playlist {
                id,
                name,
                track_ids,
                created_at: DateTime::parse_from_rfc3339(&created_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                modified_at: DateTime::parse_from_rfc3339(&modified_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                file_path: None,
            });
        }

        Ok(result)
    }

    fn load_playlist_tracks(&self, playlist_id: &Uuid) -> Result<Vec<Uuid>> {
        let mut stmt = self.conn.prepare(
            "SELECT track_id FROM playlist_tracks WHERE playlist_id = ? ORDER BY position"
        )?;

        let ids = stmt.query_map(params![playlist_id.to_string()], |row| {
            let id_str: String = row.get(0)?;
            Ok(Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4()))
        })?;

        ids.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn save_playlist(&self, playlist: &Playlist) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO playlists (id, name, created_at, modified_at)
             VALUES (?, ?, ?, ?)",
            params![
                playlist.id.to_string(),
                playlist.name,
                playlist.created_at.to_rfc3339(),
                playlist.modified_at.to_rfc3339(),
            ],
        )?;

        // Clear existing tracks
        self.conn.execute(
            "DELETE FROM playlist_tracks WHERE playlist_id = ?",
            params![playlist.id.to_string()],
        )?;

        // Insert tracks with positions
        for (pos, track_id) in playlist.track_ids.iter().enumerate() {
            self.conn.execute(
                "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?, ?, ?)",
                params![playlist.id.to_string(), track_id.to_string(), pos as i64],
            )?;
        }

        Ok(())
    }

    pub fn delete_playlist(&self, id: &Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM playlist_tracks WHERE playlist_id = ?",
            params![id.to_string()],
        )?;
        self.conn.execute(
            "DELETE FROM playlists WHERE id = ?",
            params![id.to_string()],
        )?;
        Ok(())
    }

    // ========== Session Operations ==========

    pub fn load_session(&self) -> Result<(Option<Uuid>, Vec<Uuid>)> {
        let current_track: Option<String> = self.conn
            .query_row(
                "SELECT current_track_id FROM session WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()?
            .flatten();

        let current_track_id = current_track
            .and_then(|s| Uuid::parse_str(&s).ok());

        // Load queue
        let mut stmt = self.conn.prepare(
            "SELECT track_id FROM queue ORDER BY position"
        )?;

        let queue = stmt.query_map([], |row| {
            let id_str: String = row.get(0)?;
            Ok(Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4()))
        })?;

        let queue_vec = queue.collect::<Result<Vec<_>, _>>()?;

        Ok((current_track_id, queue_vec))
    }

    pub fn save_session(&self, current_track_id: Option<Uuid>, queue: &[Uuid]) -> Result<()> {
        self.conn.execute(
            "UPDATE session SET current_track_id = ? WHERE id = 1",
            params![current_track_id.map(|id| id.to_string())],
        )?;

        // Clear and rebuild queue
        self.conn.execute("DELETE FROM queue", [])?;

        for (pos, track_id) in queue.iter().enumerate() {
            self.conn.execute(
                "INSERT INTO queue (position, track_id) VALUES (?, ?)",
                params![pos as i64, track_id.to_string()],
            )?;
        }

        Ok(())
    }
}
