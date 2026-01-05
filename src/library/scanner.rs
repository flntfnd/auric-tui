use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use lofty::prelude::*;
use lofty::probe::Probe;
use tokio::sync::mpsc;

use super::track::{AudioFormat, Track};

pub enum ScanEvent {
    TrackFound(Track),
    ScanComplete { folder: String, count: usize },
    Error { path: String, error: String },
}

pub struct Scanner;

impl Scanner {
    pub async fn scan_folder(
        path: &Path,
        sender: mpsc::Sender<ScanEvent>,
    ) -> Result<usize> {
        let folder_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();

        let mut count = 0;
        Self::scan_recursive(path, &sender, &mut count).await;

        let _ = sender
            .send(ScanEvent::ScanComplete {
                folder: folder_name,
                count,
            })
            .await;

        Ok(count)
    }

    async fn scan_recursive(
        path: &Path,
        sender: &mpsc::Sender<ScanEvent>,
        count: &mut usize,
    ) {
        let entries = match std::fs::read_dir(path) {
            Ok(e) => e,
            Err(e) => {
                let _ = sender
                    .send(ScanEvent::Error {
                        path: path.display().to_string(),
                        error: e.to_string(),
                    })
                    .await;
                return;
            }
        };

        for entry in entries.flatten() {
            let entry_path = entry.path();

            if entry_path.is_dir() {
                Box::pin(Self::scan_recursive(&entry_path, sender, count)).await;
            } else if let Some(ext) = entry_path.extension().and_then(|e| e.to_str()) {
                if AudioFormat::is_supported(ext) {
                    match Self::read_track_metadata(&entry_path) {
                        Ok(track) => {
                            *count += 1;
                            let _ = sender.send(ScanEvent::TrackFound(track)).await;
                        }
                        Err(e) => {
                            let _ = sender
                                .send(ScanEvent::Error {
                                    path: entry_path.display().to_string(),
                                    error: e.to_string(),
                                })
                                .await;
                        }
                    }
                }
            }
        }
    }

    pub fn read_track_metadata(path: &Path) -> Result<Track> {
        let mut track = Track::new(path.to_path_buf());

        // Set title from filename as fallback
        track.title = path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();

        // Try to read metadata with lofty
        if let Ok(tagged_file) = Probe::open(path)?.read() {
            // Get duration from properties
            let properties = tagged_file.properties();
            track.duration = properties.duration();

            // Get tags
            if let Some(tag) = tagged_file.primary_tag().or_else(|| tagged_file.first_tag()) {
                if let Some(title) = tag.title() {
                    track.title = title.to_string();
                }
                if let Some(artist) = tag.artist() {
                    track.artist = artist.to_string();
                }
                if let Some(album) = tag.album() {
                    track.album = album.to_string();
                }
                track.track_number = tag.track();
                track.disc_number = tag.disk();

                // Try to get album artist
                if let Some(album_artist) = tag.get_string(&lofty::tag::ItemKey::AlbumArtist) {
                    track.album_artist = Some(album_artist.to_string());
                }

                // Extract album art
                if let Some(picture) = tag.pictures().first() {
                    track.album_art_data = Some(picture.data().to_vec());
                }
            }
        } else {
            // If we can't read metadata, try to get duration from symphonia
            track.duration = Self::get_duration_symphonia(path).unwrap_or(Duration::ZERO);
        }

        Ok(track)
    }

    fn get_duration_symphonia(path: &Path) -> Result<Duration> {
        use symphonia::core::formats::FormatOptions;
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;
        use symphonia::core::probe::Hint;

        let file = std::fs::File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let format_opts = FormatOptions::default();
        let metadata_opts = MetadataOptions::default();

        let probed = symphonia::default::get_probe().format(
            &hint,
            mss,
            &format_opts,
            &metadata_opts,
        )?;

        let format = probed.format;

        if let Some(track) = format.default_track() {
            let time_base = track.codec_params.time_base;
            let n_frames = track.codec_params.n_frames;

            if let (Some(tb), Some(frames)) = (time_base, n_frames) {
                let duration_secs = (frames as f64 * tb.numer as f64) / tb.denom as f64;
                return Ok(Duration::from_secs_f64(duration_secs));
            }
        }

        Ok(Duration::ZERO)
    }
}
