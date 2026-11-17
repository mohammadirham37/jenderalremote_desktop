//! Minimal Prometheus-compatible counters (text exposition format),
//! kept dependency-free for the MVP.

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub struct MetricsRegistry {
    ws_connections_total: AtomicU64,
    messages_total: AtomicU64,
    messages_rejected_total: AtomicU64,
    registered_devices: AtomicU64,
    sessions_created: AtomicU64,
    sessions_ended: AtomicU64,
}

impl MetricsRegistry {
    pub fn inc_ws_connections(&self) {
        self.ws_connections_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_messages(&self) {
        self.messages_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_messages_rejected(&self) {
        self.messages_rejected_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_registered_devices(&self) {
        self.registered_devices.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_sessions_created(&self) {
        self.sessions_created.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_sessions_ended(&self) {
        self.sessions_ended.fetch_add(1, Ordering::Relaxed);
    }

    /// Renders the Prometheus text format.
    pub fn render(&self) -> String {
        let val = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        format!(
            "# HELP signaling_ws_connections_total WebSocket connections accepted.\n\
             # TYPE signaling_ws_connections_total counter\n\
             signaling_ws_connections_total {}\n\
             # HELP signaling_messages_total Signaling messages processed.\n\
             # TYPE signaling_messages_total counter\n\
             signaling_messages_total {}\n\
             # HELP signaling_messages_rejected_total Messages rejected (schema/rate/auth).\n\
             # TYPE signaling_messages_rejected_total counter\n\
             signaling_messages_rejected_total {}\n\
             # HELP signaling_registered_devices_total Device registrations.\n\
             # TYPE signaling_registered_devices_total counter\n\
             signaling_registered_devices_total {}\n\
             # HELP signaling_sessions_created_total Sessions created.\n\
             # TYPE signaling_sessions_created_total counter\n\
             signaling_sessions_created_total {}\n\
             # HELP signaling_sessions_ended_total Sessions ended (any reason).\n\
             # TYPE signaling_sessions_ended_total counter\n\
             signaling_sessions_ended_total {}\n",
            val(&self.ws_connections_total),
            val(&self.messages_total),
            val(&self.messages_rejected_total),
            val(&self.registered_devices),
            val(&self.sessions_created),
            val(&self.sessions_ended),
        )
    }
}
