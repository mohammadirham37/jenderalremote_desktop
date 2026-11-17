//! # remote-input
//!
//! Input control interface (rancangan teknis §5.1, §5.2).
//!
//! Backends:
//! - [`StubInput`]: records events without touching the OS — used by
//!   tests and the controller side.
//! - `EnigoInput` (feature `enigo`): native injection — SendInput on
//!   Windows, CGEvent on macOS (requires Accessibility permission),
//!   XTest on Linux.
//!
//! Input events themselves are defined in `remote-protocol::media` so
//! both engine sides share one wire format.

pub mod keys;

#[cfg(feature = "enigo")]
pub mod enigo_impl;

#[cfg(feature = "enigo")]
pub use enigo_impl::EnigoInput;

pub use keys::{EnigoKey, key_from_browser};
pub use stub::StubInput;

use async_trait::async_trait;
use remote_protocol::{InputEvent, KeyboardEvent, MouseEvent};
use thiserror::Error;

/// Errors surfaced by input backends.
#[derive(Debug, Error)]
pub enum InputError {
    #[error("input backend unsupported on this platform/configuration: {0}")]
    Unsupported(String),

    #[error("input permission denied by the OS (Accessibility/portal): {detail}. {guidance}")]
    PermissionDenied {
        detail: String,
        guidance: &'static str,
    },

    #[error("input backend failure: {0}")]
    Backend(String),

    #[error("input not allowed in the current session state")]
    NotAuthorized,
}

impl InputError {
    pub fn permission_guidance() -> &'static str {
        if cfg!(target_os = "macos") {
            "Buka System Settings > Privacy & Security > Accessibility dan izinkan aplikasi ini, lalu jalankan ulang."
        } else if cfg!(target_os = "linux") {
            "Untuk Wayland, compositor harus mendukung portal RemoteDesktop; untuk X11 pastikan XTest tersedia."
        } else {
            "Pastikan aplikasi berjalan dalam sesi pengguna interaktif."
        }
    }
}

/// Platform-independent input injection contract (§5.2).
///
/// The caller (host engine) MUST check session policy before invoking:
/// input is only allowed while the session is Active and
/// `control_input` was granted (rancangan teknis §5.3, §8.3).
#[async_trait]
pub trait InputController: Send + Sync {
    /// Injects a normalized mouse event.
    async fn send_mouse(&self, event: MouseEvent) -> Result<(), InputError>;

    /// Injects a normalized keyboard event.
    async fn send_keyboard(&self, event: KeyboardEvent) -> Result<(), InputError>;

    /// Releases all currently-pressed modifiers/keys. Called on
    /// disconnect so no key stays stuck (§7.3).
    async fn release_all(&self) -> Result<(), InputError>;

    /// Human-readable backend name for telemetry.
    fn backend_name(&self) -> &'static str;
}

/// Convenience wrapper used by the host engine after the policy gate.
pub async fn apply_event(
    backend: &dyn InputController,
    event: &InputEvent,
) -> Result<(), InputError> {
    match event {
        InputEvent::Mouse(mouse) => backend.send_mouse(*mouse).await,
        InputEvent::Keyboard(key) => backend.send_keyboard(key.clone()).await,
    }
}

/// Creates the best available input backend for the host side.
pub fn default_input() -> Box<dyn InputController> {
    #[cfg(feature = "enigo")]
    {
        Box::new(EnigoInput::new())
    }
    #[cfg(not(feature = "enigo"))]
    {
        Box::new(StubInput::default())
    }
}

mod stub;
