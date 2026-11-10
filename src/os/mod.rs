//! Windows integration: tray, hotkey, windows, single instance, diagnostics.

pub mod hotkey;
pub mod instance;
pub mod tray;
pub mod windows;

use std::io::Write;
use std::path::PathBuf;

const LOG_LIMIT: u64 = 256 * 1024;

fn log_path() -> Option<PathBuf> {
    if let Some(dir) = crate::config::portable_dir() {
        return Some(dir.join("quickswitch.log"));
    }
    std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("QuickSwitch").join("quickswitch.log"))
}

/// Append a line to `%APPDATA%\QuickSwitch\quickswitch.log` (kept under
/// 256 KB). The app has no console, so this is where problems are recorded.
pub fn log(msg: &str) {
    let Some(path) = log_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_LIMIT) {
        let _ = std::fs::rename(&path, path.with_extension("log.old"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(f, "[{secs}] {msg}");
    }
    if cfg!(debug_assertions) {
        eprintln!("{msg}");
    }
}

/// Non-blocking warning dialog (runs on its own thread so the event loop,
/// tray and hotkey keep working while it is open).
pub fn warn(msg: String) {
    log(&msg);
    std::thread::spawn(move || {
        use ::windows::Win32::UI::WindowsAndMessaging::{MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MessageBoxW};
        use ::windows::core::{HSTRING, w};
        unsafe {
            MessageBoxW(
                None,
                &HSTRING::from(msg),
                w!("QuickSwitch"),
                MB_OK | MB_ICONWARNING | MB_SETFOREGROUND,
            );
        }
    });
}
