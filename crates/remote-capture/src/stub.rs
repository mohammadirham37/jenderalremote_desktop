//! Deterministic stub backend: renders an animated gradient plus a frame
//! counter. Used by tests, CI, and headless development so the whole
//! pipeline can be exercised without a display server or permissions.

use crate::frame::{CapturedFrame, FrameFormat};
use crate::{CaptureConfig, CaptureError, MonitorInfo, ScreenCapture};
use async_trait::async_trait;
use std::time::{Duration, Instant};

pub struct StubCapture {
    width: u32,
    height: u32,
    interval: Duration,
    sequence: u64,
    started: Option<Instant>,
    last_frame: Option<Instant>,
}

impl StubCapture {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            interval: Duration::from_millis(66),
            sequence: 0,
            started: None,
            last_frame: None,
        }
    }
}

#[async_trait]
impl ScreenCapture for StubCapture {
    async fn start(&mut self, config: CaptureConfig) -> Result<(), CaptureError> {
        self.interval = Duration::from_secs_f32(1.0 / config.target_fps.max(1.0));
        self.sequence = 0;
        self.started = Some(Instant::now());
        self.last_frame = None;
        Ok(())
    }

    async fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        if self.started.is_none() {
            return Err(CaptureError::InvalidState(
                "start() must be called before next_frame()".into(),
            ));
        }
        // Pace to the target frame rate.
        if let Some(last) = self.last_frame {
            let elapsed = last.elapsed();
            if elapsed < self.interval {
                tokio::time::sleep(self.interval - elapsed).await;
            }
        }
        self.last_frame = Some(Instant::now());
        self.sequence += 1;

        let t = self
            .started
            .map(|s| s.elapsed().as_secs_f32())
            .unwrap_or(0.0);
        let phase = (t * 0.4).sin() * 0.5 + 0.5;

        let mut data = vec![0u8; (self.width * self.height * 4) as usize];
        let drift = (self.sequence % 64) as u32;
        for y in 0..self.height {
            for x in 0..self.width {
                let idx = ((y * self.width + x) * 4) as usize;
                data[idx] = (x * 255 / self.width.max(1)).min(255) as u8;
                data[idx + 1] = (y * 255 / self.height.max(1)).min(255) as u8;
                data[idx + 2] = (phase * 255.0) as u8;
                data[idx + 3] = if (x + y + drift) % 128 < 4 { 255 } else { 230 };
            }
        }
        Ok(CapturedFrame::new(
            self.sequence,
            self.width,
            self.height,
            FrameFormat::Rgba8,
            data,
        ))
    }

    async fn stop(&mut self) -> Result<(), CaptureError> {
        self.started = None;
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "stub"
    }
}

/// Monitors reported by the stub backend.
pub async fn list_monitors() -> Result<Vec<MonitorInfo>, CaptureError> {
    Ok(vec![MonitorInfo {
        id: 0,
        name: "Stub Display".into(),
        width: 1280,
        height: 720,
        is_primary: true,
    }])
}
