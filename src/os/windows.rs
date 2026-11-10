//! Top-level window enumeration, activation and popup placement (plan steps 8–9).

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMWA_CLOAKED, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmGetWindowAttribute,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, MonitorFromWindow,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GA_ROOTOWNER, GW_OWNER, GWL_EXSTYLE, GetAncestor, GetClassNameW, GetCursorPos, GetForegroundWindow,
    GetLastActivePopup, GetWindow, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    HWND_TOP, IsIconic, IsWindow, IsWindowVisible, SW_HIDE, SW_RESTORE, SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER,
    SetForegroundWindow, SetWindowPos, ShowWindow, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, PWSTR};

use crate::search::WindowInfo;

/// Window classes that are never useful switch targets.
const IGNORED_CLASSES: &[&str] = &[
    "Progman",                    // desktop
    "WorkerW",                    // desktop
    "Shell_TrayWnd",              // taskbar
    "Shell_SecondaryTrayWnd",     // taskbar on other monitors
    "Windows.UI.Core.CoreWindow", // shell flyouts (Start, Search, ...)
    "XamlExplorerHostIslandWindow",
];

pub fn hwnd(h: isize) -> HWND {
    HWND(h as *mut core::ffi::c_void)
}

fn window_text(h: HWND) -> String {
    let len = unsafe { GetWindowTextLengthW(h) };
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize + 1];
    let n = unsafe { GetWindowTextW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

pub fn class_name(h: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn process_name(pid: u32) -> String {
    let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return String::new();
    };
    let mut buf = [0u16; 1024];
    let mut size = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut size) };
    unsafe {
        let _ = CloseHandle(handle);
    }
    if ok.is_err() {
        return String::new();
    }
    let path = String::from_utf16_lossy(&buf[..size as usize]);
    std::path::Path::new(&path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn is_cloaked(h: HWND) -> bool {
    let mut cloaked: u32 = 0;
    let r = unsafe {
        DwmGetWindowAttribute(
            h,
            DWMWA_CLOAKED,
            (&mut cloaked as *mut u32).cast(),
            size_of::<u32>() as u32,
        )
    };
    r.is_ok() && cloaked != 0
}

/// Mirrors the Alt+Tab rules: a visible, uncloaked, titled window that is its
/// own root owner's last active popup, and not a tool window.
fn is_switchable(h: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(h).as_bool() || is_cloaked(h) {
            return false;
        }
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TOOLWINDOW.0 != 0 && ex & WS_EX_APPWINDOW.0 == 0 {
            return false;
        }
        if ex & WS_EX_NOACTIVATE.0 != 0 && ex & WS_EX_APPWINDOW.0 == 0 {
            return false;
        }
        let owned = GetWindow(h, GW_OWNER).is_ok_and(|o| !o.is_invalid());
        if owned && ex & WS_EX_APPWINDOW.0 == 0 {
            return false;
        }
        // Walk to the last active popup of the root owner, like Alt+Tab does.
        let root = GetAncestor(h, GA_ROOTOWNER);
        if !root.is_invalid() && root != h {
            let popup = GetLastActivePopup(root);
            if popup != h && IsWindowVisible(popup).as_bool() {
                return false;
            }
        }
    }
    !window_text(h).trim().is_empty()
}

struct EnumState {
    own_pid: u32,
    out: Vec<WindowInfo>,
}

unsafe extern "system" fn enum_proc(h: HWND, lparam: LPARAM) -> BOOL {
    let state = unsafe { &mut *(lparam.0 as *mut EnumState) };
    if is_switchable(h) {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        let class = class_name(h);
        if pid != state.own_pid && !IGNORED_CLASSES.contains(&class.as_str()) {
            state.out.push(WindowInfo {
                hwnd: h.0 as isize,
                title: window_text(h),
                process: process_name(pid),
                class_name: class,
            });
        }
    }
    true.into()
}

/// Switchable top-level windows on the current virtual desktop, in Z-order
/// (most recently active first). Excludes QuickSwitch's own windows.
pub fn enumerate() -> Vec<WindowInfo> {
    let mut state = EnumState {
        own_pid: unsafe { GetCurrentProcessId() },
        out: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut state as *mut EnumState as isize));
    }
    state.out
}

pub fn is_window(h: isize) -> bool {
    h != 0 && unsafe { IsWindow(Some(hwnd(h))) }.as_bool()
}

/// Restore (if minimized) and bring a window to the foreground.
///
/// Respects Windows' foreground rules: no simulated keystrokes or input
/// attachment. QuickSwitch is the foreground app at this point (the user just
/// pressed Enter in it), which is what allows the switch.
pub fn activate(h: isize) -> Result<(), String> {
    if !is_window(h) {
        return Err("That window has closed.".into());
    }
    let w = hwnd(h);
    unsafe {
        if IsIconic(w).as_bool() {
            let _ = ShowWindow(w, SW_RESTORE);
        }
        if SetForegroundWindow(w).as_bool() {
            Ok(())
        } else {
            Err("Windows did not allow switching to that window.".into())
        }
    }
}

/// Work area (physical pixels) and DPI scale of the monitor the user is
/// looking at: the foreground window's monitor, or the cursor's.
pub fn active_monitor() -> (RECT, f32) {
    unsafe {
        let fg = GetForegroundWindow();
        let monitor = if !fg.is_invalid() {
            MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST)
        } else {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST)
        };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        (info.rcWork, dx as f32 / 96.0)
    }
}

/// Size the popup for `work`'s DPI and center it horizontally, a bit above
/// the vertical middle (like Spotlight / PowerToys Run).
pub fn place(h: isize, work: RECT, scale: f32, logical_size: (f32, f32)) {
    let w = (logical_size.0 * scale).round() as i32;
    let ht = (logical_size.1 * scale).round() as i32;
    let x = work.left + ((work.right - work.left) - w) / 2;
    let y = work.top + ((work.bottom - work.top) - ht) * 2 / 5;
    unsafe {
        let _ = SetWindowPos(hwnd(h), Some(HWND_TOP), x, y, w, ht, SWP_NOACTIVATE | SWP_NOZORDER);
    }
}

/// Show and focus the popup. Returns whether it became the foreground window.
pub fn show_and_focus(h: isize) -> bool {
    let w = hwnd(h);
    unsafe {
        let _ = ShowWindow(w, SW_SHOW);
        SetForegroundWindow(w).as_bool()
    }
}

pub fn hide(h: isize) {
    unsafe {
        let _ = ShowWindow(hwnd(h), SW_HIDE);
    }
}

pub fn is_foreground(h: isize) -> bool {
    unsafe { GetForegroundWindow() }.0 as isize == h
}

/// Ask Windows 11 for rounded corners on our undecorated popup.
pub fn round_corners(h: isize) {
    let pref = DWMWCP_ROUND;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd(h),
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&pref as *const DWM_WINDOW_CORNER_PREFERENCE).cast(),
            size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );
    }
}
