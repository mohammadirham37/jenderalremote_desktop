//! Engine-wide error type.

use remote_capture::CaptureError;
use remote_core::StateMachineError;
use remote_input::InputError;
use remote_protocol::ProtocolError;
use remote_security::PolicyError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("signaling failure: {0}")]
    Signaling(String),

    #[error("signaling rejected: {code}: {message}")]
    SignalingRejected { code: String, message: String },

    #[error("webrtc failure: {0}")]
    WebRtc(String),

    #[error("capture failure: {0}")]
    Capture(#[from] CaptureError),

    #[error("input failure: {0}")]
    Input(#[from] InputError),

    #[error("protocol error: {0}")]
    Protocol(#[from] ProtocolError),

    #[error("session state error: {0}")]
    State(#[from] StateMachineError),

    #[error("policy denied: {0}")]
    Policy(#[from] PolicyError),

    #[error("engine task failed: {0}")]
    Task(String),

    #[error("connection closed")]
    Closed,
}
