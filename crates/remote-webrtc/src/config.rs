//! Engine configuration (signaling URL, ICE/TURN servers, video tuning).

use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::configuration::RTCConfiguration;

#[derive(Debug, Clone)]
pub struct IceServerDef {
    pub urls: Vec<String>,
    pub username: Option<String>,
    pub credential: Option<String>,
}

impl IceServerDef {
    pub fn stun(urls: impl Into<String>) -> Self {
        Self {
            urls: vec![urls.into()],
            username: None,
            credential: None,
        }
    }

    pub fn turn(urls: impl Into<String>, username: String, credential: String) -> Self {
        Self {
            urls: vec![urls.into()],
            username: Some(username),
            credential: Some(credential),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Signaling WebSocket URL (`ws://` dev / `wss://` production).
    pub signaling_url: String,
    pub ice_servers: Vec<IceServerDef>,
    /// JPEG quality (1-100).
    pub video_quality: u8,
    /// Capture frame rate target.
    pub target_fps: f32,
    /// Signaling heartbeat interval in seconds.
    pub heartbeat_secs: u64,
    /// Skip a frame when the video channel buffer exceeds this (bytes).
    pub backpressure_bytes: usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            signaling_url: "ws://127.0.0.1:8080/ws".to_string(),
            ice_servers: vec![IceServerDef::stun("stun:stun.l.google.com:19302")],
            video_quality: 60,
            target_fps: 15.0,
            heartbeat_secs: 30,
            backpressure_bytes: 2 * 1024 * 1024,
        }
    }
}

impl EngineConfig {
    pub fn rtc_configuration(&self) -> RTCConfiguration {
        RTCConfiguration {
            ice_servers: self
                .ice_servers
                .iter()
                .map(|s| RTCIceServer {
                    urls: s.urls.clone(),
                    username: s.username.clone().unwrap_or_default(),
                    credential: s.credential.clone().unwrap_or_default(),
                })
                .collect(),
            ..Default::default()
        }
    }
}
