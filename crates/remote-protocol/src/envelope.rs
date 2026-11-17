//! Signaling envelope shared by clients and the signaling server.
//!
//! Every message follows the envelope from rancangan teknis §7.1:
//!
//! ```json
//! {
//!   "version": 1,
//!   "type": "session.request",
//!   "request_id": "req_...",
//!   "session_id": "sess_...",
//!   "payload": { ... }
//! }
//! ```

use crate::error::ProtocolError;
use crate::ids::{RequestId, SessionId};
use crate::messages::{
    DeviceHeartbeat, DeviceRegister, DeviceRegistered, DeviceRemove, ErrorPayload,
    IceCandidateJson, RtcAnswer, RtcOffer, SessionDecision, SessionEnd, SessionRequest,
    SessionRevoke, SessionStateChange,
};
use crate::version::{MAX_MESSAGE_SIZE, PROTOCOL_VERSION, is_supported};
use serde::{Deserialize, Serialize};

/// Message type tag carried in the envelope `type` field (§7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    DeviceRegister,
    DeviceHeartbeat,
    DeviceRemove,
    SessionRequest,
    SessionApprove,
    SessionReject,
    SessionRevoke,
    SessionState,
    SessionEnd,
    RtcOffer,
    RtcAnswer,
    RtcIceCandidate,
    DeviceRegistered,
    #[serde(rename = "error")]
    ServerError,
}

impl MessageType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageType::DeviceRegister => "device.register",
            MessageType::DeviceHeartbeat => "device.heartbeat",
            MessageType::DeviceRemove => "device.remove",
            MessageType::SessionRequest => "session.request",
            MessageType::SessionApprove => "session.approve",
            MessageType::SessionReject => "session.reject",
            MessageType::SessionRevoke => "session.revoke",
            MessageType::SessionState => "session.state",
            MessageType::SessionEnd => "session.end",
            MessageType::RtcOffer => "rtc.offer",
            MessageType::RtcAnswer => "rtc.answer",
            MessageType::RtcIceCandidate => "rtc.ice_candidate",
            MessageType::DeviceRegistered => "device.registered",
            MessageType::ServerError => "error",
        }
    }

    pub fn from_str_raw(value: &str) -> Option<Self> {
        Some(match value {
            "device.register" => MessageType::DeviceRegister,
            "device.heartbeat" => MessageType::DeviceHeartbeat,
            "device.remove" => MessageType::DeviceRemove,
            "session.request" => MessageType::SessionRequest,
            "session.approve" => MessageType::SessionApprove,
            "session.reject" => MessageType::SessionReject,
            "session.revoke" => MessageType::SessionRevoke,
            "session.state" => MessageType::SessionState,
            "session.end" => MessageType::SessionEnd,
            "rtc.offer" => MessageType::RtcOffer,
            "rtc.answer" => MessageType::RtcAnswer,
            "rtc.ice_candidate" => MessageType::RtcIceCandidate,
            "device.registered" => MessageType::DeviceRegistered,
            "error" => MessageType::ServerError,
            _ => return None,
        })
    }
}

/// Typed payload variants, one per message type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Payload {
    DeviceRegister(Box<DeviceRegister>),
    DeviceHeartbeat(DeviceHeartbeat),
    DeviceRemove(DeviceRemove),
    SessionRequest(Box<SessionRequest>),
    SessionDecision(SessionDecision),
    SessionRevoke(SessionRevoke),
    SessionState(SessionStateChange),
    SessionEnd(SessionEnd),
    RtcOffer(RtcOffer),
    RtcAnswer(RtcAnswer),
    IceCandidate(IceCandidateJson),
    DeviceRegistered(Box<DeviceRegistered>),
    ServerError(ErrorPayload),
}

/// The signaling envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub version: u32,
    #[serde(rename = "type")]
    pub kind: String,
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    pub payload: serde_json::Value,
}

impl Envelope {
    /// Builds a new envelope with the current protocol version and a fresh
    /// request id.
    pub fn new(kind: MessageType, request_id: RequestId, payload: impl Serialize) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            kind: kind.as_str().to_string(),
            request_id: request_id.into_string(),
            session_id: None,
            payload: serde_json::to_value(payload).expect("payload must be JSON-serializable"),
        }
    }

    pub fn with_session(mut self, session_id: SessionId) -> Self {
        self.session_id = Some(session_id);
        self
    }

    pub fn message_type(&self) -> Result<MessageType, ProtocolError> {
        MessageType::from_str_raw(&self.kind)
            .ok_or_else(|| ProtocolError::UnknownMessageType(self.kind.clone()))
    }

    /// Serializes the envelope to a JSON string.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("envelope is JSON-serializable")
    }

    /// Parses and validates an envelope from raw bytes.
    ///
    /// Validation order: size limit, version support, request id presence,
    /// then payload typing via [`Envelope::payload_typed`].
    pub fn from_bytes(data: &[u8]) -> Result<Self, ProtocolError> {
        if data.len() > MAX_MESSAGE_SIZE {
            return Err(ProtocolError::MessageTooLarge {
                limit: MAX_MESSAGE_SIZE,
            });
        }
        let envelope: Envelope = serde_json::from_slice(data)?;
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if !is_supported(self.version) {
            return Err(ProtocolError::UnsupportedVersion {
                version: self.version,
                expected: PROTOCOL_VERSION,
            });
        }
        if self.request_id.trim().is_empty() {
            return Err(ProtocolError::MissingRequestId);
        }
        // Ensure the type is known even if the caller never asks for it.
        self.message_type()?;
        Ok(())
    }

    /// Decodes the payload into the typed variant matching `self.kind`.
    pub fn payload_typed(&self) -> Result<Payload, ProtocolError> {
        let mismatch = |reason: String| ProtocolError::PayloadMismatch {
            kind: self.kind.clone(),
            reason,
        };
        Ok(match self.message_type()? {
            MessageType::DeviceRegister => Payload::DeviceRegister(Box::new(
                serde_json::from_value(self.payload.clone())
                    .map_err(|e| mismatch(e.to_string()))?,
            )),
            MessageType::DeviceHeartbeat => Payload::DeviceHeartbeat(
                serde_json::from_value(self.payload.clone())
                    .map_err(|e| mismatch(e.to_string()))?,
            ),
            MessageType::DeviceRemove => Payload::DeviceRemove(
                serde_json::from_value(self.payload.clone())
                    .map_err(|e| mismatch(e.to_string()))?,
            ),
            MessageType::SessionRequest => Payload::SessionRequest(Box::new(
                serde_json::from_value(self.payload.clone())
                    .map_err(|e| mismatch(e.to_string()))?,
            )),
            MessageType::SessionApprove | MessageType::SessionReject => Payload::SessionDecision(
                serde_json::from_value(self.payload.clone())
                    .map_err(|e| mismatch(e.to_string()))?,
            ),
            MessageType::SessionRevoke => Payload::SessionRevoke(
                serde_json::from_value(self.payload.clone())
                    .map_err(|e| mismatch(e.to_string()))?,
            ),
            MessageType::SessionState => Payload::SessionState(
                serde_json::from_value(self.payload.clone())
                    .map_err(|e| mismatch(e.to_string()))?,
            ),
            MessageType::SessionEnd => Payload::SessionEnd(
                serde_json::from_value(self.payload.clone())
                    .map_err(|e| mismatch(e.to_string()))?,
            ),
            MessageType::RtcOffer => Payload::RtcOffer(
                serde_json::from_value(self.payload.clone())
                    .map_err(|e| mismatch(e.to_string()))?,
            ),
            MessageType::RtcAnswer => Payload::RtcAnswer(
                serde_json::from_value(self.payload.clone())
                    .map_err(|e| mismatch(e.to_string()))?,
            ),
            MessageType::RtcIceCandidate => Payload::IceCandidate(
                serde_json::from_value(self.payload.clone())
                    .map_err(|e| mismatch(e.to_string()))?,
            ),
            MessageType::DeviceRegistered => Payload::DeviceRegistered(Box::new(
                serde_json::from_value(self.payload.clone())
                    .map_err(|e| mismatch(e.to_string()))?,
            )),
            MessageType::ServerError => Payload::ServerError(
                serde_json::from_value(self.payload.clone())
                    .map_err(|e| mismatch(e.to_string()))?,
            ),
        })
    }
}

/// Generates a fresh request id (`req_` + random hex).
pub fn new_request_id() -> RequestId {
    use rand::RngCore;
    let mut bytes = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut bytes);
    RequestId::new(format!("req_{}", hex::encode(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permission::{AccessMode, PermissionSet};

    fn sample_request() -> Envelope {
        let mut perms = PermissionSet::empty();
        perms.grant(crate::permission::Permission::ViewScreen);
        perms.grant(crate::permission::Permission::ControlInput);
        let payload = SessionRequest {
            target_device_id: crate::ids::DeviceId::new("device_example"),
            controller_device_id: None,
            mode: AccessMode::Attended,
            permissions: perms,
        };
        Envelope::new(MessageType::SessionRequest, new_request_id(), payload)
    }

    #[test]
    fn roundtrip() {
        let env = sample_request();
        let raw = env.to_json();
        let parsed = Envelope::from_bytes(raw.as_bytes()).unwrap();
        assert_eq!(parsed.kind, "session.request");
        assert_eq!(parsed.version, PROTOCOL_VERSION);
        match parsed.payload_typed().unwrap() {
            Payload::SessionRequest(req) => {
                assert_eq!(req.target_device_id.as_str(), "device_example");
            }
            other => panic!("unexpected payload: {other:?}"),
        }
    }

    #[test]
    fn rejects_oversized_message() {
        let big = vec![b'x'; MAX_MESSAGE_SIZE + 1];
        assert!(matches!(
            Envelope::from_bytes(&big),
            Err(ProtocolError::MessageTooLarge { .. })
        ));
    }

    #[test]
    fn rejects_unknown_version() {
        let mut env = sample_request();
        env.version = 99;
        let raw = serde_json::to_string(&env).unwrap();
        assert!(matches!(
            Envelope::from_bytes(raw.as_bytes()),
            Err(ProtocolError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn rejects_unknown_type() {
        let raw = serde_json::json!({
            "version": 1,
            "type": "totally.unknown",
            "request_id": "req_1",
            "payload": {}
        })
        .to_string();
        assert!(matches!(
            Envelope::from_bytes(raw.as_bytes()),
            Err(ProtocolError::UnknownMessageType(_))
        ));
    }

    #[test]
    fn rejects_empty_request_id() {
        let raw = serde_json::json!({
            "version": 1,
            "type": "session.end",
            "request_id": "  ",
            "payload": {}
        })
        .to_string();
        assert!(matches!(
            Envelope::from_bytes(raw.as_bytes()),
            Err(ProtocolError::MissingRequestId)
        ));
    }
}
