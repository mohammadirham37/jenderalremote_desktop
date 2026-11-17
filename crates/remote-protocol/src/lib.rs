//! # remote-protocol
//!
//! Message types, protocol versioning, and media-plane framing shared by the
//! host engine, the controller engine, and the signaling server.
//!
//! This crate must stay OS-independent (see `docs/architecture.md`,
//! dependency rules): it must not depend on Tokio, Tauri, or any platform
//! crate.

pub mod envelope;
pub mod error;
pub mod ids;
pub mod media;
pub mod messages;
pub mod permission;
pub mod version;

pub use envelope::{Envelope, MessageType, Payload, new_request_id};
pub use error::ProtocolError;
pub use ids::{DeviceId, RequestId, SessionId};
pub use media::{
    ControlFrame, FrameAssembler, HostStats, InputEvent, KeyAction, KeyboardEvent,
    MAX_CHUNK_PAYLOAD, Modifier, MouseAction, MouseButton, MouseEvent, VideoChunk, chunk_frame,
};
pub use messages::{
    DeviceHeartbeat, DeviceRegister, DeviceRegistered, DeviceRemove, ErrorPayload,
    IceCandidateJson, RtcAnswer, RtcOffer, SessionDecision, SessionEnd, SessionRequest,
    SessionRevoke, SessionStateChange,
};
pub use permission::{AccessMode, Permission, PermissionSet, validate_request};
pub use version::{MAX_MESSAGE_SIZE, PROTOCOL_VERSION};
