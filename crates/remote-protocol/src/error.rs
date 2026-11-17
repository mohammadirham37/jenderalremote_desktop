//! Protocol errors.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("message exceeds maximum size of {limit} bytes")]
    MessageTooLarge { limit: usize },

    #[error("unsupported protocol version {version} (expected {expected})")]
    UnsupportedVersion { version: u32, expected: u32 },

    #[error("unknown message type `{0}`")]
    UnknownMessageType(String),

    #[error("invalid request_id: must be non-empty")]
    MissingRequestId,

    #[error("payload does not match message type `{kind}`: {reason}")]
    PayloadMismatch { kind: String, reason: String },

    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
}
