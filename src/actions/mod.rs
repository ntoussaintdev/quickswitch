//! Actions taken on a selected result: switching windows/tabs and opening folders.

pub mod paths;

use std::path::Path;

/// Open a folder in File Explorer.
///
/// Uses `ShellExecuteW` with the path as its own argument, so there is no
/// command-line string building (commas, spaces and quotes are safe).
/// Used only when no open Explorer tab already shows the folder (the app
/// switches to that tab instead). Whether Explorer then opens a new window or
/// a new tab follows the user's Explorer settings.
#[cfg(windows)]
pub fn open_folder(path: &Path) -> Result<(), String> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow, SW_SHOWNORMAL};
    use windows::core::{HSTRING, w};

    if !path.is_dir() {
        return Err(format!("Folder not found: {}", path.display()));
    }
    // Let the Explorer process we are about to start take the foreground.
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    let file = HSTRING::from(path.as_os_str());
    let result = unsafe { ShellExecuteW(None, w!("open"), &file, None, None, SW_SHOWNORMAL) };
    // ShellExecuteW returns a value greater than 32 on success.
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err(format!(
            "Could not open {} (error {})",
            path.display(),
            result.0 as isize
        ))
    }
}
