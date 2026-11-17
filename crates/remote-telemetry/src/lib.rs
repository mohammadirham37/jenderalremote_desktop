//! # remote-telemetry
//!
//! Structured logging and lightweight session metrics (rancangan teknis
//! §2, Observability). The engine never logs frame contents, clipboard
//! data, tokens, or private keys.

use std::sync::Arc;

pub mod metrics;

pub use metrics::{MetricsSnapshot, SessionMetrics};

/// Initializes the global tracing subscriber.
///
/// - `json`: machine-readable output for servers; human-readable for
///   desktop dev runs.
/// - Level comes from `RUST_LOG`, defaulting to `info`.
pub fn init_tracing(json: bool) {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    if json {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .with_target(true)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(true)
            .init();
    }
}

/// Shared, cloneable session metrics handle.
pub fn new_session_metrics() -> Arc<SessionMetrics> {
    Arc::new(SessionMetrics::default())
}
