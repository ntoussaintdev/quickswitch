//! T3 — settings: defaults, round-trip, invalid fallback, startup quoting.

use std::path::Path;

use quickswitch::config::{Config, DEFAULT_HOTKEY, MAX_RECENT};
use quickswitch::startup::run_command;

#[test]
fn defaults() {
    let c = Config::default();
    assert_eq!(c.hotkey, DEFAULT_HOTKEY);
    assert_eq!(c.fallback_hotkey, None);
    assert!(c.recent.is_empty());
    assert!((1..=20).contains(&c.max_results));
}

#[test]
fn empty_and_partial_files_use_defaults() {
    let (c, warn) = Config::from_toml("");
    assert_eq!(c, Config::default());
    assert!(warn.is_none());

    let (c, warn) = Config::from_toml("fallback_hotkey = \"Ctrl+Alt+Space\"\n");
    assert!(warn.is_none());
    assert_eq!(c.fallback_hotkey.as_deref(), Some("Ctrl+Alt+Space"));
    assert_eq!(c.hotkey, DEFAULT_HOTKEY);
}

#[test]
fn invalid_file_falls_back_with_warning() {
    let (c, warn) = Config::from_toml("max_results = \"lots\"");
    assert_eq!(c, Config::default());
    assert!(warn.unwrap().contains("invalid config"));
}

#[test]
fn out_of_range_values_are_clamped() {
    let (c, _) = Config::from_toml("max_results = 500");
    assert_eq!(c.max_results, 20);
    let (c, _) = Config::from_toml("max_results = 0");
    assert_eq!(c.max_results, 1);
}

#[test]
fn save_load_round_trip() {
    let dir = std::env::temp_dir().join(format!("quickswitch-cfg-{}", std::process::id()));
    let path = dir.join("nested").join("config.toml");
    let mut c = Config {
        fallback_hotkey: Some("Ctrl+Alt+K".into()),
        ..Default::default()
    };
    c.push_recent("win:code|x");
    c.push_recent("tab:plans");
    c.save(&path).unwrap();
    let (loaded, warn) = Config::load(&path);
    assert!(warn.is_none());
    assert_eq!(loaded, c);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn missing_file_is_silent_default() {
    let (c, warn) = Config::load(Path::new("Z:/definitely/not/here/config.toml"));
    assert_eq!(c, Config::default());
    assert!(warn.is_none());
}

#[test]
fn recent_list_dedupes_and_caps() {
    let mut c = Config::default();
    c.push_recent("a");
    c.push_recent("b");
    c.push_recent("a");
    assert_eq!(c.recent, vec!["a", "b"]);
    for i in 0..(MAX_RECENT + 10) {
        c.push_recent(&format!("k{i}"));
    }
    assert_eq!(c.recent.len(), MAX_RECENT);
    assert_eq!(c.recent[0], format!("k{}", MAX_RECENT + 9));
}

#[test]
fn startup_command_is_quoted_with_background_flag() {
    let exe = Path::new(r"C:\Users\Some User\AppData\Local\Programs\QuickSwitch\QuickSwitch.exe");
    assert_eq!(
        run_command(exe),
        r#""C:\Users\Some User\AppData\Local\Programs\QuickSwitch\QuickSwitch.exe" --background"#
    );
}

#[test]
fn remembered_folders_are_mru_and_deduplicated() {
    let mut c = Config::default();
    c.push_folder(Path::new(r"C:\a"));
    c.push_folder(Path::new(r"C:\b"));
    c.push_folder(Path::new(r"c:\A"));
    assert_eq!(c.folders, vec![r"c:\A", r"C:\b"]);
    // Folders seen open in Explorer are added once and keep known ones in place.
    assert!(c.note_folders([Path::new(r"C:\new"), Path::new(r"C:\B")]));
    assert_eq!(c.folders, vec![r"C:\new", r"c:\A", r"C:\b"]);
    assert!(!c.note_folders([Path::new(r"C:\new")]));
    c.forget_folder(Path::new(r"C:\NEW"));
    assert_eq!(c.folders.len(), 2);
    // Survives a save/load round trip.
    let (back, warning) = Config::from_toml(&c.to_toml());
    assert!(warning.is_none());
    assert_eq!(back.folders, c.folders);
}

#[test]
fn browser_tabs_default_on_and_old_files_keep_it_on() {
    assert!(Config::default().browser_tabs);
    let (c, warn) = Config::from_toml("hotkey = \"Ctrl+Win+K\"\nrecent = []\n");
    assert!(warn.is_none());
    assert!(c.browser_tabs);
    let (c, _) = Config::from_toml("browser_tabs = false\n");
    assert!(!c.browser_tabs);
}
