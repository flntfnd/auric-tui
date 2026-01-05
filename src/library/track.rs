use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Utc};
use image::DynamicImage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Flac,
    Wav,
    Alac,
    Ape,
    Aiff,
    Mp3,
    Unknown,
}

impl AudioFormat {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "flac" => Self::Flac,
            "wav" => Self::Wav,
            "m4a" | "alac" => Self::Alac,
            "ape" => Self::Ape,
            "aiff" | "aif" => Self::Aiff,
            "mp3" => Self::Mp3,
            _ => Self::Unknown,
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            Self::Flac => "flac",
            Self::Wav => "wav",
            Self::Alac => "m4a",
            Self::Ape => "ape",
            Self::Aiff => "aiff",
            Self::Mp3 => "mp3",
            Self::Unknown => "unknown",
        }
    }

    pub fn is_supported(ext: &str) -> bool {
        !matches!(Self::from_extension(ext), Self::Unknown)
    }
}

#[derive(Debug, Clone)]
pub struct Track {
    pub id: uuid::Uuid,
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: Option<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub duration: Duration,
    pub format: AudioFormat,
    pub added_at: DateTime<Utc>,
    pub album_art: Option<DynamicImage>,
    pub album_art_data: Option<Vec<u8>>,
}

impl Track {
    pub fn new(path: PathBuf) -> Self {
        let format = path
            .extension()
            .and_then(|e| e.to_str())
            .map(AudioFormat::from_extension)
            .unwrap_or(AudioFormat::Unknown);

        Self {
            id: uuid::Uuid::new_v4(),
            path,
            title: String::new(),
            artist: String::from("Unknown Artist"),
            album: String::from("Unknown Album"),
            album_artist: None,
            track_number: None,
            disc_number: None,
            duration: Duration::ZERO,
            format,
            added_at: Utc::now(),
            album_art: None,
            album_art_data: None,
        }
    }

    pub fn format_duration(&self) -> String {
        let secs = self.duration.as_secs();
        let mins = secs / 60;
        let secs = secs % 60;
        format!("{}:{:02}", mins, secs)
    }
}

impl Default for Track {
    fn default() -> Self {
        Self::new(PathBuf::new())
    }
}
