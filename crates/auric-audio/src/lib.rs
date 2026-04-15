pub mod player;

use async_trait::async_trait;
use cpal::traits::{DeviceTrait, HostTrait};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::{Path, PathBuf};
use symphonia::core::codecs::CODEC_TYPE_NULL;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub bit_depth: u16,
}

#[async_trait]
pub trait DecoderBackend: Send + Sync {
    async fn supports(&self, source_uri: &str) -> bool;
    async fn inspect_format(&self, source_uri: &str) -> Result<StreamFormat, AudioError>;
}

#[async_trait]
pub trait OutputBackend: Send + Sync {
    async fn list_devices(&self) -> Result<Vec<AudioDevice>, AudioError>;
    async fn start(&self) -> Result<(), AudioError>;
}

/// A node in the DSP processing chain.
///
/// # Real-time safety
///
/// Implementations of `process` (when added) MUST be real-time safe:
/// no heap allocations, no locks/mutexes, no blocking I/O, no Objective-C
/// messaging. Cross-thread communication must use lock-free primitives only.
pub trait DspNode: Send {
    fn id(&self) -> &'static str;
    fn enabled(&self) -> bool;
}

/// A tap that receives copies of audio frames for analysis (metering, FFT, etc).
///
/// # Real-time safety
///
/// `push_frame` is called from the audio callback thread. Implementations MUST
/// be allocation-free and lock-free. Use a lock-free ring buffer (e.g. `rtrb`)
/// to transfer data to a non-real-time consumer thread.
pub trait AnalysisTap: Send + Sync {
    fn push_frame(&self, _interleaved: &[f32], _format: StreamFormat) {}
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub default_output: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioInspection {
    pub source_uri: String,
    pub resolved_path: String,
    pub format: StreamFormat,
}

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("backend unavailable: {0}")]
    BackendUnavailable(String),
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("unsupported source URI: {0}")]
    UnsupportedSourceUri(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("probe error: {0}")]
    Probe(String),
}

pub struct AudioEngine {
    taps: Vec<Box<dyn AnalysisTap>>,
    dsp_chain: Vec<Box<dyn DspNode>>,
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEngine {
    pub fn new() -> Self {
        Self {
            taps: Vec::new(),
            dsp_chain: Vec::new(),
        }
    }

    pub fn add_tap(&mut self, tap: Box<dyn AnalysisTap>) {
        self.taps.push(tap);
    }

    pub fn add_dsp_node(&mut self, node: Box<dyn DspNode>) {
        self.dsp_chain.push(node);
    }

    pub fn inspect_source_uri(&self, source_uri: &str) -> Result<AudioInspection, AudioError> {
        let backend = SymphoniaDecoderBackend;
        let path = parse_local_source_uri(source_uri)?;
        let format = backend.inspect_path(&path)?;
        Ok(AudioInspection {
            source_uri: source_uri.to_string(),
            resolved_path: path.display().to_string(),
            format,
        })
    }

    pub fn list_output_devices(&self) -> Result<Vec<AudioDevice>, AudioError> {
        CpalOutputBackend.list_devices_blocking()
    }
}

pub struct SymphoniaDecoderBackend;

impl SymphoniaDecoderBackend {
    pub fn inspect_path(&self, path: &Path) -> Result<StreamFormat, AudioError> {
        if !path.exists() {
            return Err(AudioError::Io(format!(
                "source does not exist: {}",
                path.display()
            )));
        }
        let file = File::open(path).map_err(|e| AudioError::Io(e.to_string()))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|e| AudioError::Probe(e.to_string()))?;

        let format = probed.format;
        let track = format
            .default_track()
            .or_else(|| format.tracks().first())
            .ok_or_else(|| AudioError::UnsupportedFormat("no audio tracks found".to_string()))?;
        if track.codec_params.codec == CODEC_TYPE_NULL {
            return Err(AudioError::UnsupportedFormat(
                "container track codec type is unknown".to_string(),
            ));
        }

        let sample_rate = track.codec_params.sample_rate.ok_or_else(|| {
            AudioError::Probe("missing sample_rate in codec parameters".to_string())
        })?;
        let channels = track
            .codec_params
            .channels
            .map(|c| c.count() as u16)
            .unwrap_or(0);
        let bit_depth = track
            .codec_params
            .bits_per_sample
            .or(track.codec_params.bits_per_coded_sample)
            .unwrap_or(0) as u16;

        Ok(StreamFormat {
            sample_rate,
            channels,
            bit_depth,
        })
    }
}

#[async_trait]
impl DecoderBackend for SymphoniaDecoderBackend {
    async fn supports(&self, source_uri: &str) -> bool {
        parse_local_source_uri(source_uri).is_ok()
    }

    async fn inspect_format(&self, source_uri: &str) -> Result<StreamFormat, AudioError> {
        let path = parse_local_source_uri(source_uri)?;
        self.inspect_path(&path)
    }
}

pub struct CpalOutputBackend;

impl CpalOutputBackend {
    pub fn list_devices_blocking(&self) -> Result<Vec<AudioDevice>, AudioError> {
        let host = cpal::default_host();
        let default_device_id = host
            .default_output_device()
            .and_then(|d| d.id().ok())
            .map(|id| id.to_string());
        let devices = host
            .output_devices()
            .map_err(|e| AudioError::BackendUnavailable(e.to_string()))?;

        let mut out = Vec::new();
        for (idx, device) in devices.enumerate() {
            let device_id = device.id().ok().map(|id| id.to_string());
            let desc = device.description().ok();
            let name = desc
                .as_ref()
                .map(|d| d.name().to_string())
                .unwrap_or_else(|| format!("Unknown Output Device {idx}"));
            let is_default = default_device_id
                .as_deref()
                .map(|id| Some(id) == device_id.as_deref())
                .unwrap_or(false);
            out.push(AudioDevice {
                id: device_id.unwrap_or_else(|| format!("unknown:{idx}")),
                default_output: is_default,
                name,
            });
        }
        Ok(out)
    }
}

#[async_trait]
impl OutputBackend for CpalOutputBackend {
    async fn list_devices(&self) -> Result<Vec<AudioDevice>, AudioError> {
        self.list_devices_blocking()
    }

    async fn start(&self) -> Result<(), AudioError> {
        Err(AudioError::BackendUnavailable(
            "stream start is not implemented yet".to_string(),
        ))
    }
}

fn parse_local_source_uri(source_uri: &str) -> Result<PathBuf, AudioError> {
    let trimmed = source_uri.trim();
    if trimmed.is_empty() {
        return Err(AudioError::UnsupportedSourceUri(
            "empty source URI".to_string(),
        ));
    }

    let path = if let Some(rest) = trimmed.strip_prefix("file://") {
        if rest.is_empty() {
            return Err(AudioError::UnsupportedSourceUri(source_uri.to_string()));
        }
        #[cfg(windows)]
        let path_text = rest.strip_prefix('/').unwrap_or(rest);
        #[cfg(not(windows))]
        let path_text = rest;
        PathBuf::from(path_text)
    } else if trimmed.contains("://") {
        return Err(AudioError::UnsupportedSourceUri(trimmed.to_string()));
    } else {
        PathBuf::from(trimmed)
    };

    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(AudioError::UnsupportedSourceUri(format!(
            "path traversal not allowed: {trimmed}"
        )));
    }

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_local_source_uris() {
        assert_eq!(
            parse_local_source_uri("/tmp/example.flac").unwrap(),
            PathBuf::from("/tmp/example.flac")
        );
        assert_eq!(
            parse_local_source_uri("file:///tmp/example.flac").unwrap(),
            PathBuf::from("/tmp/example.flac")
        );
        assert!(matches!(
            parse_local_source_uri("https://example.com/stream"),
            Err(AudioError::UnsupportedSourceUri(_))
        ));
        assert!(matches!(
            parse_local_source_uri(""),
            Err(AudioError::UnsupportedSourceUri(_))
        ));
    }

    #[test]
    fn audio_engine_rejects_missing_files_on_inspect() {
        let engine = AudioEngine::new();
        let result = engine.inspect_source_uri("/definitely/missing/auric-audio-test.flac");
        assert!(matches!(result, Err(AudioError::Io(_))));
    }

    #[test]
    fn rejects_path_traversal_in_source_uri() {
        assert!(parse_local_source_uri("file:///music/../../../etc/passwd").is_err());
        assert!(parse_local_source_uri("/music/../../../etc/passwd").is_err());
        assert!(parse_local_source_uri("../../../etc/passwd").is_err());
    }
}
