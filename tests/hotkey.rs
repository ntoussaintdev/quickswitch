//! Shortcut text parsing (plan step 5).

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use quickswitch::os::hotkey::parse;

#[test]
fn default_shortcut_parses() {
    let expected = HotKey::new(Some(Modifiers::CONTROL | Modifiers::META), Code::KeyK);
    assert_eq!(parse("Ctrl+Win+K").unwrap(), expected);
    assert_eq!(parse("ctrl + windows + k").unwrap(), expected);
    assert_eq!(parse("Control+Super+KeyK").unwrap(), expected);
}

#[test]
fn fallback_style_shortcuts_parse() {
    let hk = parse("Ctrl+Alt+Space").unwrap();
    assert_eq!(hk, HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space));
}

#[test]
fn bad_shortcuts_are_errors() {
    assert!(parse("Ctrl+Win").is_err());
    assert!(parse("Ctrl+K+J").is_err());
    assert!(parse("Hyper+K").is_err());
}
