//! Signaling server binary.
//!
//! Configuration via environment:
//! - `BIND`        — listen address (default `127.0.0.1:8080`)
//! - `RUST_LOG`    — tracing filter (default `info`)
//! - `LOG_JSON`    — `1` for JSON logs (server deployments)
//!
//! Development build keeps everything in memory. Run behind TLS (reverse
//! proxy or a terminating load balancer) in staging/production (§15).

use std::net::SocketAddr;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let json = std::env::var("LOG_JSON").ok().as_deref() == Some("1");
    remote_telemetry::init_tracing(json);

    let bind: SocketAddr = std::env::var("BIND")
        .unwrap_or_else(|_| "127.0.0.1:8080".into())
        .parse()
        .expect("BIND must be a valid socket address");

    let (addr, handle) =
        jenderalremote_signaling::serve(jenderalremote_signaling::ServerConfig { bind }).await?;

    tracing::info!(
        %addr,
        "jenderalremote signaling server started (in-memory development mode)"
    );

    tokio::signal::ctrl_c()
        .await
        .expect("failed to wait for ctrl-c");
    handle.abort();
    Ok(())
}
