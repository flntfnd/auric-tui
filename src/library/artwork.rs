use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use lofty::config::WriteOptions;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::*;
use musicbrainz_rs::entity::release::Release;
use musicbrainz_rs::prelude::*;
use tokio::sync::mpsc;

const COVER_ART_ARCHIVE_BASE: &str = "https://coverartarchive.org";
const USER_AGENT: &str = "auric-tui/0.1.0 (https://github.com/user/auric-tui)";

#[derive(Debug, Clone)]
pub enum ArtworkEvent {
    Found {
        artist: String,
        album: String,
        data: Vec<u8>,
    },
    NotFound {
        artist: String,
        album: String,
    },
    Written {
        path: String,
    },
    Error {
        artist: String,
        album: String,
        error: String,
    },
}

pub struct ArtworkFetcher {
    client: reqwest::Client,
}

impl ArtworkFetcher {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(30))
            .build()?;

        Ok(Self { client })
    }

    /// Search MusicBrainz for a release and fetch its cover art
    pub async fn fetch_artwork(&self, artist: &str, album: &str) -> Result<Option<Vec<u8>>> {
        // Search for the release on MusicBrainz
        let mbid = self.search_release(artist, album).await?;

        let Some(mbid) = mbid else {
            return Ok(None);
        };

        // Fetch cover art from Cover Art Archive
        self.fetch_cover_art(&mbid).await
    }

    /// Search MusicBrainz for a release by artist and album name
    async fn search_release(&self, artist: &str, album: &str) -> Result<Option<String>> {
        // Build search query - escape special characters
        let artist_clean = Self::sanitize_query(artist);
        let album_clean = Self::sanitize_query(album);

        let query = format!("artist:\"{}\" AND release:\"{}\"", artist_clean, album_clean);

        let results = match Release::search(query).execute().await {
            Ok(r) => r,
            Err(_) => return Ok(None), // Silently return None on search errors
        };

        // Return the first result's MBID if found
        if let Some(release) = results.entities.first() {
            return Ok(Some(release.id.clone()));
        }

        // Try a more relaxed search without quotes
        let query = format!("artist:{} AND release:{}", artist_clean, album_clean);

        let results = match Release::search(query).execute().await {
            Ok(r) => r,
            Err(_) => return Ok(None), // Silently return None on search errors
        };

        Ok(results.entities.first().map(|r| r.id.clone()))
    }

    /// Fetch cover art from Cover Art Archive
    async fn fetch_cover_art(&self, mbid: &str) -> Result<Option<Vec<u8>>> {
        // Try to get the front cover (500px version for good quality without being huge)
        let url = format!("{}/release/{}/front-500", COVER_ART_ARCHIVE_BASE, mbid);

        let response = self.client.get(&url).send().await;

        match response {
            Ok(resp) => {
                if resp.status().is_success() {
                    let bytes = resp.bytes().await?;
                    return Ok(Some(bytes.to_vec()));
                } else if resp.status().as_u16() == 404 {
                    // No cover art available, try release-group
                    return self.fetch_release_group_art(mbid).await;
                }
            }
            Err(_) => {
                // Silently try release group on error
            }
        }

        // Fallback to release group artwork
        self.fetch_release_group_art(mbid).await
    }

    /// Try to fetch artwork from the release group (covers multiple editions)
    async fn fetch_release_group_art(&self, mbid: &str) -> Result<Option<Vec<u8>>> {
        // First, get the release to find its release-group
        let release = Release::fetch()
            .id(mbid)
            .with_release_groups()
            .execute()
            .await;

        if let Ok(release) = release {
            if let Some(rg) = release.release_group {
                let url = format!(
                    "{}/release-group/{}/front-500",
                    COVER_ART_ARCHIVE_BASE, rg.id
                );

                let response = self.client.get(&url).send().await;

                if let Ok(resp) = response {
                    if resp.status().is_success() {
                        let bytes = resp.bytes().await?;
                        return Ok(Some(bytes.to_vec()));
                    }
                }
            }
        }

        Ok(None)
    }

    /// Sanitize a string for use in MusicBrainz Lucene query
    fn sanitize_query(s: &str) -> String {
        // Escape Lucene special characters
        let special_chars = [
            '+', '-', '&', '|', '!', '(', ')', '{', '}', '[', ']', '^', '"', '~', '*', '?', ':',
            '\\', '/',
        ];

        let mut result = String::with_capacity(s.len());
        for c in s.chars() {
            if special_chars.contains(&c) {
                result.push('\\');
            }
            result.push(c);
        }
        result
    }
}

impl Default for ArtworkFetcher {
    fn default() -> Self {
        Self::new().expect("Failed to create HTTP client")
    }
}

/// Write album art to a file's metadata
pub fn write_artwork_to_file(path: &Path, image_data: &[u8]) -> Result<()> {
    // Detect image type from magic bytes
    let mime_type = detect_mime_type(image_data)?;

    // Create a Picture
    let picture = Picture::new_unchecked(
        PictureType::CoverFront,
        Some(mime_type),
        None,
        image_data.to_vec(),
    );

    // Open the audio file
    let mut tagged_file = lofty::read_from_path(path)?;

    // Get or create the primary tag - try primary first, then first available
    let has_primary = tagged_file.primary_tag_mut().is_some();

    let tag = if has_primary {
        tagged_file.primary_tag_mut()
    } else {
        tagged_file.first_tag_mut()
    };

    if let Some(tag) = tag {
        // Remove existing front cover pictures
        tag.remove_picture_type(PictureType::CoverFront);

        // Add the new picture
        tag.push_picture(picture);

        // Save the file
        tag.save_to_path(path, WriteOptions::default())?;

        Ok(())
    } else {
        anyhow::bail!("No suitable tag found in file")
    }
}

/// Detect MIME type from image data magic bytes
fn detect_mime_type(data: &[u8]) -> Result<MimeType> {
    if data.len() < 8 {
        anyhow::bail!("Image data too small");
    }

    // Check magic bytes
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Ok(MimeType::Jpeg)
    } else if data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        Ok(MimeType::Png)
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Ok(MimeType::Gif)
    } else if data.starts_with(b"RIFF") && data.len() > 12 && &data[8..12] == b"WEBP" {
        // WebP is not directly supported by lofty's MimeType, use Unknown
        Ok(MimeType::Unknown(String::from("image/webp")))
    } else {
        // Default to JPEG for unknown
        Ok(MimeType::Jpeg)
    }
}

/// Background task to fetch artwork for tracks missing album art
pub async fn fetch_missing_artwork(
    tracks: Vec<(String, String, String)>, // (path, artist, album)
    tx: mpsc::Sender<ArtworkEvent>,
) {
    let fetcher = match ArtworkFetcher::new() {
        Ok(f) => f,
        Err(e) => {
            let _ = tx
                .send(ArtworkEvent::Error {
                    artist: String::new(),
                    album: String::new(),
                    error: format!("Failed to create fetcher: {}", e),
                })
                .await;
            return;
        }
    };

    for (path, artist, album) in tracks {
        // Rate limit: MusicBrainz asks for 1 request per second
        tokio::time::sleep(Duration::from_millis(1100)).await;

        match fetcher.fetch_artwork(&artist, &album).await {
            Ok(Some(data)) => {
                // Write to file
                let file_path = std::path::Path::new(&path);
                match write_artwork_to_file(file_path, &data) {
                    Ok(()) => {
                        let _ = tx
                            .send(ArtworkEvent::Written {
                                path: path.clone(),
                            })
                            .await;
                        let _ = tx
                            .send(ArtworkEvent::Found {
                                artist: artist.clone(),
                                album: album.clone(),
                                data,
                            })
                            .await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(ArtworkEvent::Error {
                                artist: artist.clone(),
                                album: album.clone(),
                                error: format!("Failed to write artwork: {}", e),
                            })
                            .await;
                    }
                }
            }
            Ok(None) => {
                let _ = tx
                    .send(ArtworkEvent::NotFound {
                        artist: artist.clone(),
                        album: album.clone(),
                    })
                    .await;
            }
            Err(e) => {
                let _ = tx
                    .send(ArtworkEvent::Error {
                        artist,
                        album,
                        error: e.to_string(),
                    })
                    .await;
            }
        }
    }
}
