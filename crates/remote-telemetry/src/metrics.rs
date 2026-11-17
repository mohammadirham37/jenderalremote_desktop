//! Per-session counters and gauges. All fields are atomics so the struct
//! is `Sync` and cheap to share across capture/encode/send tasks.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

#[derive(Debug, Default)]
pub struct SessionMetrics {
    pub frames_captured: AtomicU64,
    pub frames_encoded: AtomicU64,
    pub frames_skipped: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub input_events_received: AtomicU64,
    pub input_events_applied: AtomicU64,
    pub input_events_rejected: AtomicU64,
    pub encode_errors: AtomicU64,
    /// Exponentially-smoothed encode time in microseconds.
    encode_us_ema: AtomicU64,
    /// Exponentially-smoothed capture time in microseconds.
    capture_us_ema: AtomicU64,
    /// Last measured round-trip time in milliseconds (ping/pong).
    rtt_ms: AtomicU32,
}

impl SessionMetrics {
    pub fn inc_frames_captured(&self) {
        self.frames_captured.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_frames_encoded(&self, encode_us: u128) {
        self.frames_encoded.fetch_add(1, Ordering::Relaxed);
        self.update_ema(&self.encode_us_ema, encode_us);
    }

    pub fn inc_frames_skipped(&self) {
        self.frames_skipped.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_bytes_sent(&self, n: u64) {
        self.bytes_sent.fetch_add(n, Ordering::Relaxed);
    }

    pub fn inc_input_received(&self) {
        self.input_events_received.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_input_applied(&self) {
        self.input_events_applied.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_input_rejected(&self) {
        self.input_events_rejected.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_encode_error(&self) {
        self.encode_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn observe_capture(&self, started: Instant) {
        self.update_ema(&self.capture_us_ema, started.elapsed().as_micros());
    }

    pub fn observe_rtt_ms(&self, rtt_ms: u32) {
        self.rtt_ms.store(rtt_ms, Ordering::Relaxed);
    }

    fn update_ema(&self, slot: &AtomicU64, sample: u128) {
        const ALPHA_NUM: u64 = 30; // ~30% smoothing
        const ALPHA_DEN: u64 = 100;
        let sample = sample as u64;
        let mut prev = slot.load(Ordering::Relaxed);
        loop {
            let next = if prev == 0 {
                sample
            } else {
                prev + (sample.saturating_sub(prev) * ALPHA_NUM / ALPHA_DEN)
                    .saturating_sub(prev.saturating_sub(sample) * ALPHA_NUM / ALPHA_DEN)
            };
            match slot.compare_exchange_weak(prev, next, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(actual) => prev = actual,
            }
        }
    }

    /// Smoothed frames-per-second estimate given elapsed wall time.
    pub fn fps(&self, since: Instant) -> f32 {
        let frames = self.frames_encoded.load(Ordering::Relaxed);
        let secs = since.elapsed().as_secs_f32().max(0.001);
        frames as f32 / secs
    }

    pub fn encode_ms(&self) -> f32 {
        self.encode_us_ema.load(Ordering::Relaxed) as f32 / 1000.0
    }

    pub fn capture_ms(&self) -> f32 {
        self.capture_us_ema.load(Ordering::Relaxed) as f32 / 1000.0
    }

    pub fn rtt_ms(&self) -> u32 {
        self.rtt_ms.load(Ordering::Relaxed)
    }

    pub fn snapshot(&self, since: Instant) -> MetricsSnapshot {
        MetricsSnapshot {
            frames_captured: self.frames_captured.load(Ordering::Relaxed),
            frames_encoded: self.frames_encoded.load(Ordering::Relaxed),
            frames_skipped: self.frames_skipped.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            input_events_received: self.input_events_received.load(Ordering::Relaxed),
            input_events_applied: self.input_events_applied.load(Ordering::Relaxed),
            input_events_rejected: self.input_events_rejected.load(Ordering::Relaxed),
            encode_errors: self.encode_errors.load(Ordering::Relaxed),
            fps: self.fps(since),
            encode_ms: self.encode_ms(),
            capture_ms: self.capture_ms(),
            rtt_ms: self.rtt_ms(),
        }
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct MetricsSnapshot {
    pub frames_captured: u64,
    pub frames_encoded: u64,
    pub frames_skipped: u64,
    pub bytes_sent: u64,
    pub input_events_received: u64,
    pub input_events_applied: u64,
    pub input_events_rejected: u64,
    pub encode_errors: u64,
    pub fps: f32,
    pub encode_ms: f32,
    pub capture_ms: f32,
    pub rtt_ms: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_accumulate() {
        let metrics = SessionMetrics::default();
        metrics.inc_frames_captured();
        metrics.inc_frames_captured();
        metrics.inc_frames_encoded(1200);
        let snapshot = metrics.snapshot(Instant::now());
        assert_eq!(snapshot.frames_captured, 2);
        assert_eq!(snapshot.frames_encoded, 1);
        assert!(snapshot.encode_ms > 0.0);
    }
}
