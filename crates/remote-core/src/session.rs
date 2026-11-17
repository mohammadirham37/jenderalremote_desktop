//! Session record and status vocabulary (rancangan teknis §5.3, §10.4).

use remote_protocol::{AccessMode, DeviceId, PermissionSet, SessionId};
use serde::{Deserialize, Serialize};

/// Lifecycle states of a remote session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Idle,
    Requesting,
    AwaitingApproval,
    Negotiating,
    Connecting,
    Active,
    Ending,
    Ended,
    Rejected,
    Failed,
    Revoked,
    Disconnected,
    Reconnecting,
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl SessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionStatus::Idle => "idle",
            SessionStatus::Requesting => "requesting",
            SessionStatus::AwaitingApproval => "awaiting_approval",
            SessionStatus::Negotiating => "negotiating",
            SessionStatus::Connecting => "connecting",
            SessionStatus::Active => "active",
            SessionStatus::Ending => "ending",
            SessionStatus::Ended => "ended",
            SessionStatus::Rejected => "rejected",
            SessionStatus::Failed => "failed",
            SessionStatus::Revoked => "revoked",
            SessionStatus::Disconnected => "disconnected",
            SessionStatus::Reconnecting => "reconnecting",
        }
    }

    /// Terminal states never transition again.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            SessionStatus::Ended
                | SessionStatus::Rejected
                | SessionStatus::Failed
                | SessionStatus::Revoked
        )
    }

    /// A session is "live" once media may flow.
    pub fn is_live(&self) -> bool {
        matches!(self, SessionStatus::Active)
    }
}

/// Domain view of a session, mirrored by the signaling server record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: SessionId,
    pub controller_device_id: DeviceId,
    pub host_device_id: DeviceId,
    pub mode: AccessMode,
    pub granted_permissions: PermissionSet,
    pub status: SessionStatus,
    pub created_at_ms: u64,
}
