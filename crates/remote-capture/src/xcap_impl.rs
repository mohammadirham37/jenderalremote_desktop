//! Native capture backend built on `xcap` (Windows Graphics Capture /
//! macOS CoreGraphics / Linux X11). See `docs/adr/0002` for the phased
//! plan toward ScreenCaptureKit and the PipeWire portal.

use crate::frame::{CapturedFrame, FrameFormat, bgra_to_rgba};
use crate::{CaptureConfig, CaptureError, MonitorInfo, ScreenCapture};
use async_trait::async_trait;
use std::time::{Duration, Instant};

pub struct XcapCapture {
    monitor: Option<xcap::Monitor>,
    interval: Duration,
    sequence: u64,
    last_frame: Option<Instant>,
}

impl XcapCapture {
    pub fn new() -> Self {
        Self {
            monitor: None,
            interval: Duration::from_millis(66),
            sequence: 0,
            last_frame: None,
        }
    }

    fn select_monitor(index: usize) -> Result<xcap::Monitor, CaptureError> {
        let mut monitors = xcap::Monitor::all().map_err(|e| CaptureError::Io(e.to_string()))?;
        if monitors.is_empty() {
            return Err(CaptureError::Unsupported(
                "no monitor found for capture".into(),
            ));
        }
        if index < monitors.len() {
            return Ok(monitors.swap_remove(index));
        }
        // Fall back to the primary monitor.
        monitors.sort_by_key(|m| !m.is_primary().unwrap_or(false));
        Ok(monitors.swap_remove(0))
    }
}

impl Default for XcapCapture {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ScreenCapture for XcapCapture {
    async fn start(&mut self, config: CaptureConfig) -> Result<(), CaptureError> {
        let monitor = Self::select_monitor(config.monitor_index)?;
        tracing::info!(
            monitor = %monitor.name().unwrap_or_default(),
            width = monitor.width().unwrap_or(0),
            height = monitor.height().unwrap_or(0),
            "starting native screen capture"
        );
        self.monitor = Some(monitor);
        self.interval = Duration::from_secs_f32(1.0 / config.target_fps.max(1.0));
        self.sequence = 0;
        self.last_frame = None;
        Ok(())
    }

    async fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        let monitor = self
            .monitor
            .as_ref()
            .ok_or_else(|| CaptureError::InvalidState("capture not started".into()))?;

        if let Some(last) = self.last_frame {
            let elapsed = last.elapsed();
            if elapsed < self.interval {
                tokio::time::sleep(self.interval - elapsed).await;
            }
        }

        let capture_started = Instant::now();
        let image = monitor
            .capture_image()
            .map_err(|e| map_xcap_error(&e.to_string()))?;
        self.last_frame = Some(Instant::now());
        let _ = capture_started;

        let (width, height) = (image.width(), image.height());
        if width == 0 || height == 0 {
            // On macOS a zero-size image is how a missing Screen Recording
            // permission frequently manifests.
            return Err(CaptureError::PermissionDenied {
                detail: "empty capture image".into(),
                guidance: CaptureError::permission_guidance(),
            });
        }

        let mut rgba = image.into_raw();
        if xcap_outputs_bgra() {
            bgra_to_rgba(&mut rgba);
        }

        self.sequence += 1;
        Ok(CapturedFrame::new(
            self.sequence,
            width,
            height,
            FrameFormat::Rgba8,
            rgba,
        ))
    }

    async fn stop(&mut self) -> Result<(), CaptureError> {
        self.monitor = None;
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "xcap-native"
    }
}

fn xcap_outputs_bgra() -> bool {
    // `xcap` normalizes to RGBA8 on every platform today; keep the switch
    // centralized in case a backend diverges.
    false
}

fn map_xcap_error(message: &str) -> CaptureError {
    let lower = message.to_lowercase();
    if lower.contains("permission") || lower.contains("not authorized") || lower.contains("denied")
    {
        return CaptureError::PermissionDenied {
            detail: message.to_string(),
            guidance: CaptureError::permission_guidance(),
        };
    }
    CaptureError::Io(message.to_string())
}

/// Lists physical monitors via the native backend.
pub async fn list_monitors() -> Result<Vec<MonitorInfo>, CaptureError> {
    let monitors = xcap::Monitor::all().map_err(|e| CaptureError::Io(e.to_string()))?;
    Ok(monitors
        .into_iter()
        .enumerate()
        .map(|(index, m)| MonitorInfo {
            id: index as u32,
            name: m.name().unwrap_or_default(),
            width: m.width().unwrap_or(0),
            height: m.height().unwrap_or(0),
            is_primary: m.is_primary().unwrap_or(false),
        })
        .collect())
}
