use crate::{LibraryRoot, TrackRecord};
use auric_core::TrackId;
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const SCHEMA_VERSION: i64 = 2;

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS library_roots (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    watched INTEGER NOT NULL CHECK (watched IN (0, 1)),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS tracks (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    title TEXT,
    artist TEXT,
    album TEXT,
    duration_ms INTEGER,
    sample_rate INTEGER,
    channels INTEGER,
    bit_depth INTEGER,
    file_mtime_ms INTEGER,
    added_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tracks_artist_album ON tracks(artist, album);
CREATE INDEX IF NOT EXISTS idx_tracks_album_title ON tracks(album, title);

CREATE TABLE IF NOT EXISTS playlists (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_playlists_name_nocase ON playlists(name COLLATE NOCASE);

CREATE TABLE IF NOT EXISTS playlist_entries (
    playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    added_at_ms INTEGER NOT NULL,
    PRIMARY KEY (playlist_id, position)
);

CREATE INDEX IF NOT EXISTS idx_playlist_entries_track_id ON playlist_entries(track_id);

CREATE TABLE IF NOT EXISTS artwork_assets (
    id TEXT PRIMARY KEY,
    sha256_hex TEXT NOT NULL UNIQUE,
    source_kind TEXT NOT NULL,
    mime_type TEXT,
    picture_type TEXT,
    byte_len INTEGER NOT NULL,
    data BLOB NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_artwork_assets_source_kind ON artwork_assets(source_kind);

CREATE TABLE IF NOT EXISTS track_artwork (
    track_id TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    artwork_id TEXT NOT NULL REFERENCES artwork_assets(id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    extracted_at_ms INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_track_artwork_artwork_id ON track_artwork(artwork_id);
"#;

const MIGRATION_V1_TO_V2_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS artwork_assets (
    id TEXT PRIMARY KEY,
    sha256_hex TEXT NOT NULL UNIQUE,
    source_kind TEXT NOT NULL,
    mime_type TEXT,
    picture_type TEXT,
    byte_len INTEGER NOT NULL,
    data BLOB NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_artwork_assets_source_kind ON artwork_assets(source_kind);

CREATE TABLE IF NOT EXISTS track_artwork (
    track_id TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    artwork_id TEXT NOT NULL REFERENCES artwork_assets(id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    extracted_at_ms INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_track_artwork_artwork_id ON track_artwork(artwork_id);
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalMode {
    Wal,
    Delete,
    Memory,
}

impl JournalMode {
    fn as_sql(self) -> &'static str {
        match self {
            Self::Wal => "WAL",
            Self::Delete => "DELETE",
            Self::Memory => "MEMORY",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SynchronousMode {
    Off,
    Normal,
    Full,
}

impl SynchronousMode {
    fn as_sql(self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::Normal => "NORMAL",
            Self::Full => "FULL",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DatabaseOptions {
    pub path: PathBuf,
    pub journal_mode: JournalMode,
    pub synchronous: SynchronousMode,
    pub busy_timeout_ms: u64,
    pub cache_size_kib: i64,
    pub mmap_size_bytes: u64,
    pub wal_autocheckpoint_pages: u32,
}

impl Default for DatabaseOptions {
    fn default() -> Self {
        Self {
            path: PathBuf::from("var/auric.db"),
            journal_mode: JournalMode::Wal,
            synchronous: SynchronousMode::Normal,
            busy_timeout_ms: 5_000,
            cache_size_kib: 8 * 1024,
            mmap_size_bytes: 64 * 1024 * 1024,
            wal_autocheckpoint_pages: 1000,
        }
    }
}

#[derive(Debug)]
pub struct Database {
    conn: Connection,
    path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryRootRow {
    pub id: String,
    pub path: String,
    pub watched: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistRow {
    pub id: String,
    pub name: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistTrackRow {
    pub playlist_id: String,
    pub position: i64,
    pub added_at_ms: i64,
    pub track: TrackRow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackRow {
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
    pub added_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseStats {
    pub settings_count: i64,
    pub library_root_count: i64,
    pub track_count: i64,
    pub artwork_asset_count: i64,
    pub track_artwork_count: i64,
    pub playlist_count: i64,
    pub playlist_entry_count: i64,
    pub page_count: i64,
    pub page_size: i64,
    pub db_size_bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtworkAssetRow {
    pub id: String,
    pub sha256_hex: String,
    pub source_kind: String,
    pub mime_type: Option<String>,
    pub picture_type: Option<String>,
    pub byte_len: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackArtworkRow {
    pub track_id: TrackId,
    pub track_path: String,
    pub artwork_id: String,
    pub source: String,
    pub mime_type: Option<String>,
    pub picture_type: Option<String>,
    pub sha256_hex: String,
    pub byte_len: i64,
    pub extracted_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackArtworkUpsert {
    pub track_path: String,
    pub source_kind: String,
    pub source: String,
    pub mime_type: Option<String>,
    pub picture_type: Option<String>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtworkBatchUpsertSummary {
    pub input_items: usize,
    pub linked_tracks: usize,
    pub inserted_assets: usize,
    pub reused_assets: usize,
    pub skipped_missing_tracks: usize,
    pub bytes_stored: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PragmaSnapshot {
    pub journal_mode: String,
    pub synchronous: i64,
    pub foreign_keys: bool,
    pub cache_size: i64,
    pub mmap_size: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("uuid parse error: {0}")]
    Uuid(#[from] uuid::Error),
    #[error("unsupported schema version {found}; max supported {max_supported}")]
    UnsupportedSchemaVersion { found: i64, max_supported: i64 },
    #[error("not found: {0}")]
    NotFound(String),
    #[error("integrity check failed: {0}")]
    IntegrityCheck(String),
}

impl Database {
    pub fn open(options: &DatabaseOptions) -> Result<Self, DbError> {
        if let Some(parent) = options.path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        let conn = Connection::open(&options.path)?;
        Self::from_connection(conn, options, Some(options.path.clone()))
    }

    pub fn open_in_memory_for_tests() -> Result<Self, DbError> {
        let options = DatabaseOptions {
            journal_mode: JournalMode::Memory,
            synchronous: SynchronousMode::Off,
            path: PathBuf::from(":memory:"),
            ..DatabaseOptions::default()
        };
        let conn = Connection::open_in_memory()?;
        Self::from_connection(conn, &options, None)
    }

    fn from_connection(
        conn: Connection,
        options: &DatabaseOptions,
        path: Option<PathBuf>,
    ) -> Result<Self, DbError> {
        let mut db = Self { conn, path };
        db.configure_connection(options)?;
        db.migrate()?;
        Ok(db)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn configure_connection(&mut self, options: &DatabaseOptions) -> Result<(), DbError> {
        self.conn
            .busy_timeout(Duration::from_millis(options.busy_timeout_ms))?;
        self.conn.execute_batch(&format!(
            "PRAGMA foreign_keys = ON;\nPRAGMA journal_mode = {};\nPRAGMA synchronous = {};\nPRAGMA temp_store = MEMORY;\nPRAGMA cache_size = -{};\nPRAGMA mmap_size = {};\nPRAGMA wal_autocheckpoint = {};\n",
            options.journal_mode.as_sql(),
            options.synchronous.as_sql(),
            options.cache_size_kib,
            options.mmap_size_bytes,
            options.wal_autocheckpoint_pages
        ))?;
        Ok(())
    }

    fn migrate(&mut self) -> Result<(), DbError> {
        let current = self.schema_version()?;
        if current > SCHEMA_VERSION {
            return Err(DbError::UnsupportedSchemaVersion {
                found: current,
                max_supported: SCHEMA_VERSION,
            });
        }

        if current == 0 {
            let tx = self
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(SCHEMA_SQL)?;
            tx.execute_batch(&format!("PRAGMA user_version = {};", SCHEMA_VERSION))?;
            tx.commit()?;
        } else if current == 1 {
            let tx = self
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(MIGRATION_V1_TO_V2_SQL)?;
            tx.execute_batch(&format!("PRAGMA user_version = {};", SCHEMA_VERSION))?;
            tx.commit()?;
        }

        Ok(())
    }

    pub fn schema_version(&self) -> Result<i64, DbError> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version;", [], |row| row.get(0))?)
    }

    pub fn quick_check(&self) -> Result<(), DbError> {
        let result: String = self
            .conn
            .query_row("PRAGMA quick_check;", [], |row| row.get(0))?;
        if result.eq_ignore_ascii_case("ok") {
            Ok(())
        } else {
            Err(DbError::IntegrityCheck(result))
        }
    }

    pub fn optimize(&self) -> Result<(), DbError> {
        self.conn.execute_batch("PRAGMA optimize;")?;
        Ok(())
    }

    pub fn pragma_snapshot(&self) -> Result<PragmaSnapshot, DbError> {
        let journal_mode = self
            .conn
            .query_row("PRAGMA journal_mode;", [], |row| row.get::<_, String>(0))?;
        let synchronous = self
            .conn
            .query_row("PRAGMA synchronous;", [], |row| row.get::<_, i64>(0))?;
        let foreign_keys = self
            .conn
            .query_row("PRAGMA foreign_keys;", [], |row| row.get::<_, i64>(0))?
            != 0;
        let cache_size = self
            .conn
            .query_row("PRAGMA cache_size;", [], |row| row.get::<_, i64>(0))?;
        let mmap_size = self
            .conn
            .query_row("PRAGMA mmap_size;", [], |row| row.get::<_, i64>(0))
            .optional()?
            .unwrap_or(0);

        Ok(PragmaSnapshot {
            journal_mode,
            synchronous,
            foreign_keys,
            cache_size,
            mmap_size,
        })
    }

    pub fn set_setting_json(&self, key: &str, value: &JsonValue) -> Result<(), DbError> {
        let now = now_ms();
        let value_json = serde_json::to_string(value)?;
        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO app_settings (key, value_json, updated_at_ms)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at_ms = excluded.updated_at_ms",
        )?;
        stmt.execute(params![key, value_json, now])?;
        Ok(())
    }

    pub fn get_setting_json(&self, key: &str) -> Result<Option<JsonValue>, DbError> {
        let value_json: Option<String> = self
            .conn
            .query_row(
                "SELECT value_json FROM app_settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;

        match value_json {
            Some(v) => Ok(Some(serde_json::from_str(&v)?)),
            None => Ok(None),
        }
    }

    pub fn upsert_library_root(&self, root: &LibraryRoot) -> Result<LibraryRootRow, DbError> {
        let now = now_ms();
        let id = Uuid::new_v4().to_string();
        let watched = bool_to_i64(root.watched);
        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO library_roots (id, path, watched, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(path) DO UPDATE SET watched = excluded.watched, updated_at_ms = excluded.updated_at_ms",
        )?;
        stmt.execute(params![id, root.path, watched, now])?;
        self.get_library_root_by_path(&root.path)?
            .ok_or_else(|| DbError::NotFound(format!("library root {}", root.path)))
    }

    pub fn get_library_root_by_path(&self, path: &str) -> Result<Option<LibraryRootRow>, DbError> {
        self.conn
            .query_row(
                "SELECT id, path, watched, created_at_ms, updated_at_ms FROM library_roots WHERE path = ?1",
                params![path],
                read_library_root,
            )
            .optional()
            .map_err(DbError::from)
    }

    pub fn list_library_roots(&self) -> Result<Vec<LibraryRootRow>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, watched, created_at_ms, updated_at_ms FROM library_roots ORDER BY path ASC",
        )?;
        let rows = stmt.query_map([], read_library_root)?;
        collect_rows(rows)
    }

    pub fn create_playlist(&self, name: &str) -> Result<String, DbError> {
        let now = now_ms();
        let id = Uuid::new_v4().to_string();
        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO playlists (id, name, created_at_ms, updated_at_ms) VALUES (?1, ?2, ?3, ?3)",
        )?;
        stmt.execute(params![id, name, now])?;
        Ok(id)
    }

    pub fn list_playlists(&self) -> Result<Vec<PlaylistRow>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, created_at_ms, updated_at_ms FROM playlists ORDER BY lower(name), name",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(PlaylistRow {
                id: row.get(0)?,
                name: row.get(1)?,
                created_at_ms: row.get(2)?,
                updated_at_ms: row.get(3)?,
            })
        })?;
        collect_rows(rows)
    }

    pub fn rename_playlist(&self, playlist_id: &str, name: &str) -> Result<(), DbError> {
        let changed = self.conn.execute(
            "UPDATE playlists SET name = ?2, updated_at_ms = ?3 WHERE id = ?1",
            params![playlist_id, name, now_ms()],
        )?;
        if changed == 0 {
            return Err(DbError::NotFound(format!("playlist {playlist_id}")));
        }
        Ok(())
    }

    pub fn delete_playlist(&self, playlist_id: &str) -> Result<(), DbError> {
        let changed = self
            .conn
            .execute("DELETE FROM playlists WHERE id = ?1", params![playlist_id])?;
        if changed == 0 {
            return Err(DbError::NotFound(format!("playlist {playlist_id}")));
        }
        Ok(())
    }

    pub fn upsert_track(&self, track: &TrackRecord) -> Result<(), DbError> {
        let now = now_ms();
        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO tracks (
                id, path, title, artist, album,
                duration_ms, sample_rate, channels, bit_depth, file_mtime_ms,
                added_at_ms, updated_at_ms
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9, ?10,
                ?11, ?11
             )
             ON CONFLICT(path) DO UPDATE SET
                title = excluded.title,
                artist = excluded.artist,
                album = excluded.album,
                duration_ms = excluded.duration_ms,
                sample_rate = excluded.sample_rate,
                channels = excluded.channels,
                bit_depth = excluded.bit_depth,
                file_mtime_ms = excluded.file_mtime_ms,
                updated_at_ms = excluded.updated_at_ms",
        )?;
        stmt.execute(params![
            track.id.0.to_string(),
            track.path,
            track.title,
            track.artist,
            track.album,
            track.duration_ms,
            track.sample_rate,
            track.channels,
            track.bit_depth,
            track.file_mtime_ms,
            now
        ])?;
        Ok(())
    }

    pub fn upsert_tracks_batch(&mut self, tracks: &[TrackRecord]) -> Result<usize, DbError> {
        if tracks.is_empty() {
            return Ok(0);
        }

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = now_ms();
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO tracks (
                    id, path, title, artist, album,
                    duration_ms, sample_rate, channels, bit_depth, file_mtime_ms,
                    added_at_ms, updated_at_ms
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5,
                    ?6, ?7, ?8, ?9, ?10,
                    ?11, ?11
                 )
                 ON CONFLICT(path) DO UPDATE SET
                    title = excluded.title,
                    artist = excluded.artist,
                    album = excluded.album,
                    duration_ms = excluded.duration_ms,
                    sample_rate = excluded.sample_rate,
                    channels = excluded.channels,
                    bit_depth = excluded.bit_depth,
                    file_mtime_ms = excluded.file_mtime_ms,
                    updated_at_ms = excluded.updated_at_ms",
            )?;

            for track in tracks {
                stmt.execute(params![
                    track.id.0.to_string(),
                    track.path,
                    track.title,
                    track.artist,
                    track.album,
                    track.duration_ms,
                    track.sample_rate,
                    track.channels,
                    track.bit_depth,
                    track.file_mtime_ms,
                    now
                ])?;
            }
        }
        tx.commit()?;
        Ok(tracks.len())
    }

    pub fn count_tracks(&self) -> Result<i64, DbError> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM tracks", [], |row| row.get(0))?)
    }

    pub fn count_artwork_assets(&self) -> Result<i64, DbError> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM artwork_assets", [], |row| row.get(0))?)
    }

    pub fn count_track_artwork_links(&self) -> Result<i64, DbError> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM track_artwork", [], |row| row.get(0))?)
    }

    pub fn artwork_total_bytes(&self) -> Result<i64, DbError> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(byte_len), 0) FROM artwork_assets",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn list_tracks(&self, limit: usize) -> Result<Vec<TrackRow>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, title, artist, album, duration_ms, sample_rate, channels, bit_depth, file_mtime_ms, added_at_ms, updated_at_ms
             FROM tracks ORDER BY path ASC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], read_track_row)?;
        collect_rows(rows)
    }

    pub fn distinct_artists(&self) -> Result<Vec<String>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT artist FROM tracks WHERE artist IS NOT NULL AND artist != '' ORDER BY artist COLLATE NOCASE ASC",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn distinct_albums(&self) -> Result<Vec<(String, String)>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT album, COALESCE(artist, '') FROM tracks WHERE album IS NOT NULL AND album != '' ORDER BY album COLLATE NOCASE ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn distinct_genres(&self) -> Result<Vec<String>, DbError> {
        Ok(Vec::new())
    }

    pub fn list_tracks_by_artist(&self, artist: &str) -> Result<Vec<TrackRow>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, title, artist, album, duration_ms, sample_rate, channels, bit_depth, file_mtime_ms, added_at_ms, updated_at_ms
             FROM tracks WHERE artist = ?1 ORDER BY album COLLATE NOCASE ASC, path ASC",
        )?;
        let rows = stmt.query_map(params![artist], read_track_row)?;
        collect_rows(rows)
    }

    pub fn list_tracks_by_album(&self, album: &str) -> Result<Vec<TrackRow>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, title, artist, album, duration_ms, sample_rate, channels, bit_depth, file_mtime_ms, added_at_ms, updated_at_ms
             FROM tracks WHERE album = ?1 ORDER BY path ASC",
        )?;
        let rows = stmt.query_map(params![album], read_track_row)?;
        collect_rows(rows)
    }

    pub fn get_artwork_data_for_track(&self, track_path: &str) -> Result<Option<Vec<u8>>, DbError> {
        self.conn
            .query_row(
                "SELECT aa.data
                 FROM tracks t
                 JOIN track_artwork ta ON ta.track_id = t.id
                 JOIN artwork_assets aa ON aa.id = ta.artwork_id
                 WHERE t.path = ?1
                 LIMIT 1",
                params![track_path],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(DbError::from)
    }

    pub fn get_track_by_id(&self, track_id: TrackId) -> Result<Option<TrackRow>, DbError> {
        self.conn
            .query_row(
                "SELECT id, path, title, artist, album, duration_ms, sample_rate, channels, bit_depth, file_mtime_ms, added_at_ms, updated_at_ms
                 FROM tracks WHERE id = ?1 LIMIT 1",
                params![track_id.0.to_string()],
                read_track_row,
            )
            .optional()
            .map_err(DbError::from)
    }

    pub fn get_track_by_path(&self, path: &str) -> Result<Option<TrackRow>, DbError> {
        self.conn
            .query_row(
                "SELECT id, path, title, artist, album, duration_ms, sample_rate, channels, bit_depth, file_mtime_ms, added_at_ms, updated_at_ms
                 FROM tracks WHERE path = ?1 LIMIT 1",
                params![path],
                read_track_row,
            )
            .optional()
            .map_err(DbError::from)
    }

    pub fn list_tracks_by_prefix(
        &self,
        path_prefix: &str,
        limit: usize,
    ) -> Result<Vec<TrackRow>, DbError> {
        let escaped = escape_sql_like(path_prefix);
        let slash_pattern = format!("{escaped}/%");
        let backslash_pattern = format!("{escaped}\\\\%");
        let mut stmt = self.conn.prepare(
            "SELECT id, path, title, artist, album, duration_ms, sample_rate, channels, bit_depth, file_mtime_ms, added_at_ms, updated_at_ms
             FROM tracks
             WHERE path = ?1
                OR path LIKE ?2 ESCAPE '\\'
                OR path LIKE ?3 ESCAPE '\\'
             ORDER BY path ASC LIMIT ?4",
        )?;
        let rows = stmt.query_map(
            params![path_prefix, slash_pattern, backslash_pattern, limit as i64],
            read_track_row,
        )?;
        collect_rows(rows)
    }

    pub fn list_track_paths_under_prefix(&self, root_path: &str) -> Result<Vec<String>, DbError> {
        let escaped = escape_sql_like(root_path);
        let slash_pattern = format!("{escaped}/%");
        let backslash_pattern = format!("{escaped}\\\\%");

        let mut stmt = self.conn.prepare(
            "SELECT path FROM tracks
             WHERE path = ?1
                OR path LIKE ?2 ESCAPE '\\'
                OR path LIKE ?3 ESCAPE '\\'
             ORDER BY path ASC",
        )?;
        let rows = stmt.query_map(
            params![root_path, slash_pattern, backslash_pattern],
            |row| row.get::<_, String>(0),
        )?;
        collect_rows(rows)
    }

    pub fn delete_tracks_by_paths(&mut self, paths: &[String]) -> Result<usize, DbError> {
        if paths.is_empty() {
            return Ok(0);
        }

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut deleted = 0usize;
        {
            let mut stmt = tx.prepare_cached("DELETE FROM tracks WHERE path = ?1")?;
            for path in paths {
                deleted += stmt.execute(params![path])?;
            }
        }
        tx.commit()?;
        Ok(deleted)
    }

    pub fn upsert_track_artwork_batch(
        &mut self,
        items: &[TrackArtworkUpsert],
    ) -> Result<ArtworkBatchUpsertSummary, DbError> {
        if items.is_empty() {
            return Ok(ArtworkBatchUpsertSummary {
                input_items: 0,
                linked_tracks: 0,
                inserted_assets: 0,
                reused_assets: 0,
                skipped_missing_tracks: 0,
                bytes_stored: 0,
            });
        }

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = now_ms();
        let mut linked_tracks = 0usize;
        let mut inserted_assets = 0usize;
        let mut reused_assets = 0usize;
        let mut skipped_missing_tracks = 0usize;
        let mut bytes_stored = 0usize;

        {
            let mut track_id_stmt =
                tx.prepare_cached("SELECT id FROM tracks WHERE path = ?1 LIMIT 1")?;
            let mut select_asset_stmt =
                tx.prepare_cached("SELECT id FROM artwork_assets WHERE sha256_hex = ?1 LIMIT 1")?;
            let mut insert_asset_stmt = tx.prepare_cached(
                "INSERT INTO artwork_assets (
                    id, sha256_hex, source_kind, mime_type, picture_type, byte_len, data, created_at_ms, updated_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            )?;
            let mut upsert_link_stmt = tx.prepare_cached(
                "INSERT INTO track_artwork (track_id, artwork_id, source, extracted_at_ms)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(track_id) DO UPDATE SET
                    artwork_id = excluded.artwork_id,
                    source = excluded.source,
                    extracted_at_ms = excluded.extracted_at_ms",
            )?;

            for item in items {
                let track_id: Option<String> = track_id_stmt
                    .query_row(params![item.track_path.as_str()], |row| row.get(0))
                    .optional()?;
                let Some(track_id) = track_id else {
                    skipped_missing_tracks += 1;
                    continue;
                };

                let sha256_hex = sha256_hex(&item.bytes);
                let artwork_id = if let Some(existing_id) = select_asset_stmt
                    .query_row(params![sha256_hex], |row| row.get::<_, String>(0))
                    .optional()?
                {
                    reused_assets += 1;
                    existing_id
                } else {
                    let artwork_id = Uuid::new_v4().to_string();
                    insert_asset_stmt.execute(params![
                        &artwork_id,
                        &sha256_hex,
                        item.source_kind.as_str(),
                        item.mime_type.as_deref(),
                        item.picture_type.as_deref(),
                        i64::try_from(item.bytes.len()).unwrap_or(i64::MAX),
                        item.bytes.as_slice(),
                        now
                    ])?;
                    inserted_assets += 1;
                    bytes_stored = bytes_stored.saturating_add(item.bytes.len());
                    artwork_id
                };

                upsert_link_stmt.execute(params![
                    track_id,
                    artwork_id,
                    item.source.as_str(),
                    now
                ])?;
                linked_tracks += 1;
            }
        }

        tx.commit()?;
        Ok(ArtworkBatchUpsertSummary {
            input_items: items.len(),
            linked_tracks,
            inserted_assets,
            reused_assets,
            skipped_missing_tracks,
            bytes_stored,
        })
    }

    pub fn get_track_artwork_by_path(
        &self,
        track_path: &str,
    ) -> Result<Option<TrackArtworkRow>, DbError> {
        self.conn
            .query_row(
                "SELECT
                    t.id, t.path,
                    aa.id, ta.source, aa.mime_type, aa.picture_type, aa.sha256_hex, aa.byte_len, ta.extracted_at_ms
                 FROM tracks t
                 JOIN track_artwork ta ON ta.track_id = t.id
                 JOIN artwork_assets aa ON aa.id = ta.artwork_id
                 WHERE t.path = ?1
                 LIMIT 1",
                params![track_path],
                read_track_artwork_row,
            )
            .optional()
            .map_err(DbError::from)
    }

    pub fn list_artwork_assets(&self, limit: usize) -> Result<Vec<ArtworkAssetRow>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, sha256_hex, source_kind, mime_type, picture_type, byte_len, created_at_ms, updated_at_ms
             FROM artwork_assets
             ORDER BY updated_at_ms DESC, id ASC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(ArtworkAssetRow {
                id: row.get(0)?,
                sha256_hex: row.get(1)?,
                source_kind: row.get(2)?,
                mime_type: row.get(3)?,
                picture_type: row.get(4)?,
                byte_len: row.get(5)?,
                created_at_ms: row.get(6)?,
                updated_at_ms: row.get(7)?,
            })
        })?;
        collect_rows(rows)
    }

    pub fn purge_orphan_artwork_assets(&self) -> Result<usize, DbError> {
        Ok(self.conn.execute(
            "DELETE FROM artwork_assets
             WHERE id NOT IN (SELECT DISTINCT artwork_id FROM track_artwork)",
            [],
        )?)
    }

    pub fn append_track_to_playlist(
        &self,
        playlist_id: &str,
        track_id: TrackId,
    ) -> Result<i64, DbError> {
        let next_position: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM playlist_entries WHERE playlist_id = ?1",
            params![playlist_id],
            |row| row.get(0),
        )?;

        self.conn.execute(
            "INSERT INTO playlist_entries (playlist_id, track_id, position, added_at_ms)
             VALUES (?1, ?2, ?3, ?4)",
            params![playlist_id, track_id.0.to_string(), next_position, now_ms()],
        )?;
        Ok(next_position)
    }

    pub fn playlist_track_count(&self, playlist_id: &str) -> Result<i64, DbError> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM playlist_entries WHERE playlist_id = ?1",
            params![playlist_id],
            |row| row.get(0),
        )?)
    }

    pub fn list_playlist_tracks(
        &self,
        playlist_id: &str,
        limit: usize,
    ) -> Result<Vec<PlaylistTrackRow>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT
                pe.playlist_id, pe.position, pe.added_at_ms,
                t.id, t.path, t.title, t.artist, t.album,
                t.duration_ms, t.sample_rate, t.channels, t.bit_depth, t.file_mtime_ms,
                t.added_at_ms, t.updated_at_ms
             FROM playlist_entries pe
             JOIN tracks t ON t.id = pe.track_id
             WHERE pe.playlist_id = ?1
             ORDER BY pe.position ASC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![playlist_id, limit as i64], |row| {
            Ok(PlaylistTrackRow {
                playlist_id: row.get(0)?,
                position: row.get(1)?,
                added_at_ms: row.get(2)?,
                track: TrackRow {
                    id: parse_track_id_for_row(&row.get::<_, String>(3)?)?,
                    path: row.get(4)?,
                    title: row.get(5)?,
                    artist: row.get(6)?,
                    album: row.get(7)?,
                    duration_ms: row.get(8)?,
                    sample_rate: row.get(9)?,
                    channels: row.get(10)?,
                    bit_depth: row.get(11)?,
                    file_mtime_ms: row.get(12)?,
                    added_at_ms: row.get(13)?,
                    updated_at_ms: row.get(14)?,
                },
            })
        })?;
        collect_rows(rows)
    }

    pub fn clear_playlist_tracks(&self, playlist_id: &str) -> Result<usize, DbError> {
        Ok(self.conn.execute(
            "DELETE FROM playlist_entries WHERE playlist_id = ?1",
            params![playlist_id],
        )?)
    }

    pub fn remove_playlist_track_at(
        &self,
        playlist_id: &str,
        position: i64,
    ) -> Result<(), DbError> {
        let deleted = self.conn.execute(
            "DELETE FROM playlist_entries WHERE playlist_id = ?1 AND position = ?2",
            params![playlist_id, position],
        )?;
        if deleted == 0 {
            return Err(DbError::NotFound(format!(
                "playlist entry {playlist_id}@{position}"
            )));
        }
        self.conn.execute(
            "UPDATE playlist_entries
             SET position = position - 1
             WHERE playlist_id = ?1 AND position > ?2",
            params![playlist_id, position],
        )?;
        Ok(())
    }

    pub fn stats(&self) -> Result<DatabaseStats, DbError> {
        let settings_count = count_table(&self.conn, StatsTable::AppSettings)?;
        let library_root_count = count_table(&self.conn, StatsTable::LibraryRoots)?;
        let track_count = count_table(&self.conn, StatsTable::Tracks)?;
        let artwork_asset_count = count_table(&self.conn, StatsTable::ArtworkAssets)?;
        let track_artwork_count = count_table(&self.conn, StatsTable::TrackArtwork)?;
        let playlist_count = count_table(&self.conn, StatsTable::Playlists)?;
        let playlist_entry_count = count_table(&self.conn, StatsTable::PlaylistEntries)?;
        let page_count: i64 = self
            .conn
            .query_row("PRAGMA page_count;", [], |row| row.get(0))?;
        let page_size: i64 = self
            .conn
            .query_row("PRAGMA page_size;", [], |row| row.get(0))?;

        Ok(DatabaseStats {
            settings_count,
            library_root_count,
            track_count,
            artwork_asset_count,
            track_artwork_count,
            playlist_count,
            playlist_entry_count,
            page_count,
            page_size,
            db_size_bytes: page_count.saturating_mul(page_size),
        })
    }
}

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

fn read_library_root(row: &Row<'_>) -> rusqlite::Result<LibraryRootRow> {
    Ok(LibraryRootRow {
        id: row.get(0)?,
        path: row.get(1)?,
        watched: row.get::<_, i64>(2)? != 0,
        created_at_ms: row.get(3)?,
        updated_at_ms: row.get(4)?,
    })
}

fn read_track_row(row: &Row<'_>) -> rusqlite::Result<TrackRow> {
    let id_text: String = row.get(0)?;
    let id = parse_track_id_for_row(&id_text)?;
    Ok(TrackRow {
        id,
        path: row.get(1)?,
        title: row.get(2)?,
        artist: row.get(3)?,
        album: row.get(4)?,
        duration_ms: row.get(5)?,
        sample_rate: row.get(6)?,
        channels: row.get(7)?,
        bit_depth: row.get(8)?,
        file_mtime_ms: row.get(9)?,
        added_at_ms: row.get(10)?,
        updated_at_ms: row.get(11)?,
    })
}

fn read_track_artwork_row(row: &Row<'_>) -> rusqlite::Result<TrackArtworkRow> {
    let id_text: String = row.get(0)?;
    let track_id = parse_track_id_for_row(&id_text)?;
    Ok(TrackArtworkRow {
        track_id,
        track_path: row.get(1)?,
        artwork_id: row.get(2)?,
        source: row.get(3)?,
        mime_type: row.get(4)?,
        picture_type: row.get(5)?,
        sha256_hex: row.get(6)?,
        byte_len: row.get(7)?,
        extracted_at_ms: row.get(8)?,
    })
}

fn parse_track_id_for_row(id_text: &str) -> rusqlite::Result<TrackId> {
    Uuid::parse_str(id_text).map(TrackId).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
    })
}

fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&Row<'_>) -> rusqlite::Result<T>>,
) -> Result<Vec<T>, DbError> {
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn escape_sql_like(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '%' => out.push_str("\\%"),
            '_' => out.push_str("\\_"),
            _ => out.push(ch),
        }
    }
    out
}

fn bool_to_i64(v: bool) -> i64 {
    if v {
        1
    } else {
        0
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

fn now_ms() -> i64 {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before UNIX_EPOCH");
    dur.as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TrackRecord;
    use auric_core::TrackId;
    use serde_json::json;

    fn sample_track(path: &str) -> TrackRecord {
        TrackRecord {
            id: TrackId(Uuid::new_v4()),
            path: path.to_string(),
            title: Some("Track".to_string()),
            artist: Some("Artist".to_string()),
            album: Some("Album".to_string()),
            duration_ms: None,
            sample_rate: None,
            channels: None,
            bit_depth: None,
            file_mtime_ms: None,
        }
    }

    #[test]
    fn initializes_schema_and_pragmas() {
        let db = Database::open_in_memory_for_tests().expect("db open");
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
        db.quick_check().unwrap();
        let p = db.pragma_snapshot().unwrap();
        assert!(p.foreign_keys);
    }

    #[test]
    fn settings_round_trip_json() {
        let db = Database::open_in_memory_for_tests().unwrap();
        db.set_setting_json("ui.theme", &json!("auric-dark"))
            .unwrap();
        db.set_setting_json("features.visualizer", &json!(false))
            .unwrap();
        assert_eq!(
            db.get_setting_json("ui.theme").unwrap(),
            Some(json!("auric-dark"))
        );
        assert_eq!(db.get_setting_json("missing").unwrap(), None);
    }

    #[test]
    fn library_root_upsert_is_idempotent_by_path() {
        let db = Database::open_in_memory_for_tests().unwrap();
        let first = db
            .upsert_library_root(&LibraryRoot {
                path: "/music".into(),
                watched: true,
            })
            .unwrap();
        let second = db
            .upsert_library_root(&LibraryRoot {
                path: "/music".into(),
                watched: false,
            })
            .unwrap();
        assert_eq!(first.id, second.id);
        assert!(!second.watched);
        assert_eq!(db.list_library_roots().unwrap().len(), 1);
    }

    #[test]
    fn playlist_crud_and_entries_work() {
        let db = Database::open_in_memory_for_tests().unwrap();
        let playlist_id = db.create_playlist("Favorites").unwrap();
        db.rename_playlist(&playlist_id, "Faves").unwrap();

        let t1 = sample_track("/music/a.flac");
        let t2 = sample_track("/music/b.flac");
        db.upsert_track(&t1).unwrap();
        db.upsert_track(&t2).unwrap();

        assert_eq!(db.append_track_to_playlist(&playlist_id, t1.id).unwrap(), 0);
        assert_eq!(db.append_track_to_playlist(&playlist_id, t2.id).unwrap(), 1);
        assert_eq!(db.playlist_track_count(&playlist_id).unwrap(), 2);

        let playlists = db.list_playlists().unwrap();
        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0].name, "Faves");

        db.delete_playlist(&playlist_id).unwrap();
        assert!(db.list_playlists().unwrap().is_empty());
    }

    #[test]
    fn batch_track_upsert_and_stats() {
        let mut db = Database::open_in_memory_for_tests().unwrap();
        let tracks: Vec<_> = (0..2_000)
            .map(|i| TrackRecord {
                id: TrackId(Uuid::new_v4()),
                path: format!("/music/{i:05}.flac"),
                title: Some(format!("Track {i}")),
                artist: Some("Batch Artist".to_string()),
                album: Some("Batch Album".to_string()),
                duration_ms: None,
                sample_rate: None,
                channels: None,
                bit_depth: None,
                file_mtime_ms: None,
            })
            .collect();

        let inserted = db.upsert_tracks_batch(&tracks).unwrap();
        assert_eq!(inserted, tracks.len());
        assert_eq!(db.count_tracks().unwrap(), 2_000);

        let listed = db.list_tracks(5).unwrap();
        assert_eq!(listed.len(), 5);

        let stats = db.stats().unwrap();
        assert_eq!(stats.track_count, 2_000);
        assert_eq!(stats.artwork_asset_count, 0);
        assert_eq!(stats.track_artwork_count, 0);
        assert!(stats.page_size > 0);
        db.quick_check().unwrap();
    }

    #[test]
    fn list_and_delete_tracks_by_prefix() {
        let mut db = Database::open_in_memory_for_tests().unwrap();
        let inside_a = sample_track("/music/a.flac");
        let inside_b = sample_track("/music/sub/b.flac");
        let outside = sample_track("/other/c.flac");
        db.upsert_track(&inside_a).unwrap();
        db.upsert_track(&inside_b).unwrap();
        db.upsert_track(&outside).unwrap();

        let paths = db.list_track_paths_under_prefix("/music").unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths.iter().any(|p| p == "/music/a.flac"));
        assert!(paths.iter().any(|p| p == "/music/sub/b.flac"));

        let deleted = db.delete_tracks_by_paths(&paths).unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(db.count_tracks().unwrap(), 1);
    }

    #[test]
    fn artwork_cache_dedupes_and_purges_orphans() {
        let mut db = Database::open_in_memory_for_tests().unwrap();
        let t1 = sample_track("/music/a.flac");
        let t2 = sample_track("/music/b.flac");
        db.upsert_track(&t1).unwrap();
        db.upsert_track(&t2).unwrap();

        let summary = db
            .upsert_track_artwork_batch(&[
                TrackArtworkUpsert {
                    track_path: t1.path.clone(),
                    source_kind: "embedded".to_string(),
                    source: "embedded".to_string(),
                    mime_type: Some("image/jpeg".to_string()),
                    picture_type: Some("CoverFront".to_string()),
                    bytes: vec![1, 2, 3, 4],
                },
                TrackArtworkUpsert {
                    track_path: t2.path.clone(),
                    source_kind: "embedded".to_string(),
                    source: "embedded".to_string(),
                    mime_type: Some("image/jpeg".to_string()),
                    picture_type: Some("CoverFront".to_string()),
                    bytes: vec![1, 2, 3, 4],
                },
            ])
            .unwrap();
        assert_eq!(summary.input_items, 2);
        assert_eq!(summary.linked_tracks, 2);
        assert_eq!(summary.inserted_assets, 1);
        assert_eq!(summary.reused_assets, 1);
        assert_eq!(db.count_artwork_assets().unwrap(), 1);
        assert_eq!(db.count_track_artwork_links().unwrap(), 2);

        let row = db.get_track_artwork_by_path(&t1.path).unwrap().unwrap();
        assert_eq!(row.mime_type.as_deref(), Some("image/jpeg"));
        assert_eq!(row.byte_len, 4);

        db.delete_tracks_by_paths(std::slice::from_ref(&t1.path))
            .unwrap();
        assert_eq!(db.purge_orphan_artwork_assets().unwrap(), 0);
        assert_eq!(db.count_artwork_assets().unwrap(), 1);

        db.delete_tracks_by_paths(std::slice::from_ref(&t2.path))
            .unwrap();
        assert_eq!(db.purge_orphan_artwork_assets().unwrap(), 1);
        assert_eq!(db.count_artwork_assets().unwrap(), 0);
    }

    #[test]
    fn migrates_v1_database_to_v2_artwork_schema() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE tracks (
                id TEXT PRIMARY KEY,
                path TEXT NOT NULL UNIQUE,
                title TEXT,
                artist TEXT,
                album TEXT,
                duration_ms INTEGER,
                sample_rate INTEGER,
                channels INTEGER,
                bit_depth INTEGER,
                file_mtime_ms INTEGER,
                added_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );
            PRAGMA user_version = 1;
            "#,
        )
        .unwrap();

        let options = DatabaseOptions {
            journal_mode: JournalMode::Memory,
            synchronous: SynchronousMode::Off,
            ..DatabaseOptions::default()
        };
        let db = Database::from_connection(conn, &options, None).unwrap();
        assert_eq!(db.schema_version().unwrap(), 2);
        assert_eq!(db.count_artwork_assets().unwrap(), 0);
        assert_eq!(db.count_track_artwork_links().unwrap(), 0);
    }
}
