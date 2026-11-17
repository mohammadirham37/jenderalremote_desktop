//! Native capture backend built on `xcap` (Windows Graphics Capture /
//! macOS CoreGraphics / Linux X11). See `docs/adr/0002` for the phased
//! plan toward ScreenCaptureKit and the PipeWire portal.
//!
//! Threading: the native monitor handle is not `Send` on every platform
//! (e.g. Windows WGC pointers), so it lives on a dedicated capture thread.
//! The async [`ScreenCapture`] implementation only shuttles commands and
//! frames through channels — futures stay `Send`.

use crate::frame::{CapturedFrame, FrameFormat};
use crate::{CaptureConfig, CaptureError, MonitorInfo, ScreenCapture};
use async_trait::async_trait;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

/// Thread-side error kinds, transported as `(kind, message)`.
const KIND_IO: u8 = 0;
const KIND_UNSUPPORTED: u8 = 1;
const KIND_PERMISSION: u8 = 2;
const KIND_INVALID_STATE: u8 = 3;

type ThreadError = (u8, String);

fn thread_error(kind: u8, message: impl Into<String>) -> ThreadError {
    (kind, message.into())
}

fn map_thread_error((kind, message): ThreadError) -> CaptureError {
    match kind {
        KIND_UNSUPPORTED => CaptureError::Unsupported(message),
        KIND_PERMISSION => CaptureError::PermissionDenied {
            detail: message,
            guidance: CaptureError::permission_guidance(),
        },
        KIND_INVALID_STATE => CaptureError::InvalidState(message),
        _ => CaptureError::Io(message),
    }
}

enum Cmd {
    Start(CaptureConfig, std_mpsc::Sender<Result<(), ThreadError>>),
    Frame(std_mpsc::Sender<Result<(u32, u32, Vec<u8>), ThreadError>>),
    Stop(std_mpsc::Sender<Result<(), ThreadError>>),
}

pub struct XcapCapture {
    tx: std_mpsc::SyncSender<Cmd>,
    interval: Duration,
    sequence: u64,
    last_frame: Option<Instant>,
}

impl XcapCapture {
    pub fn new() -> Self {
        let (tx, rx) = std_mpsc::sync_channel::<Cmd>(8);
        std::thread::Builder::new()
            .name("xcap-capture".into())
            .spawn(move || capture_thread(rx))
            .expect("spawn xcap capture thread");
        Self {
            tx,
            interval: Duration::from_millis(66),
            sequence: 0,
            last_frame: None,
        }
    }

    /// Sends a command and awaits the thread reply on a blocking task.
    /// Reply channels always carry `Result<T, ThreadError>` so a dead
    /// thread degrades into a transportable error.
    async fn dispatch<T: Send + 'static>(
        &self,
        make: impl FnOnce(std_mpsc::Sender<Result<T, ThreadError>>) -> Cmd,
    ) -> Result<T, CaptureError> {
        let (reply_tx, reply_rx) = std_mpsc::channel();
        self.tx
            .send(make(reply_tx))
            .map_err(|_| CaptureError::Io("capture thread exited".into()))?;
        tokio::task::spawn_blocking(move || {
            reply_rx
                .recv()
                .unwrap_or_else(|_| Err(thread_error(KIND_IO, "capture thread dropped reply")))
        })
        .await
        .map_err(|e| CaptureError::Io(format!("join error: {e}")))?
        .map_err(map_thread_error)
    }
}

impl Default for XcapCapture {
    fn default() -> Self {
        Self::new()
    }
}

fn capture_thread(rx: std_mpsc::Receiver<Cmd>) {
    let mut monitor: Option<xcap::Monitor> = None;
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Start(config, reply) => {
                let result = select_monitor(config.monitor_index).map(|selected| {
                    tracing::info!(
                        monitor = %selected.name().unwrap_or_default(),
                        width = selected.width().unwrap_or(0),
                        height = selected.height().unwrap_or(0),
                        "starting native screen capture"
                    );
                    monitor = Some(selected);
                });
                let _ = reply.send(result);
            }
            Cmd::Frame(reply) => {
                let result = capture_frame(monitor.as_ref());
                let _ = reply.send(result);
            }
            Cmd::Stop(reply) => {
                monitor = None;
                let _ = reply.send(Ok(()));
            }
        }
    }
    tracing::debug!("xcap capture thread finished");
}

fn select_monitor(index: usize) -> Result<xcap::Monitor, ThreadError> {
    let mut monitors = xcap::Monitor::all().map_err(|e| thread_error(KIND_IO, e.to_string()))?;
    if monitors.is_empty() {
        return Err(thread_error(
            KIND_UNSUPPORTED,
            "no monitor found for capture",
        ));
    }
    if index < monitors.len() {
        return Ok(monitors.swap_remove(index));
    }
    // Fall back to the primary monitor.
    monitors.sort_by_key(|m| !m.is_primary().unwrap_or(false));
    Ok(monitors.swap_remove(0))
}

fn capture_frame(monitor: Option<&xcap::Monitor>) -> Result<(u32, u32, Vec<u8>), ThreadError> {
    let Some(monitor) = monitor else {
        return Err(thread_error(KIND_INVALID_STATE, "capture not started"));
    };
    let image = monitor
        .capture_image()
        .map_err(|e| classify_xcap_error(e.to_string()))?;
    let (width, height) = (image.width(), image.height());
    if width == 0 || height == 0 {
        // On macOS a zero-size image is how a missing Screen Recording
        // permission frequently manifests.
        return Err(thread_error(KIND_PERMISSION, "empty capture image"));
    }
    Ok((width, height, image.into_raw()))
}

fn classify_xcap_error(message: String) -> ThreadError {
    let lower = message.to_lowercase();
    if lower.contains("permission") || lower.contains("not authorized") || lower.contains("denied")
    {
        return thread_error(KIND_PERMISSION, message);
    }
    thread_error(KIND_IO, message)
}

#[async_trait]
impl ScreenCapture for XcapCapture {
    async fn start(&mut self, config: CaptureConfig) -> Result<(), CaptureError> {
        self.interval = Duration::from_secs_f32(1.0 / config.target_fps.max(1.0));
        self.sequence = 0;
        self.last_frame = None;
        self.dispatch(|reply| Cmd::Start(config, reply)).await
    }

    async fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        if let Some(last) = self.last_frame {
            let elapsed = last.elapsed();
            if elapsed < self.interval {
                tokio::time::sleep(self.interval - elapsed).await;
            }
        }
        let (width, height, rgba) = self.dispatch(Cmd::Frame).await?;
        self.last_frame = Some(Instant::now());
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
        self.last_frame = None;
        self.dispatch(Cmd::Stop).await
    }

    fn backend_name(&self) -> &'static str {
        "xcap-native"
    }
}

/// Lists physical monitors via the native backend on a blocking task
/// (monitor enumeration shares the same non-Send native handles).
pub async fn list_monitors() -> Result<Vec<MonitorInfo>, CaptureError> {
    tokio::task::spawn_blocking(|| {
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
    })
    .await
    .map_err(|e| CaptureError::Io(format!("join error: {e}")))?
}
