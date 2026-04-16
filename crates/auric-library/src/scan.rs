use crate::db::{Database, TrackArtworkUpsert};
use crate::TrackRecord;
use auric_core::TrackId;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::Instant;
use uuid::Uuid;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub batch_size: usize,
    pub prune_missing: bool,
    pub follow_symlinks: bool,
    pub read_embedded_artwork: bool,
    pub max_embedded_artwork_bytes: usize,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            batch_size: 2_000,
            prune_missing: false,
            follow_symlinks: false,
            read_embedded_artwork: true,
            max_embedded_artwork_bytes: 8 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanSummary {
    pub root_path: String,
    pub discovered_audio_files: usize,
    pub imported_tracks: usize,
    pub embedded_artwork_candidates: usize,
    pub embedded_artwork_linked_tracks: usize,
    pub embedded_artwork_inserted_assets: usize,
    pub embedded_artwork_reused_assets: usize,
    pub embedded_artwork_skipped_oversize: usize,
    pub skipped_non_audio_files: usize,
    pub skipped_unreadable_entries: usize,
    pub pruned_missing_tracks: usize,
    pub purged_orphan_artwork_assets: usize,
    pub elapsed_ms: u128,
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("invalid scan root: {0}")]
    InvalidRoot(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("db error: {0}")]
    Db(#[from] crate::db::DbError),
    #[error("walkdir error: {0}")]
    Walkdir(#[from] walkdir::Error),
}

#[derive(Debug, Clone)]
pub struct DirectoryScanner {
    options: ScanOptions,
}

impl DirectoryScanner {
    pub fn new(options: ScanOptions) -> Self {
        Self { options }
    }

    pub fn options(&self) -> &ScanOptions {
        &self.options
    }

    pub fn scan_path(
        &self,
        db: &mut Database,
        root: impl AsRef<Path>,
    ) -> Result<ScanSummary, ScanError> {
        let root = root.as_ref();
        let root_meta = fs::metadata(root).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                ScanError::InvalidRoot(format!("{} (not found)", root.display()))
            } else {
                ScanError::Io(err)
            }
        })?;
        if !root_meta.is_dir() {
            return Err(ScanError::InvalidRoot(format!(
                "{} (not a directory)",
                root.display()
            )));
        }

        let root_path = normalize_path(root)?;
        let start = Instant::now();
        let mut discovered_audio_files = 0usize;
        let mut imported_tracks = 0usize;
        let mut embedded_artwork_candidates = 0usize;
        let mut embedded_artwork_linked_tracks = 0usize;
        let mut embedded_artwork_inserted_assets = 0usize;
        let mut embedded_artwork_reused_assets = 0usize;
        let mut embedded_artwork_skipped_oversize = 0usize;
        let mut skipped_non_audio_files = 0usize;
        let mut skipped_unreadable_entries = 0usize;
        let mut batch = Vec::with_capacity(self.options.batch_size.max(1));
        let mut artwork_batch = Vec::with_capacity(self.options.batch_size.max(1));
        let mut seen_audio_paths = if self.options.prune_missing {
            Some(HashSet::new())
        } else {
            None
        };

        let walker = WalkDir::new(&root_path).follow_links(self.options.follow_symlinks);
        for entry in walker {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    skipped_unreadable_entries += 1;
                    continue;
                }
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            if !is_supported_audio_file(path) {
                skipped_non_audio_files += 1;
                continue;
            }

            let path_string = normalize_path(path)?;
            if let Some(seen) = &mut seen_audio_paths {
                seen.insert(path_string.clone());
            }

            let metadata = probe_embedded_metadata(
                path,
                self.options.read_embedded_artwork,
                self.options.max_embedded_artwork_bytes,
            );
            let (artist, album) = metadata
                .as_ref()
                .map(|m| (m.artist.clone(), m.album.clone()))
                .unwrap_or_else(|| infer_artist_album(path));
            let title = metadata
                .as_ref()
                .and_then(|m| m.title.clone())
                .or_else(|| infer_title(path));
            let duration_ms = metadata.as_ref().and_then(|m| m.duration_ms);
            let sample_rate = metadata.as_ref().and_then(|m| m.sample_rate);
            let channels = metadata.as_ref().and_then(|m| m.channels);
            let bit_depth = metadata.as_ref().and_then(|m| m.bit_depth);
            let file_mtime_ms = file_mtime_ms(path);
            let artwork = metadata.as_ref().and_then(|m| m.artwork.clone());
            let artwork_oversize = metadata
                .as_ref()
                .and_then(|m| m.artwork_oversize_bytes)
                .is_some();

            batch.push(TrackRecord {
                id: TrackId(Uuid::new_v4()),
                path: path_string.clone(),
                title,
                artist,
                album,
                duration_ms,
                sample_rate,
                channels,
                bit_depth,
                file_mtime_ms,
            });
            discovered_audio_files += 1;
            if artwork_oversize {
                embedded_artwork_skipped_oversize += 1;
            }
            if let Some(artwork) = artwork {
                embedded_artwork_candidates += 1;
                artwork_batch.push(TrackArtworkUpsert {
                    track_path: path_string,
                    source_kind: "embedded".to_string(),
                    source: "embedded".to_string(),
                    mime_type: artwork.mime_type,
                    picture_type: artwork.picture_type,
                    bytes: artwork.bytes,
                });
            }

            if batch.len() >= self.options.batch_size.max(1) {
                imported_tracks += db.upsert_tracks_batch(&batch)?;
                if !artwork_batch.is_empty() {
                    let art_summary = db.upsert_track_artwork_batch(&artwork_batch)?;
                    embedded_artwork_linked_tracks += art_summary.linked_tracks;
                    embedded_artwork_inserted_assets += art_summary.inserted_assets;
                    embedded_artwork_reused_assets += art_summary.reused_assets;
                    artwork_batch.clear();
                }
                batch.clear();
            }
        }

        if !batch.is_empty() {
            imported_tracks += db.upsert_tracks_batch(&batch)?;
            if !artwork_batch.is_empty() {
                let art_summary = db.upsert_track_artwork_batch(&artwork_batch)?;
                embedded_artwork_linked_tracks += art_summary.linked_tracks;
                embedded_artwork_inserted_assets += art_summary.inserted_assets;
                embedded_artwork_reused_assets += art_summary.reused_assets;
                artwork_batch.clear();
            }
        }

        let pruned_missing_tracks = if self.options.prune_missing {
            self.prune_missing_under_root(db, &root_path, seen_audio_paths.as_ref())?
        } else {
            0
        };

        let purge_orphans = self.options.read_embedded_artwork || self.options.prune_missing;
        let purged_orphan_artwork_assets = if purge_orphans {
            db.purge_orphan_artwork_assets()?
        } else {
            0
        };

        Ok(ScanSummary {
            root_path,
            discovered_audio_files,
            imported_tracks,
            embedded_artwork_candidates,
            embedded_artwork_linked_tracks,
            embedded_artwork_inserted_assets,
            embedded_artwork_reused_assets,
            embedded_artwork_skipped_oversize,
            skipped_non_audio_files,
            skipped_unreadable_entries,
            pruned_missing_tracks,
            purged_orphan_artwork_assets,
            elapsed_ms: start.elapsed().as_millis(),
        })
    }

    pub fn scan_saved_roots(&self, db: &mut Database) -> Result<Vec<ScanSummary>, ScanError> {
        let roots = db.list_library_roots()?;
        let mut summaries = Vec::with_capacity(roots.len());
        for root in roots {
            summaries.push(self.scan_path(db, Path::new(&root.path))?);
        }
        Ok(summaries)
    }

    fn prune_missing_under_root(
        &self,
        db: &mut Database,
        root_path: &str,
        seen_audio_paths: Option<&HashSet<String>>,
    ) -> Result<usize, ScanError> {
        let mut delete_paths = Vec::new();
        for path in db.list_track_paths_under_prefix(root_path)? {
            let should_keep = seen_audio_paths.is_some_and(|seen| seen.contains(&path));
            if should_keep {
                continue;
            }
            if !Path::new(&path).exists() {
                delete_paths.push(path);
            }
        }
        Ok(db.delete_tracks_by_paths(&delete_paths)?)
    }
}

fn is_supported_audio_file(path: &Path) -> bool {
    let ext = match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => ext,
        None => return false,
    };

    matches!(
        ext.to_ascii_lowercase().as_str(),
        "flac"
            | "wav"
            | "wave"
            | "aiff"
            | "aif"
            | "mp3"
            | "m4a"
            | "aac"
            | "alac"
            | "ogg"
            | "opus"
            | "wma"
            | "ape"
            | "wv"
            | "dsf"
            | "dff"
    )
}

fn infer_title(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_string_lossy();
    let title = stem.replace('_', " ").trim().to_string();
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

fn infer_artist_album(path: &Path) -> (Option<String>, Option<String>) {
    let mut comps = path
        .parent()
        .into_iter()
        .flat_map(|p| p.components())
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>();

    if comps.len() >= 2 {
        let album = comps.pop().filter(|s| !s.is_empty());
        let artist = comps.pop().filter(|s| !s.is_empty());
        (artist, album)
    } else if comps.len() == 1 {
        (None, comps.pop().filter(|s| !s.is_empty()))
    } else {
        (None, None)
    }
}

#[derive(Debug, Clone, Default)]
struct EmbeddedMetadata {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    duration_ms: Option<i64>,
    sample_rate: Option<i64>,
    channels: Option<i64>,
    bit_depth: Option<i64>,
    artwork: Option<EmbeddedArtwork>,
    artwork_oversize_bytes: Option<usize>,
}

#[derive(Debug, Clone)]
struct EmbeddedArtwork {
    mime_type: Option<String>,
    picture_type: Option<String>,
    bytes: Vec<u8>,
}

fn probe_embedded_metadata(
    path: &Path,
    read_embedded_artwork: bool,
    max_embedded_artwork_bytes: usize,
) -> Option<EmbeddedMetadata> {
    use lofty::file::{AudioFile, TaggedFileExt};
    use lofty::picture::PictureType;
    use lofty::probe::Probe;
    use lofty::tag::Accessor;

    let tagged_file = Probe::open(path).ok()?.read().ok()?;
    let props = tagged_file.properties();
    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag());

    let duration_ms = i64::try_from(props.duration().as_millis()).ok();
    let sample_rate = props.sample_rate().map(i64::from);
    let channels = props.channels().map(i64::from);
    let bit_depth = props.bit_depth().map(i64::from);

    let title = tag.and_then(|t| t.title()).map(|s| s.into_owned());
    let artist = tag.and_then(|t| t.artist()).map(|s| s.into_owned());
    let album = tag.and_then(|t| t.album()).map(|s| s.into_owned());
    let (artwork, artwork_oversize_bytes) = if read_embedded_artwork {
        let picture = tag.and_then(|t| {
            t.get_picture_type(PictureType::CoverFront)
                .or_else(|| t.pictures().first())
        });
        match picture {
            Some(pic) if pic.data().len() > max_embedded_artwork_bytes => {
                (None, Some(pic.data().len()))
            }
            Some(pic) => (
                Some(EmbeddedArtwork {
                    mime_type: pic.mime_type().map(|m| m.as_str().to_string()),
                    picture_type: Some(format!("{:?}", pic.pic_type())),
                    bytes: pic.data().to_vec(),
                }),
                None,
            ),
            None => (None, None),
        }
    } else {
        (None, None)
    };

    Some(EmbeddedMetadata {
        title,
        artist,
        album,
        duration_ms,
        sample_rate,
        channels,
        bit_depth,
        artwork,
        artwork_oversize_bytes,
    })
}

fn file_mtime_ms(path: &Path) -> Option<i64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let since_epoch = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(since_epoch.as_millis()).ok()
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use tempfile::tempdir;

    #[test]
    fn scans_audio_files_and_ignores_non_audio() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("Artist").join("Album");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("01_intro.flac"), b"x").unwrap();
        fs::write(root.join("02_song.MP3"), b"x").unwrap();
        fs::write(root.join("cover.jpg"), b"x").unwrap();
        fs::write(root.join("notes.txt"), b"x").unwrap();

        let mut db = Database::open_in_memory_for_tests().unwrap();
        let scanner = DirectoryScanner::new(ScanOptions {
            batch_size: 1,
            prune_missing: false,
            follow_symlinks: false,
            read_embedded_artwork: true,
            max_embedded_artwork_bytes: 8 * 1024 * 1024,
        });

        let summary = scanner.scan_path(&mut db, dir.path()).unwrap();
        assert_eq!(summary.discovered_audio_files, 2);
        assert_eq!(summary.imported_tracks, 2);
        assert_eq!(summary.skipped_non_audio_files, 2);
        assert_eq!(db.count_tracks().unwrap(), 2);

        let rows = db.list_tracks(10).unwrap();
        assert!(rows.iter().any(|t| t.title.as_deref() == Some("01 intro")));
        assert!(rows.iter().all(|t| t.album.as_deref() == Some("Album")));
    }

    #[test]
    fn prune_missing_removes_deleted_tracks_under_root() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("music");
        fs::create_dir_all(&root).unwrap();
        let keep = root.join("keep.flac");
        let gone = root.join("gone.flac");
        fs::write(&keep, b"x").unwrap();
        fs::write(&gone, b"x").unwrap();

        let mut db = Database::open_in_memory_for_tests().unwrap();
        let scanner = DirectoryScanner::new(ScanOptions {
            batch_size: 32,
            prune_missing: false,
            follow_symlinks: false,
            read_embedded_artwork: true,
            max_embedded_artwork_bytes: 8 * 1024 * 1024,
        });
        scanner.scan_path(&mut db, &root).unwrap();
        assert_eq!(db.count_tracks().unwrap(), 2);

        fs::remove_file(&gone).unwrap();
        let prune_scanner = DirectoryScanner::new(ScanOptions {
            prune_missing: true,
            ..ScanOptions::default()
        });
        let summary = prune_scanner.scan_path(&mut db, &root).unwrap();
        assert_eq!(summary.pruned_missing_tracks, 1);
        assert_eq!(db.count_tracks().unwrap(), 1);
    }
}
