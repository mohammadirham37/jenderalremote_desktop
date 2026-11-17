//! Application state: device identity (OS keychain), engine handles, and
//! the frontend event DTOs.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use remote_protocol::{AccessMode, PermissionSet};
use remote_security::{DeviceIdentity, FileSecretStore, KeyringStore, SecretStore};
use remote_telemetry::MetricsSnapshot;
use remote_webrtc::{ControllerCmd, ControllerEvent, HostCmd, HostEvent};
use std::sync::Mutex;

const SERVICE: &str = "jenderalremote.desktop";
const IDENTITY_ACCOUNT: &str = "device-identity";
const TOKEN_ACCOUNT: &str = "device-token";

/// A running engine: command sender plus the pump task that forwards
/// engine events to the webview.
pub struct EngineSlot<Cmd> {
    pub cmd: tokio::sync::mpsc::UnboundedSender<Cmd>,
    pub pump: tokio::task::JoinHandle<()>,
}

#[derive(Default)]
pub struct AppState {
    pub identity: Mutex<Option<DeviceIdentity>>,
    pub host: Mutex<Option<EngineSlot<HostCmd>>>,
    pub controller: Mutex<Option<EngineSlot<ControllerCmd>>>,
}

/// Loads (or creates) the device identity. Prefers the OS keychain and
/// falls back to an app-local file store when no keychain is available
/// (headless Linux sessions, CI).
pub fn load_or_create_identity(app_dir: &std::path::Path) -> Result<DeviceIdentity, String> {
    let file_store = FileSecretStore::new(app_dir.join("secretstore"));

    let keyring = KeyringStore::new(SERVICE, IDENTITY_ACCOUNT);
    let identity = match keyring.load_identity() {
        Ok(Some(identity)) => identity,
        Ok(None) => {
            // Try the file fallback before generating a new one.
            match file_store.load_identity() {
                Ok(Some(identity)) => identity,
                Ok(None) => {
                    let identity = DeviceIdentity::generate();
                    identity
                        .save_to(&keyring as &dyn SecretStore)
                        .or_else(|_| identity.save_to(&file_store as &dyn SecretStore))
                        .map_err(|e| format!("menyimpan identitas perangkat gagal: {e}"))?;
                    identity
                }
                Err(e) => return Err(format!("membaca identitas gagal: {e}")),
            }
        }
        Err(e) => {
            tracing::warn!(%e, "keychain tidak tersedia, memakai file store");
            match file_store
                .load_identity()
                .map_err(|e| format!("membaca identitas gagal: {e}"))?
            {
                Some(identity) => identity,
                None => {
                    let identity = DeviceIdentity::generate();
                    identity
                        .save_to(&file_store as &dyn SecretStore)
                        .map_err(|e| format!("menyimpan identitas perangkat gagal: {e}"))?;
                    identity
                }
            }
        }
    };
    Ok(identity)
}

/// Device token persistence (OS keychain; never localStorage — §11.1).
pub fn load_device_token() -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(SERVICE, TOKEN_ACCOUNT).map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn save_device_token(token: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, TOKEN_ACCOUNT).map_err(|e| e.to_string())?;
    entry.set_password(token).map_err(|e| e.to_string())
}

/// Host event → frontend payload.
#[derive(serde::Serialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostEventDto {
    TokenIssued {
        token: String,
    },
    SessionRequest {
        session_id: String,
        controller_device_id: String,
        mode: AccessMode,
        permissions: Vec<String>,
    },
    StateChanged {
        status: String,
        detail: Option<String>,
    },
    Stats {
        snapshot: MetricsSnapshot,
    },
    SessionEnded {
        reason: Option<String>,
    },
    Error {
        message: String,
    },
}

impl From<HostEvent> for HostEventDto {
    fn from(event: HostEvent) -> Self {
        match event {
            HostEvent::TokenIssued(token) => HostEventDto::TokenIssued { token },
            HostEvent::SessionRequest {
                session_id,
                controller_device_id,
                mode,
                permissions,
            } => HostEventDto::SessionRequest {
                session_id: session_id.into_string(),
                controller_device_id: controller_device_id.into_string(),
                mode,
                permissions: permissions.iter().map(|p| p.as_str().to_string()).collect(),
            },
            HostEvent::StateChanged { status, detail } => {
                HostEventDto::StateChanged { status, detail }
            }
            HostEvent::Stats(snapshot) => HostEventDto::Stats { snapshot },
            HostEvent::SessionEnded { reason } => HostEventDto::SessionEnded { reason },
            HostEvent::Error(message) => HostEventDto::Error { message },
        }
    }
}

/// Controller event → frontend payload (JPEG frames become base64).
#[derive(serde::Serialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ControllerEventDto {
    TokenIssued {
        token: String,
    },
    StateChanged {
        status: String,
        detail: Option<String>,
    },
    Frame {
        frame_id: u32,
        width: u16,
        height: u16,
        jpeg_base64: String,
    },
    Stats {
        host_fps: f32,
        host_encode_ms: f32,
        host_capture_ms: f32,
        host_frames_skipped: u64,
        rtt_ms: u32,
    },
    Ended {
        reason: Option<String>,
    },
    Error {
        message: String,
    },
}

impl From<ControllerEvent> for ControllerEventDto {
    fn from(event: ControllerEvent) -> Self {
        match event {
            ControllerEvent::TokenIssued(token) => ControllerEventDto::TokenIssued { token },
            ControllerEvent::StateChanged { status, detail } => {
                ControllerEventDto::StateChanged { status, detail }
            }
            ControllerEvent::Frame {
                frame_id,
                width,
                height,
                jpeg,
            } => ControllerEventDto::Frame {
                frame_id,
                width,
                height,
                jpeg_base64: STANDARD.encode(jpeg.as_slice()),
            },
            ControllerEvent::Stats { host, rtt_ms } => ControllerEventDto::Stats {
                host_fps: host.fps,
                host_encode_ms: host.encode_ms,
                host_capture_ms: host.capture_ms,
                host_frames_skipped: host.frames_skipped,
                rtt_ms,
            },
            ControllerEvent::Ended { reason } => ControllerEventDto::Ended { reason },
            ControllerEvent::Error(message) => ControllerEventDto::Error { message },
        }
    }
}

/// Parses a permission list from the UI (never trusted for authorization
/// by itself — the engine re-validates).
pub fn parse_permissions(list: &[String]) -> PermissionSet {
    let mut set = PermissionSet::empty();
    for name in list {
        match name.as_str() {
            "view_screen" => set.grant(remote_protocol::Permission::ViewScreen),
            "control_input" => set.grant(remote_protocol::Permission::ControlInput),
            "clipboard" => set.grant(remote_protocol::Permission::Clipboard),
            "audio" => set.grant(remote_protocol::Permission::Audio),
            _ => {}
        }
    }
    set
}
