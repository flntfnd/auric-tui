use async_trait::async_trait;
use auric_core::TrackId;

#[derive(Debug, Clone)]
pub struct SessionId(pub String);

#[derive(Debug, Clone)]
pub struct ListenAlongState {
    pub track_id: Option<TrackId>,
    pub position_ms: u64,
    pub playing: bool,
}

#[derive(Debug, Clone)]
pub struct EncodedAudioPacket {
    pub seq: u64,
    pub timestamp_ms: u64,
    pub payload: Vec<u8>,
}

#[async_trait]
pub trait SessionService: Send + Sync {
    async fn create_session(&self) -> Result<SessionId, NetError>;
    async fn join_session(&self, session_id: &SessionId) -> Result<(), NetError>;
    async fn leave_session(&self) -> Result<(), NetError>;
}

#[async_trait]
pub trait ListenAlongSync: Send + Sync {
    async fn publish_state(&self, state: ListenAlongState) -> Result<(), NetError>;
    async fn subscribe(&self) -> Result<(), NetError>;
}

#[async_trait]
pub trait StreamTransport: Send + Sync {
    async fn send_packet(&self, packet: EncodedAudioPacket) -> Result<(), NetError>;
    async fn receive_loop(&self) -> Result<(), NetError>;
}

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("signaling error: {0}")]
    Signaling(String),
    #[error("transport error: {0}")]
    Transport(String),
    #[error("session error: {0}")]
    Session(String),
}
