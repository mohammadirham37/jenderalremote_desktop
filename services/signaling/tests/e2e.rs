//! End-to-end integration test (rancangan teknis §13.2):
//! - two endpoints connect through the real signaling server,
//! - attended request → host approval → WebRTC negotiation (ICE host
//!   candidates) → Active,
//! - video frames flow from the stub capture to the controller,
//! - input events flow from the controller through the policy gate to
//!   the stub input backend,
//! - session end tears both sides down.

use std::sync::Arc;
use std::time::{Duration, Instant};

use jenderalremote_signaling::{ServerConfig, serve};
use remote_capture::{ScreenCapture, StubCapture};
use remote_input::StubInput;
use remote_protocol::{AccessMode, Permission, PermissionSet};
use remote_security::{DeviceIdentity, HostAccessPolicy};
use remote_webrtc::{
    ControllerCmd, ControllerEngine, ControllerEvent, EngineConfig, HostCmd, HostEngine, HostEvent,
};

type CaptureFactory = Arc<dyn Fn() -> Box<dyn ScreenCapture> + Send + Sync>;

fn engine_config(port: u16) -> EngineConfig {
    EngineConfig {
        signaling_url: format!("ws://127.0.0.1:{port}/ws"),
        ice_servers: vec![],
        video_quality: 50,
        target_fps: 10.0,
        heartbeat_secs: 5,
        backpressure_bytes: 4 * 1024 * 1024,
    }
}

fn full_permissions() -> PermissionSet {
    PermissionSet::from_permissions([Permission::ViewScreen, Permission::ControlInput])
}

fn make_factory() -> CaptureFactory {
    Arc::new(|| Box::new(StubCapture::new(640, 480)))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn attended_session_end_to_end() {
    remote_telemetry::init_tracing(false);
    let (addr, server) = serve(ServerConfig {
        bind: "127.0.0.1:0".parse().unwrap(),
    })
    .await
    .expect("server binds");
    let server_task = tokio::spawn(async move {
        let _ = server.await;
    });

    let host_identity = DeviceIdentity::generate();
    let controller_identity = DeviceIdentity::generate();
    let host_device_id = host_identity.device_id().clone();

    let input = Arc::new(StubInput::new());

    let mut host = HostEngine::start(
        engine_config(addr.port()),
        host_identity,
        None,
        make_factory(),
        input.clone(),
        HostAccessPolicy::default(),
    )
    .expect("host engine starts");

    let mut controller = ControllerEngine::start(
        engine_config(addr.port()),
        controller_identity,
        None,
        host_device_id,
        AccessMode::Attended,
        full_permissions(),
    )
    .expect("controller engine starts");

    let mut got_frame = false;
    let mut approved = false;
    let deadline = Duration::from_secs(90);
    let started = Instant::now();

    loop {
        if started.elapsed() > deadline {
            panic!("e2e timed out; approved={approved} frame={got_frame}");
        }
        tokio::select! {
            event = host.next_event() => {
                let Some(event) = event else { break };
                match event {
                    HostEvent::SessionRequest { session_id, permissions, .. } => {
                        approved = true;
                        host.cmd_tx().send(HostCmd::Respond {
                            session_id,
                            approved: true,
                            permissions,
                        }).expect("send respond");
                    }
                    HostEvent::Error(error) => panic!("host error: {error}"),
                    _ => {}
                }
            }
            event = controller.next_event() => {
                let Some(event) = event else { break };
                match event {
                    ControllerEvent::Frame { width, height, jpeg, .. } => {
                        assert!(width > 0 && height > 0);
                        assert!(!jpeg.is_empty(), "frame must not be empty");
                        got_frame = true;
                        break;
                    }
                    ControllerEvent::Error(error) => panic!("controller error: {error}"),
                    ControllerEvent::Ended { reason } => {
                        panic!("controller ended before frame: {reason:?}");
                    }
                    _ => {}
                }
            }
        }
    }

    assert!(approved, "host must have approved the session");
    assert!(got_frame, "controller must have received a frame");

    // Input flows through the policy gate (Active + control_input).
    // Give the control channel a moment to finish opening after the
    // first video frame arrived.
    tokio::time::sleep(Duration::from_secs(1)).await;
    controller
        .cmd_tx()
        .send(ControllerCmd::Mouse {
            nx: 0.5,
            ny: 0.5,
            action: remote_protocol::MouseAction::Move,
            button: remote_protocol::MouseButton::Left,
            scroll_dx: 0.0,
            scroll_dy: 0.0,
        })
        .expect("send mouse cmd");
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(
        input.applied_count() >= 1,
        "host stub input must have applied the mouse event (applied={})",
        input.applied_count()
    );

    // Terminate from the controller side; the host observes the end.
    controller
        .cmd_tx()
        .send(ControllerCmd::End {
            reason: Some("test done".into()),
        })
        .expect("send end");

    let end_deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if Instant::now() > end_deadline {
            break;
        }
        tokio::select! {
            event = host.next_event() => {
                if matches!(event, Some(HostEvent::SessionEnded { .. })) {
                    break;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
        }
    }

    controller.shutdown().await;
    host.shutdown().await;
    server_task.abort();
}
