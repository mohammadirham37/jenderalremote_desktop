//! Tauri commands — the only bridge between the webview UI and the
//! native engine (§4.4). Every command validates inputs and delegates
//! policy decisions to the engine; the UI is never a source of
//! authorization.

use crate::state::{
    AppState, load_device_token, load_or_create_identity, parse_permissions, save_device_token,
};
use remote_protocol::{DeviceId, SessionId};
use remote_security::HostAccessPolicy;
use remote_webrtc::{ControllerCmd, ControllerEngine, EngineConfig, HostCmd, HostEngine};
use std::sync::Arc;
use tauri::{Manager, State};

fn engine_config(server_url: &str) -> EngineConfig {
    EngineConfig {
        signaling_url: format!("{server_url}/ws"),
        ..EngineConfig::default()
    }
}

#[derive(serde::Serialize)]
pub struct DeviceInfo {
    pub device_id: String,
    pub platform: String,
    pub has_saved_token: bool,
}

#[tauri::command]
pub fn device_info(app: tauri::AppHandle, state: State<AppState>) -> Result<DeviceInfo, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app data dir: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let identity = {
        let mut guard = state.identity.lock().expect("identity lock");
        match guard.as_ref() {
            Some(identity) => identity.clone(),
            None => {
                let identity = load_or_create_identity(&dir)?;
                *guard = Some(identity.clone());
                identity
            }
        }
    };
    Ok(DeviceInfo {
        device_id: identity.device_id().as_str().to_string(),
        platform: std::env::consts::OS.to_string(),
        has_saved_token: load_device_token()?.is_some(),
    })
}

#[tauri::command]
pub async fn host_start(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    server_url: String,
) -> Result<(), String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app data dir: {e}"))?;

    let identity = {
        let mut guard = state.identity.lock().expect("identity lock");
        match guard.as_ref() {
            Some(identity) => identity.clone(),
            None => {
                let identity = load_or_create_identity(&dir)?;
                *guard = Some(identity.clone());
                identity
            }
        }
    };
    let token = load_device_token()?;

    let capture_factory: remote_webrtc::host::CaptureFactory =
        Arc::new(|| Box::new(remote_capture::XcapCapture::new()));
    let input: Arc<dyn remote_input::InputController> = Arc::new(remote_input::EnigoInput::new());

    let handle = HostEngine::start(
        engine_config(&server_url),
        identity,
        token,
        capture_factory,
        input,
        HostAccessPolicy::default(), // unattended stays disabled (fase 3)
    )
    .map_err(|e| e.to_string())?;

    // Persist a freshly rotated device token if the server issues one.
    let token_app = app.clone();
    let mut inner_handle = handle;
    // The pump also stores tokens on TokenIssued events via a side channel:
    // simplest is to store from the DTO pump below; instead we spawn a
    // thin wrapper pump here.
    let cmd = inner_handle.cmd_tx();
    let pump = tokio::spawn(async move {
        use tauri::Emitter;
        while let Some(event) = inner_handle.next_event().await {
            if let remote_webrtc::HostEvent::TokenIssued(ref token) = event {
                if let Err(error) = save_device_token(token) {
                    tracing::warn!(%error, "gagal menyimpan device token");
                } else {
                    let _ = token_app.emit("device://token-saved", ());
                }
            }
            let _ = token_app.emit("host://event", crate::state::HostEventDto::from(event));
        }
    });
    *state.host.lock().expect("host slot") = Some(crate::state::EngineSlot { cmd, pump });
    Ok(())
}

#[tauri::command]
pub fn host_respond(
    state: State<AppState>,
    session_id: String,
    approved: bool,
    permissions: Vec<String>,
) -> Result<(), String> {
    let guard = state.host.lock().expect("host slot");
    let Some(slot) = guard.as_ref() else {
        return Err("host belum aktif".into());
    };
    slot.cmd
        .send(HostCmd::Respond {
            session_id: SessionId::new(session_id),
            approved,
            permissions: parse_permissions(&permissions),
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn host_end(state: State<AppState>, reason: Option<String>) -> Result<(), String> {
    let mut guard = state.host.lock().expect("host slot");
    if let Some(slot) = guard.take() {
        let _ = slot.cmd.send(HostCmd::End { reason });
        slot.pump.abort();
    }
    Ok(())
}

#[tauri::command]
pub async fn controller_connect(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    server_url: String,
    target_device_id: String,
    mode: String,
    permissions: Vec<String>,
) -> Result<(), String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app data dir: {e}"))?;

    let identity = {
        let mut guard = state.identity.lock().expect("identity lock");
        match guard.as_ref() {
            Some(identity) => identity.clone(),
            None => {
                let identity = load_or_create_identity(&dir)?;
                *guard = Some(identity.clone());
                identity
            }
        }
    };
    let token = load_device_token()?;
    let mode = match mode.as_str() {
        "unattended" => remote_protocol::AccessMode::Unattended,
        _ => remote_protocol::AccessMode::Attended,
    };
    let requested = parse_permissions(&permissions);
    if let Err(reason) = remote_protocol::validate_request(&requested) {
        return Err(reason.into());
    }

    let handle = ControllerEngine::start(
        engine_config(&server_url),
        identity,
        token,
        DeviceId::new(target_device_id),
        mode,
        requested,
    )
    .map_err(|e| e.to_string())?;

    let token_app = app.clone();
    let mut inner = handle;
    let cmd = inner.cmd_tx();
    let pump = tokio::spawn(async move {
        use tauri::Emitter;
        while let Some(event) = inner.next_event().await {
            if let remote_webrtc::ControllerEvent::TokenIssued(ref token) = event
                && let Err(error) = save_device_token(token)
            {
                tracing::warn!(%error, "gagal menyimpan device token");
            }
            let _ = token_app.emit(
                "controller://event",
                crate::state::ControllerEventDto::from(event),
            );
        }
    });

    *state.controller.lock().expect("controller slot") =
        Some(crate::state::EngineSlot { cmd, pump });
    Ok(())
}

#[tauri::command]
pub fn controller_mouse(
    state: State<AppState>,
    nx: f64,
    ny: f64,
    action: String,
    button: String,
    scroll_dx: f64,
    scroll_dy: f64,
) -> Result<(), String> {
    let guard = state.controller.lock().expect("controller slot");
    let Some(slot) = guard.as_ref() else {
        return Ok(()); // no active session: ignore
    };
    let action = match action.as_str() {
        "press" => remote_protocol::MouseAction::Press,
        "release" => remote_protocol::MouseAction::Release,
        "scroll" => remote_protocol::MouseAction::Scroll,
        _ => remote_protocol::MouseAction::Move,
    };
    let button = match button.as_str() {
        "right" => remote_protocol::MouseButton::Right,
        "middle" => remote_protocol::MouseButton::Middle,
        _ => remote_protocol::MouseButton::Left,
    };
    slot.cmd
        .send(ControllerCmd::Mouse {
            nx,
            ny,
            action,
            button,
            scroll_dx,
            scroll_dy,
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn controller_keyboard(
    state: State<AppState>,
    action: String,
    key: String,
    modifiers: Vec<String>,
) -> Result<(), String> {
    let guard = state.controller.lock().expect("controller slot");
    let Some(slot) = guard.as_ref() else {
        return Ok(());
    };
    let key_action = match action.as_str() {
        "release" => remote_protocol::KeyAction::Release,
        _ => remote_protocol::KeyAction::Press,
    };
    let mods = modifiers
        .iter()
        .filter_map(|m| match m.as_str() {
            "ctrl" => Some(remote_protocol::Modifier::Ctrl),
            "alt" => Some(remote_protocol::Modifier::Alt),
            "shift" => Some(remote_protocol::Modifier::Shift),
            "meta" => Some(remote_protocol::Modifier::Meta),
            _ => None,
        })
        .collect();

    // Map a couple of browser key names the wire protocol spells
    // differently (see remote-input::keys::NAMED_KEYS).
    let key = match key.as_str() {
        "Enter" => "Enter".to_string(),
        other => other.to_string(),
    };

    slot.cmd
        .send(ControllerCmd::Keyboard(remote_protocol::KeyboardEvent {
            action: key_action,
            key,
            modifiers: mods,
        }))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn controller_end(state: State<AppState>, reason: Option<String>) -> Result<(), String> {
    let mut guard = state.controller.lock().expect("controller slot");
    if let Some(slot) = guard.take() {
        let _ = slot.cmd.send(ControllerCmd::End { reason });
        slot.pump.abort();
    }
    Ok(())
}
