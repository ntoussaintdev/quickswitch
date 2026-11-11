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

use quickswitch::os::hotkey::{KEYS, Mod, Shortcut};

fn shortcut(mods: [Option<Mod>; 3], key: &'static str) -> Shortcut {
    Shortcut { mods, key }
}

#[test]
fn every_offered_key_makes_a_valid_shortcut() {
    for &key in KEYS {
        let text = shortcut([Some(Mod::Ctrl), Some(Mod::Alt), None], key)
            .to_text()
            .unwrap_or_else(|e| panic!("{key:?}: {e}"));
        assert!(parse(&text).is_ok(), "{text}");
        // The text reads back into the same dropdown choices.
        assert_eq!(Shortcut::from_text(&text).map(|s| s.key), Some(key));
    }
}

#[test]
fn three_modifiers_are_written_in_a_fixed_order() {
    let s = shortcut([Some(Mod::Win), Some(Mod::Shift), Some(Mod::Ctrl)], "K");
    assert_eq!(s.to_text().unwrap(), "Ctrl+Shift+Win+K");
    assert_eq!(
        parse("Ctrl+Shift+Win+K").unwrap(),
        HotKey::new(
            Some(Modifiers::CONTROL | Modifiers::SHIFT | Modifiers::META),
            Code::KeyK
        )
    );
    // Gaps between dropdowns don't matter.
    let s = shortcut([None, Some(Mod::Alt), None], "Space");
    assert_eq!(s.to_text().unwrap(), "Alt+Space");
}

#[test]
fn unusable_combinations_are_explained() {
    assert!(
        shortcut([None, None, None], "K")
            .to_text()
            .unwrap_err()
            .contains("at least one")
    );
    assert_eq!(
        shortcut([Some(Mod::Ctrl), Some(Mod::Ctrl), None], "K")
            .to_text()
            .unwrap_err(),
        "Ctrl is picked twice"
    );
    assert!(
        shortcut([Some(Mod::Shift), None, None], "K")
            .to_text()
            .unwrap_err()
            .contains("Shift alone")
    );
    assert!(
        shortcut([Some(Mod::Shift), Some(Mod::Alt), None], "K")
            .to_text()
            .is_ok()
    );
}

#[test]
fn config_text_reads_back_into_dropdowns() {
    assert_eq!(Shortcut::from_text("Ctrl+Win+K"), Some(Shortcut::default()));
    assert_eq!(
        Shortcut::from_text("control + windows + KeyK"),
        Some(Shortcut::default())
    );
    assert_eq!(
        Shortcut::from_text("Ctrl+Alt+Digit1"),
        Some(shortcut([Some(Mod::Ctrl), Some(Mod::Alt), None], "1"))
    );
    assert_eq!(Shortcut::from_text("alt+space").map(|s| s.key), Some("Space"));
    // Not representable: four modifiers, unknown names, keys not offered.
    assert_eq!(Shortcut::from_text("Ctrl+Alt+Shift+Win+K"), None);
    assert_eq!(Shortcut::from_text("Hyper+K"), None);
    assert_eq!(Shortcut::from_text("Ctrl+PageUp"), None);
    assert_eq!(Shortcut::from_text(""), None);
}
