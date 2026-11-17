//! Native input injection via `enigo`:
//! - Windows: `SendInput`
//! - macOS: CoreGraphics `CGEvent` (requires Accessibility permission)
//! - Linux: X11 `XTest` extension
//!
//! Enigo is not `Sync` by construction, so all injection goes through a
//! dedicated blocking thread with a channel; the async trait methods
//! only await results.

use crate::keys::EnigoKey;
use crate::{InputController, InputError};
use async_trait::async_trait;
use enigo::{Axis, Coordinate, Direction, Keyboard, Mouse};
use remote_protocol::{KeyAction, KeyboardEvent, Modifier, MouseAction, MouseButton, MouseEvent};
use std::sync::mpsc as std_mpsc;

/// Commands executed on the dedicated input thread.
enum Cmd {
    Mouse(MouseEvent),
    Keyboard(KeyboardEvent),
    ReleaseAll,
}

pub struct EnigoInput {
    tx: std_mpsc::SyncSender<(Cmd, std_mpsc::Sender<Result<(), InputError>>)>,
}

impl EnigoInput {
    pub fn new() -> Self {
        let (tx, rx) =
            std_mpsc::sync_channel::<(Cmd, std_mpsc::Sender<Result<(), InputError>>)>(64);
        std::thread::Builder::new()
            .name("enigo-input".into())
            .spawn(move || {
                let mut enigo = match enigo::Enigo::new(&enigo::Settings::default()) {
                    Ok(enigo) => enigo,
                    Err(e) => {
                        // Drain and report the failure for every command.
                        while let Ok((cmd, reply)) = rx.recv() {
                            let _ = cmd;
                            let _ = reply.send(Err(map_enigo_error(&e.to_string())));
                        }
                        return;
                    }
                };
                while let Ok((cmd, reply)) = rx.recv() {
                    let result = run_cmd(&mut enigo, &cmd);
                    if reply.send(result).is_err() {
                        break; // caller gone
                    }
                }
            })
            .expect("spawn enigo thread");
        Self { tx }
    }

    async fn dispatch(&self, cmd: Cmd) -> Result<(), InputError> {
        let (reply_tx, reply_rx) = std_mpsc::channel();
        self.tx
            .send((cmd, reply_tx))
            .map_err(|_| InputError::Backend("input thread exited".into()))?;
        tokio::task::spawn_blocking(move || {
            reply_rx
                .recv()
                .unwrap_or_else(|_| Err(InputError::Backend("input thread dropped reply".into())))
        })
        .await
        .map_err(|e| InputError::Backend(format!("join error: {e}")))?
    }
}

impl Default for EnigoInput {
    fn default() -> Self {
        Self::new()
    }
}

fn run_cmd(enigo: &mut enigo::Enigo, cmd: &Cmd) -> Result<(), InputError> {
    match cmd {
        Cmd::Mouse(event) => run_mouse(enigo, event),
        Cmd::Keyboard(event) => run_keyboard(enigo, event),
        Cmd::ReleaseAll => {
            for key in [
                enigo::Key::Shift,
                enigo::Key::Control,
                enigo::Key::Alt,
                enigo::Key::Meta,
                enigo::Key::Space,
            ] {
                let _ = enigo.key(key, Direction::Release);
            }
            for button in [
                enigo::Button::Left,
                enigo::Button::Right,
                enigo::Button::Middle,
            ] {
                let _ = enigo.button(button, Direction::Release);
            }
            Ok(())
        }
    }
}

fn run_mouse(enigo: &mut enigo::Enigo, event: &MouseEvent) -> Result<(), InputError> {
    // Modifiers must be held during synthesis for correct chords.
    hold_modifiers(enigo, &event_sync_modifiers_stub(event))?;
    let button = match event.button {
        MouseButton::Left => enigo::Button::Left,
        MouseButton::Right => enigo::Button::Right,
        MouseButton::Middle => enigo::Button::Middle,
    };
    let result = match event.action {
        MouseAction::Move => enigo
            .move_mouse(event.x as i32, event.y as i32, Coordinate::Abs)
            .map_err(|e| map_enigo_error(&e.to_string())),
        MouseAction::Press => enigo
            .button(button, Direction::Press)
            .map_err(|e| map_enigo_error(&e.to_string())),
        MouseAction::Release => enigo
            .button(button, Direction::Release)
            .map_err(|e| map_enigo_error(&e.to_string())),
        MouseAction::Scroll => {
            let dy = event.scroll_dy;
            let dx = event.scroll_dx;
            if dy != 0.0 {
                enigo
                    .scroll(dy as i32, Axis::Vertical)
                    .map_err(|e| map_enigo_error(&e.to_string()))?;
            }
            if dx != 0.0 {
                enigo
                    .scroll(dx as i32, Axis::Horizontal)
                    .map_err(|e| map_enigo_error(&e.to_string()))?;
            }
            Ok(())
        }
    };
    let _ = release_modifiers(enigo);
    result
}

fn run_keyboard(enigo: &mut enigo::Enigo, event: &KeyboardEvent) -> Result<(), InputError> {
    let key = match crate::keys::resolve_key(&event.key) {
        Some(EnigoKey::Char(c)) => enigo::Key::Unicode(c),
        Some(EnigoKey::Named(name)) => match named_enigo_key(&name) {
            Some(key) => key,
            None => {
                return Err(InputError::Unsupported(format!(
                    "unmapped named key {}",
                    event.key
                )));
            }
        },
        None => {
            return Err(InputError::Unsupported(format!(
                "invalid wire key {:?}",
                event.key
            )));
        }
    };

    // Press modifiers first, then the key, then release in reverse.
    let pressed: Vec<enigo::Key> = event
        .modifiers
        .iter()
        .filter_map(|m| modifier_enigo_key(*m))
        .collect();
    for modifier in &pressed {
        enigo
            .key(*modifier, Direction::Press)
            .map_err(|e| map_enigo_error(&e.to_string()))?;
    }
    let result = match event.action {
        KeyAction::Press => enigo
            .key(key, Direction::Press)
            .map_err(|e| map_enigo_error(&e.to_string())),
        KeyAction::Release => enigo
            .key(key, Direction::Release)
            .map_err(|e| map_enigo_error(&e.to_string())),
    };
    for modifier in pressed.iter().rev() {
        let _ = enigo.key(*modifier, Direction::Release);
    }
    result
}

/// Mouse events do not carry modifiers on the wire yet; placeholder
/// returns an empty set.
fn event_sync_modifiers_stub(_event: &MouseEvent) -> Vec<Modifier> {
    Vec::new()
}

fn hold_modifiers(enigo: &mut enigo::Enigo, modifiers: &[Modifier]) -> Result<(), InputError> {
    for modifier in modifiers.iter().filter_map(|m| modifier_enigo_key(*m)) {
        enigo
            .key(modifier, Direction::Press)
            .map_err(|e| map_enigo_error(&e.to_string()))?;
    }
    Ok(())
}

fn release_modifiers(enigo: &mut enigo::Enigo) -> Result<(), InputError> {
    for key in [
        enigo::Key::Shift,
        enigo::Key::Control,
        enigo::Key::Alt,
        enigo::Key::Meta,
    ] {
        let _ = enigo.key(key, Direction::Release);
    }
    Ok(())
}

fn modifier_enigo_key(modifier: Modifier) -> Option<enigo::Key> {
    Some(match modifier {
        Modifier::Ctrl => enigo::Key::Control,
        Modifier::Alt => enigo::Key::Alt,
        Modifier::Shift => enigo::Key::Shift,
        Modifier::Meta => enigo::Key::Meta,
    })
}

fn named_enigo_key(name: &str) -> Option<enigo::Key> {
    Some(match name {
        "Return" => enigo::Key::Return,
        "Backspace" => enigo::Key::Backspace,
        "Tab" => enigo::Key::Tab,
        "Space" => enigo::Key::Space,
        "Escape" => enigo::Key::Escape,
        "Delete" => enigo::Key::Delete,
        "UpArrow" => enigo::Key::UpArrow,
        "DownArrow" => enigo::Key::DownArrow,
        "LeftArrow" => enigo::Key::LeftArrow,
        "RightArrow" => enigo::Key::RightArrow,
        "Home" => enigo::Key::Home,
        "End" => enigo::Key::End,
        "PageUp" => enigo::Key::PageUp,
        "PageDown" => enigo::Key::PageDown,
        "Shift" => enigo::Key::Shift,
        "Control" => enigo::Key::Control,
        "Alt" => enigo::Key::Alt,
        "Meta" => enigo::Key::Meta,
        "CapsLock" => enigo::Key::CapsLock,
        "F1" => enigo::Key::F1,
        "F2" => enigo::Key::F2,
        "F3" => enigo::Key::F3,
        "F4" => enigo::Key::F4,
        "F5" => enigo::Key::F5,
        "F6" => enigo::Key::F6,
        "F7" => enigo::Key::F7,
        "F8" => enigo::Key::F8,
        "F9" => enigo::Key::F9,
        "F10" => enigo::Key::F10,
        "F11" => enigo::Key::F11,
        "F12" => enigo::Key::F12,
        _ => return None,
    })
}

fn map_enigo_error(message: &str) -> InputError {
    let lower = message.to_lowercase();
    if lower.contains("permission")
        || lower.contains("not authorized")
        || lower.contains("accessibility")
        || lower.contains("denied")
    {
        return InputError::PermissionDenied {
            detail: message.to_string(),
            guidance: InputError::permission_guidance(),
        };
    }
    InputError::Backend(message.to_string())
}

#[async_trait]
impl InputController for EnigoInput {
    async fn send_mouse(&self, event: MouseEvent) -> Result<(), InputError> {
        self.dispatch(Cmd::Mouse(event)).await
    }

    async fn send_keyboard(&self, event: KeyboardEvent) -> Result<(), InputError> {
        self.dispatch(Cmd::Keyboard(event)).await
    }

    async fn release_all(&self) -> Result<(), InputError> {
        self.dispatch(Cmd::ReleaseAll).await
    }

    fn backend_name(&self) -> &'static str {
        "enigo-native"
    }
}
