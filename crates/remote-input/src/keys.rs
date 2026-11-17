//! Key mapping between the wire format (`KeyboardEvent.key`, named after
//! browser `KeyboardEvent.key`) and the native backend.

use remote_protocol::{KeyAction, KeyboardEvent, Modifier, MouseAction, MouseButton, MouseEvent};

/// Named-key translation table (browser key name -> enigo key name).
/// Characters are passed through as unicode.
pub const NAMED_KEYS: &[(&str, &str)] = &[
    ("Enter", "Return"),
    ("Backspace", "Backspace"),
    ("Tab", "Tab"),
    ("Space", "Space"),
    ("Escape", "Escape"),
    ("Delete", "Delete"),
    ("ArrowUp", "UpArrow"),
    ("ArrowDown", "DownArrow"),
    ("ArrowLeft", "LeftArrow"),
    ("ArrowRight", "RightArrow"),
    ("Home", "Home"),
    ("End", "End"),
    ("PageUp", "PageUp"),
    ("PageDown", "PageDown"),
    ("Shift", "Shift"),
    ("Control", "Control"),
    ("Alt", "Alt"),
    ("Meta", "Meta"),
    ("CapsLock", "CapsLock"),
    ("F1", "F1"),
    ("F2", "F2"),
    ("F3", "F3"),
    ("F4", "F4"),
    ("F5", "F5"),
    ("F6", "F6"),
    ("F7", "F7"),
    ("F8", "F8"),
    ("F9", "F9"),
    ("F10", "F10"),
    ("F11", "F11"),
    ("F12", "F12"),
];

/// A key resolved for the native backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnigoKey {
    Char(char),
    Named(String),
}

/// Resolves a wire key string to a native key. Rejects multi-character
/// strings that are not known named keys (protocol normalization).
pub fn resolve_key(key: &str) -> Option<EnigoKey> {
    let mut chars = key.chars();
    let (first, rest) = (chars.next()?, chars.as_str());
    if rest.is_empty() {
        return Some(EnigoKey::Char(first));
    }
    NAMED_KEYS
        .iter()
        .find(|(wire, _)| *wire == key)
        .map(|(_, native)| EnigoKey::Named(native.to_string()))
}

/// Normalizes a browser-side keyboard event into the wire format.
/// `key` comes from the DOM `KeyboardEvent.key`; modifiers from
/// `event.ctrlKey/altKey/shiftKey/metaKey`.
pub fn key_from_browser(action: KeyAction, key: &str, modifiers: &[bool; 4]) -> KeyboardEvent {
    let [ctrl, alt, shift, meta] = modifiers;
    let mut mods = Vec::new();
    if *ctrl {
        mods.push(Modifier::Ctrl);
    }
    if *alt {
        mods.push(Modifier::Alt);
    }
    if *shift {
        mods.push(Modifier::Shift);
    }
    if *meta {
        mods.push(Modifier::Meta);
    }
    KeyboardEvent {
        action,
        key: key.to_string(),
        modifiers: mods,
    }
}

/// Builds a wire mouse event from normalized controller coordinates.
pub fn mouse_event(
    action: MouseAction,
    button: MouseButton,
    x: f64,
    y: f64,
    scroll_dx: f64,
    scroll_dy: f64,
) -> MouseEvent {
    MouseEvent {
        action,
        button,
        x,
        y,
        scroll_dx,
        scroll_dy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_chars_and_named_keys() {
        assert_eq!(resolve_key("a"), Some(EnigoKey::Char('a')));
        assert_eq!(resolve_key("A"), Some(EnigoKey::Char('A')));
        assert_eq!(resolve_key("Enter"), Some(EnigoKey::Named("Return".into())));
        assert_eq!(
            resolve_key("ArrowLeft"),
            Some(EnigoKey::Named("LeftArrow".into()))
        );
        assert_eq!(resolve_key("NotAKey"), None);
        assert_eq!(resolve_key(""), None);
    }

    #[test]
    fn browser_key_normalization() {
        let event = key_from_browser(KeyAction::Press, "Enter", &[true, false, true, false]);
        assert_eq!(event.key, "Enter");
        assert_eq!(event.modifiers, vec![Modifier::Ctrl, Modifier::Shift]);
    }
}
