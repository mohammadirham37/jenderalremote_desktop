//! # remote-webrtc
//!
//! WebRTC transport binding the engine together (rancangan teknis §5.1
//! `remote-webrtc`, §7.3 media/DataChannel policy).
//!
//! This crate handles transport only — never permission decisions. Every
//! input event arriving at the host passes through the `remote-core`
//! state machine + `remote-security` policy gate before it reaches a
//! `remote-input` backend.
//!
//! MVP media path: video frames travel as JPEG chunks over an unreliable
//! SCTP DataChannel; input travels as JSON over a reliable ordered
//! `control` DataChannel (see `docs/adr/0001`).

pub mod config;
pub mod controller;
pub mod encoder;
pub mod error;
pub mod host;
pub mod peer;
pub mod signaling;

pub use config::{EngineConfig, IceServerDef};
pub use controller::{ControllerCmd, ControllerEngine, ControllerEvent, ControllerHandle};
pub use encoder::encode_jpeg;
pub use error::EngineError;
pub use host::{HostCmd, HostEngine, HostEvent, HostHandle};
