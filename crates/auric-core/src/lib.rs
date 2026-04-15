use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FeatureId {
    Metadata,
    Artwork,
    RemoteMetadata,
    WatchedFolders,
    Equalizer,
    Visualizer,
    Analytics,
    P2PSync,
    P2PStream,
    Mouse,
    ImageArtwork,
}

impl FeatureId {
    pub const ALL: [FeatureId; 11] = [
        FeatureId::Metadata,
        FeatureId::Artwork,
        FeatureId::RemoteMetadata,
        FeatureId::WatchedFolders,
        FeatureId::Equalizer,
        FeatureId::Visualizer,
        FeatureId::Analytics,
        FeatureId::P2PSync,
        FeatureId::P2PStream,
        FeatureId::Mouse,
        FeatureId::ImageArtwork,
    ];

    pub fn as_key(self) -> &'static str {
        match self {
            FeatureId::Metadata => "metadata",
            FeatureId::Artwork => "artwork",
            FeatureId::RemoteMetadata => "remote_metadata",
            FeatureId::WatchedFolders => "watched_folders",
            FeatureId::Equalizer => "equalizer",
            FeatureId::Visualizer => "visualizer",
            FeatureId::Analytics => "analytics",
            FeatureId::P2PSync => "p2p_sync",
            FeatureId::P2PStream => "p2p_stream",
            FeatureId::Mouse => "mouse",
            FeatureId::ImageArtwork => "image_artwork",
        }
    }

    pub fn from_key(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "metadata" => Some(FeatureId::Metadata),
            "artwork" => Some(FeatureId::Artwork),
            "remote_metadata" => Some(FeatureId::RemoteMetadata),
            "watched_folders" => Some(FeatureId::WatchedFolders),
            "equalizer" => Some(FeatureId::Equalizer),
            "visualizer" => Some(FeatureId::Visualizer),
            "analytics" => Some(FeatureId::Analytics),
            "p2p_sync" => Some(FeatureId::P2PSync),
            "p2p_stream" => Some(FeatureId::P2PStream),
            "mouse" => Some(FeatureId::Mouse),
            "image_artwork" => Some(FeatureId::ImageArtwork),
            _ => None,
        }
    }
}

impl fmt::Display for FeatureId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_key())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeatureState {
    Disabled,
    Starting,
    Enabled,
    Degraded { reason: String },
    Stopping,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FeatureRegistry {
    pub states: BTreeMap<FeatureId, FeatureState>,
}

impl FeatureRegistry {
    pub fn with_defaults_enabled() -> Self {
        let mut states = BTreeMap::new();
        for feature in FeatureId::ALL {
            states.insert(feature, FeatureState::Enabled);
        }
        Self { states }
    }

    pub fn set_state(&mut self, feature: FeatureId, state: FeatureState) {
        self.states.insert(feature, state);
    }

    pub fn set_enabled(&mut self, feature: FeatureId, enabled: bool) {
        self.states.insert(
            feature,
            if enabled {
                FeatureState::Enabled
            } else {
                FeatureState::Disabled
            },
        );
    }

    pub fn state(&self, feature: FeatureId) -> FeatureState {
        self.states
            .get(&feature)
            .cloned()
            .unwrap_or(FeatureState::Disabled)
    }

    pub fn is_enabled(&self, feature: FeatureId) -> bool {
        matches!(self.state(feature), FeatureState::Enabled)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalCapabilities {
    pub mouse: bool,
    pub color_depth: ColorDepth,
    pub image_protocols: Vec<ImageProtocol>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ColorDepth {
    Basic16,
    Ansi256,
    TrueColor,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ImageProtocol {
    Kitty,
    Sixel,
    ITerm2Inline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TrackId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackStatus {
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    Off,
    One,
    All,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaybackSession {
    pub status: PlaybackStatus,
    pub current_index: Option<usize>,
    pub position_ms: u64,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: RepeatMode,
}

impl Default for PlaybackSession {
    fn default() -> Self {
        Self {
            status: PlaybackStatus::Stopped,
            current_index: None,
            position_ms: 0,
            volume: 1.0,
            shuffle: false,
            repeat: RepeatMode::Off,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaybackQueueEntry {
    pub track_id: TrackId,
    pub path: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<i64>,
    pub sample_rate: Option<i64>,
    pub channels: Option<i64>,
    pub bit_depth: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PlaybackState {
    pub session: PlaybackSession,
    pub queue: Vec<PlaybackQueueEntry>,
}

impl PlaybackState {
    pub fn current_entry(&self) -> Option<&PlaybackQueueEntry> {
        self.session
            .current_index
            .and_then(|idx| self.queue.get(idx))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AppCommand {
    ToggleFeature { feature: FeatureId, enabled: bool },
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    SeekMillis(u64),
    SetVolume(f32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AppEvent {
    FeatureStateChanged {
        feature: FeatureId,
        state: FeatureState,
    },
    PlaybackStateChanged {
        status: PlaybackStatus,
        current_index: Option<usize>,
        queue_len: usize,
    },
    PlaybackPositionMillis(u64),
    TrackChanged {
        track_id: Option<TrackId>,
    },
    Warning(String),
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_id_keys_round_trip() {
        for feature in FeatureId::ALL {
            let key = feature.as_key();
            assert_eq!(FeatureId::from_key(key), Some(feature));
        }
        assert_eq!(FeatureId::from_key("unknown"), None);
    }

    #[test]
    fn feature_registry_defaults_and_toggle() {
        let mut registry = FeatureRegistry::with_defaults_enabled();
        assert!(registry.is_enabled(FeatureId::Metadata));
        registry.set_enabled(FeatureId::Metadata, false);
        assert_eq!(registry.state(FeatureId::Metadata), FeatureState::Disabled);
    }

    #[test]
    fn playback_state_defaults_are_sensible() {
        let state = PlaybackState::default();
        assert!(state.queue.is_empty());
        assert_eq!(state.session.status, PlaybackStatus::Stopped);
        assert_eq!(state.session.current_index, None);
        assert_eq!(state.session.volume, 1.0);
        assert_eq!(state.session.repeat, RepeatMode::Off);
    }
}
