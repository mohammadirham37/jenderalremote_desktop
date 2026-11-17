//! Per-device token-bucket rate limiting (§11.3: serangan terhadap
//! signaling — rate limit, schema validation, batas ukuran pesan).

use remote_protocol::DeviceId;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

pub struct RateLimiter {
    capacity: f64,
    refill_per_sec: f64,
    buckets: Mutex<HashMap<DeviceId, (f64, Instant)>>,
}

impl RateLimiter {
    /// `capacity` burst size, refilled at `refill_per_sec`.
    pub fn new(capacity: f64, refill_per_sec: f64) -> Self {
        Self {
            capacity,
            refill_per_sec,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Consumes one token; returns false when over the limit.
    pub fn allow(&self, device: &DeviceId) -> bool {
        let mut buckets = self.buckets.lock().expect("rate limiter lock");
        let now = Instant::now();
        let (mut tokens, last) = buckets.remove(device).unwrap_or((self.capacity, now));
        let elapsed = now.duration_since(last).as_secs_f64();
        tokens = (tokens + elapsed * self.refill_per_sec).min(self.capacity);
        if tokens >= 1.0 {
            tokens -= 1.0;
            buckets.insert(device.clone(), (tokens, now));
            true
        } else {
            buckets.insert(device.clone(), (tokens, now));
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_burst_then_blocks() {
        let limiter = RateLimiter::new(3.0, 0.0); // no refill
        let device = DeviceId::new("dev");
        assert!(limiter.allow(&device));
        assert!(limiter.allow(&device));
        assert!(limiter.allow(&device));
        assert!(!limiter.allow(&device));
    }
}
