//! # remote-core
//!
//! Session lifecycle and domain types shared by the host and controller
//! engines. This crate is UI-framework agnostic (no Tauri) and
//! OS-independent; it depends only on `remote-protocol`.

pub mod session;
pub mod state_machine;

pub use session::{SessionRecord, SessionStatus};
pub use state_machine::{SessionEvent, SessionStateMachine, StateMachineError};

#[cfg(test)]
mod tests;
