//! Typed payloads for every signaling message (rancangan teknis §7.2).

use crate::ids::DeviceId;
use crate::permission::{AccessMode, PermissionSet};
use serde::{Deserialize, Serialize};

/// `device.register` — register (or refresh) a device identity.
///
/// The `device_token` returned by the server must be stored in the OS
/// credential vault, never in the database row or logs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceRegister {
    pub device_id: DeviceId,
    pub display_name: String,
    pub platform: String,
    /// Base64 (SPKI) public key of the device identity key pair.
    pub public_key: String,
}

/// `device.heartbeat` — presence ping. Sent periodically over the
/// signaling WebSocket. Identity is taken from the authenticated
/// connection, not from this payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceHeartbeat {
    /// Coarse status, e.g. `online`, `busy`.
    pub status: String,
}

/// `device.remove` — revoke a trusted device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceRemove {
    pub device_id: DeviceId,
}

/// `device.registered` — server acknowledgement of `device.register`.
///
/// The device token is delivered exactly once over TLS and must be
/// stored in the OS credential vault by the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceRegistered {
    pub device_id: DeviceId,
    pub device_token: String,
}

/// `session.request` — controller asks the host for a session.
///
/// `controller_device_id` is filled in by the server from the
/// authenticated connection. A client-supplied identity field must never
/// be trusted (§7.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRequest {
    pub target_device_id: DeviceId,
    /// Server-side: identity of the requesting controller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller_device_id: Option<DeviceId>,
    pub mode: AccessMode,
    pub permissions: PermissionSet,
}

/// `session.approve` / `session.reject` — host decision.
///
/// The host may narrow the requested permissions; only granted ones
/// become active.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDecision {
    pub approved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub granted_permissions: PermissionSet,
}

/// `session.revoke` — revoke a running session or an unattended grant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRevoke {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `session.state` — status change notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStateChange {
    /// Serialized `SessionStatus` name (e.g. `awaiting_approval`).
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// `session.end` — graceful termination from either side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEnd {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `rtc.offer` — SDP offer from the host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtcOffer {
    pub sdp: String,
}

/// `rtc.answer` — SDP answer from the controller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtcAnswer {
    pub sdp: String,
}

/// `rtc.ice_candidate` — trickle ICE candidate, JSON-encoded as produced
/// by the WebRTC stack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IceCandidateJson {
    pub candidate: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdp_mid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdp_mline_index: Option<u16>,
}

/// `error` — server-side rejection with a machine-readable code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
}
