//! # jenderalremote-signaling
//!
//! Control-plane server (rancangan teknis §3.1): device registry,
//! presence, session request/approval routing, WebRTC signaling relay
//! (SDP offers/answers + ICE candidates), rate limiting, audit events,
//! and Prometheus-compatible metrics.
//!
//! Storage: in-memory by default (development). PostgreSQL persistence
//! and Redis presence are planned behind feature flags (see
//! `docs/architecture.md`); permissions and device ownership must never
//! depend on Redis alone (§4.3).

pub mod metrics;
pub mod rate_limit;
pub mod state;
pub mod ws;

use std::net::SocketAddr;

/// Server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind: SocketAddr,
}

/// Builds the axum router (also used by integration tests).
pub fn build_router() -> axum::Router {
    use axum::routing::get;
    let shared = std::sync::Arc::new(state::AppState::new());

    axum::Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics_endpoint))
        .route("/api/devices", get(list_devices))
        .route("/ws", get(ws::upgrade))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(shared)
}

async fn health() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "status": "ok", "service": "signaling" }))
}

async fn metrics_endpoint(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<state::AppState>>,
) -> String {
    state.metrics_registry.render()
}

/// Lists currently-online devices (id, name, platform only — no secrets).
async fn list_devices(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<state::AppState>>,
) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "devices": state.online_devices(),
    }))
}

/// Runs the server; returns once the listener is bound, together with a
/// handle that resolves when the server stops.
pub async fn serve(
    config: ServerConfig,
) -> std::io::Result<(SocketAddr, tokio::task::JoinHandle<()>)> {
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    let addr = listener.local_addr()?;
    let app = build_router();
    let handle = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            tracing::error!(%error, "signaling server stopped");
        }
    });
    tracing::info!(%addr, "signaling server listening");
    Ok((addr, handle))
}
