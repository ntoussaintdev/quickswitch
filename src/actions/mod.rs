//! Actions taken on a selected result: switching windows/tabs and opening
//! folders, files and web addresses.

pub mod paths;
pub mod web;

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

/// Open a file with its default app, as double-clicking it in Explorer
/// would (programs and scripts run).
#[cfg(windows)]
pub fn open_file(path: &Path) -> Result<(), String> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow, SW_SHOWNORMAL};
    use windows::core::HSTRING;

    if !path.is_file() {
        return Err(format!("File not found: {}", path.display()));
    }
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    let file = HSTRING::from(path.as_os_str());
    // No verb: the file's default action, which is "open" for most types.
    let result = unsafe { ShellExecuteW(None, None, &file, None, None, SW_SHOWNORMAL) };
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

/// Show a file in File Explorer with it selected.
///
/// `SHOpenFolderAndSelectItems` reuses an Explorer window already showing
/// the folder. If it fails, `explorer.exe /select,"path"` is used instead
/// (Windows paths can't contain `"`, so the quoting is safe).
#[cfg(windows)]
pub fn reveal(path: &Path) -> Result<(), String> {
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
    use windows::Win32::UI::Shell::{ILCreateFromPathW, ILFree, SHOpenFolderAndSelectItems, ShellExecuteW};
    use windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow, SW_SHOWNORMAL};
    use windows::core::{HSTRING, w};

    if !path.exists() {
        return Err(format!("Not found: {}", path.display()));
    }
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    let file = HSTRING::from(path.as_os_str());
    let shown = unsafe {
        // The shell call needs COM on this thread; S_FALSE (already on) is fine.
        let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let pidl = ILCreateFromPathW(&file);
        let ok = !pidl.is_null() && SHOpenFolderAndSelectItems(pidl, None, 0).is_ok();
        if !pidl.is_null() {
            ILFree(Some(pidl));
        }
        if com.is_ok() {
            CoUninitialize();
        }
        ok
    };
    if shown {
        return Ok(());
    }
    let args = HSTRING::from(format!("/select,\"{}\"", path.display()));
    let result = unsafe { ShellExecuteW(None, w!("open"), w!("explorer.exe"), &args, None, SW_SHOWNORMAL) };
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err(format!(
            "Could not show {} (error {})",
            path.display(),
            result.0 as isize
        ))
    }
}

/// Open a web address in the default browser.
#[cfg(windows)]
pub fn open_url(url: &str) -> Result<(), String> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow, SW_SHOWNORMAL};
    use windows::core::{HSTRING, w};

    // Let the browser take the foreground.
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    let file = HSTRING::from(url);
    let result = unsafe { ShellExecuteW(None, w!("open"), &file, None, None, SW_SHOWNORMAL) };
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err(format!("Could not open the browser (error {})", result.0 as isize))
    }
}
