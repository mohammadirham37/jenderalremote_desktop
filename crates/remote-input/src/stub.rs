//! No-op input backend that records applied events. Used by tests and by
//! the controller side (which must never inject locally).

use async_trait::async_trait;
use remote_protocol::{KeyboardEvent, MouseEvent};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{InputController, InputError};

#[derive(Default)]
pub struct StubInput {
    applied: AtomicU64,
    log: Mutex<Vec<String>>,
}

impl StubInput {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn applied_count(&self) -> u64 {
        self.applied.load(Ordering::Relaxed)
    }

    pub fn recorded(&self) -> Vec<String> {
        self.log.lock().expect("stub input lock").clone()
    }

    fn record(&self, what: String) {
        self.applied.fetch_add(1, Ordering::Relaxed);
        self.log.lock().expect("stub input lock").push(what);
    }
}

#[async_trait]
impl InputController for StubInput {
    async fn send_mouse(&self, event: MouseEvent) -> Result<(), InputError> {
        self.record(format!(
            "mouse {:?} {:?} ({:.0},{:.0})",
            event.action, event.button, event.x, event.y
        ));
        Ok(())
    }

    async fn send_keyboard(&self, event: KeyboardEvent) -> Result<(), InputError> {
        self.record(format!("key {:?} {}", event.action, event.key));
        Ok(())
    }

    async fn release_all(&self) -> Result<(), InputError> {
        self.record("release_all".to_string());
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "stub"
    }
}
