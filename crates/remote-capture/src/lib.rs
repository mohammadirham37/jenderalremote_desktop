//! # remote-capture
//!
//! Screen capture interface (rancangan teknis §5.1, §5.2).
//!
//! Backends:
//! - [`StubCapture`]: deterministic pattern generator for tests, CI, and
//!   headless development. Always available.
//! - `XcapCapture` (feature `xcap`): real capture using native OS APIs
//!   (Windows Graphics Capture / CoreGraphics / X11) through the `xcap`
//!   crate. Platform-specific crates (`capture-macos` with
//!   ScreenCaptureKit, `capture-linux` with PipeWire portal) replace it
//!   in later phases — the interface stays stable.

pub mod frame;
pub mod stub;

#[cfg(feature = "xcap")]
pub mod xcap_impl;

#[cfg(feature = "xcap")]
pub use xcap_impl::XcapCapture;

pub use frame::{CapturedFrame, FrameFormat};
pub use stub::StubCapture;

use async_trait::async_trait;
use std::sync::Arc;
use thiserror::Error;

/// Errors surfaced by capture backends. Permission failures must be
/// *explained* to the user, never bypassed (§2, prinsip produk).
#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("capture backend unsupported on this platform/configuration: {0}")]
    Unsupported(String),

    #[error("screen capture permission denied by the OS: {detail}. {guidance}")]
    PermissionDenied {
        detail: String,
        guidance: &'static str,
    },

    #[error("capture not in a valid state for this operation: {0}")]
    InvalidState(String),

    #[error("capture I/O failure: {0}")]
    Io(String),
}

impl CaptureError {
    /// Guidance shown to the user for each platform when permission is
    /// missing (macOS Screen Recording, etc.).
    pub fn permission_guidance() -> &'static str {
        if cfg!(target_os = "macos") {
            "Buka System Settings > Privacy & Security > Screen Recording dan izinkan aplikasi ini, lalu jalankan ulang."
        } else if cfg!(target_os = "windows") {
            "Pastikan aplikasi berjalan dalam sesi pengguna interaktif dengan akses desktop."
        } else {
            "Pastikan sesi display aktif (X11) atau gunakan backend portal PipeWire untuk Wayland."
        }
    }
}

/// Static description of a capture target (monitor).
#[derive(Debug, Clone)]
pub struct MonitorInfo {
    pub id: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
}

/// Backend configuration for starting a capture stream.
#[derive(Debug, Clone)]
pub struct CaptureConfig {
    /// Monitor index (0 = primary when available).
    pub monitor_index: usize,
    /// Target frames per second; backends may deliver slower.
    pub target_fps: f32,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            monitor_index: 0,
            target_fps: 15.0,
        }
    }
}

/// Platform-independent screen capture contract (§5.2).
#[async_trait]
pub trait ScreenCapture: Send {
    /// Prepares the capture pipeline. Must be called before `next_frame`.
    async fn start(&mut self, config: CaptureConfig) -> Result<(), CaptureError>;

    /// Waits for and returns the next frame. Backpressure policy: the
    /// caller owns pacing; backends should not queue unboundedly.
    async fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError>;

    /// Releases capture resources.
    async fn stop(&mut self) -> Result<(), CaptureError>;

    /// Human-readable backend name for telemetry.
    fn backend_name(&self) -> &'static str;
}

/// Creates the best available capture backend:
/// `xcap` native when the feature is enabled, stub otherwise.
pub fn default_capture() -> Box<dyn ScreenCapture> {
    #[cfg(feature = "xcap")]
    {
        Box::new(XcapCapture::new())
    }
    #[cfg(not(feature = "xcap"))]
    {
        Box::new(StubCapture::new(1280, 720))
    }
}

/// Lists monitors via the active backend (stub reports a single virtual
/// monitor).
pub async fn list_monitors() -> Result<Vec<MonitorInfo>, CaptureError> {
    #[cfg(feature = "xcap")]
    {
        xcap_impl::list_monitors().await
    }
    #[cfg(not(feature = "xcap"))]
    {
        Ok(vec![MonitorInfo {
            id: 0,
            name: "Stub Display".into(),
            width: 1280,
            height: 720,
            is_primary: true,
        }])
    }
}

/// Re-exported so callers can hold frames cheaply.
pub type SharedFrame = Arc<CapturedFrame>;
