//! Explorer tab discovery and switching. Runs on the worker thread (`worker`).
//!
//! Findings from the diagnostics in `examples/` (Windows 11, Nov 2026):
//! - The Shell's `ShellWindows` collection lists every Explorer *tab* as its
//!   own browser. Through `IShellBrowser` each one gives its folder and its
//!   own `ShellTabWindowClass` window. This works in a single-threaded
//!   apartment; from a multi-threaded one `GetWindow` fails with
//!   RPC_E_CANTCALLOUT_ININPUTSYNCCALL. So the worker thread is an STA and
//!   pumps messages while it waits.
//! - `ShellWindows` order is creation order, not tab-strip order.
//! - The shown tab's `ShellTabWindowClass` is always the first one in its
//!   Explorer window's child Z-order (agrees with UI Automation's
//!   `IsSelected`; the window title can lag behind).
//! - UI Automation exposes each tab as a `TabItem` with the folder's display
//!   name and `SelectionItemPattern`, but no path. Switching uses it: find the
//!   items with the tab's name and select one. With several same-named tabs,
//!   the worker checks which tab window came to the front and tries the next
//!   candidate if it was the wrong one.
//!
//! COM objects never leave the worker thread. The UI thread gets plain
//! `TabInfo` records.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree, IServiceProvider};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{IUIAutomationElement, TreeScope_Descendants};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    IFolderView, IPersistFolder2, IShellBrowser, IShellWindows, SHGetNameFromIDList, SID_STopLevelBrowser, SIGDN,
    SIGDN_FILESYSPATH, SIGDN_NORMALDISPLAY, ShellWindows,
};
use windows::Win32::UI::WindowsAndMessaging::{GA_ROOT, GetAncestor};
use windows::core::Interface;

use super::TabInfo;
use super::uia::{self, Uia};
use crate::os::windows::{
    active_explorer_tab, class_name, descendants_of_class, explorer_tabs_in_z_order, hwnd, is_window,
};

/// Child window hosting the XAML tab strip.
const TAB_HOST_CLASS: &str = "Microsoft.UI.Content.DesktopChildSiteBridge";
/// How long to wait for the chosen tab to come to the front.
const CONFIRM_MS: u64 = 500;
/// How long to wait for a restored (was minimized) window's tab strip.
const TAB_STRIP_WAIT_MS: u64 = 1500;

// ----- scan -----------------------------------------------------------------

struct RawTab {
    explorer: isize,
    tab: isize,
    name: String,
    path: Option<std::path::PathBuf>,
}

pub fn scan_tabs() -> Vec<TabInfo> {
    let raw = match list_browsers() {
        Ok(r) => r,
        Err(e) => {
            crate::os::log(&format!("Reading Explorer tabs failed: {e}"));
            return Vec::new();
        }
    };
    let mut z_orders: std::collections::HashMap<isize, Vec<isize>> = std::collections::HashMap::new();
    let mut out: Vec<TabInfo> = Vec::with_capacity(raw.len());
    for (i, t) in raw.iter().enumerate() {
        let nth = raw[..i]
            .iter()
            .filter(|o| o.explorer == t.explorer && o.name == t.name)
            .count();
        let order = z_orders
            .entry(t.explorer)
            .or_insert_with(|| explorer_tabs_in_z_order(t.explorer));
        let z = order.iter().position(|&h| h == t.tab).unwrap_or(usize::MAX);
        out.push(TabInfo {
            explorer_hwnd: t.explorer,
            tab_hwnd: t.tab,
            name: t.name.clone(),
            path: t.path.clone(),
            nth,
            active: z == 0,
            z,
        });
    }
    out
}

fn list_browsers() -> windows::core::Result<Vec<RawTab>> {
    let shell: IShellWindows = unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)? };
    let count = unsafe { shell.Count()? };
    let mut out = Vec::new();
    for i in 0..count {
        // One bad entry (e.g. a window closing right now) shouldn't stop the scan.
        if let Ok(Some(t)) = browser_tab(&shell, i) {
            out.push(t);
        }
    }
    Ok(out)
}

fn browser_tab(shell: &IShellWindows, i: i32) -> windows::core::Result<Option<RawTab>> {
    unsafe {
        let disp = shell.Item(&VARIANT::from(i))?;
        let sp: IServiceProvider = disp.cast()?;
        let browser: IShellBrowser = sp.QueryService(&SID_STopLevelBrowser)?;
        let tab: HWND = browser.GetWindow()?;
        let explorer = GetAncestor(tab, GA_ROOT);
        if class_name(explorer) != "CabinetWClass" {
            return Ok(None);
        }
        let view = browser.QueryActiveShellView()?;
        let folder: IPersistFolder2 = view.cast::<IFolderView>()?.GetFolder()?;
        let pidl = folder.GetCurFolder()?;
        let name = idlist_name(pidl, SIGDN_NORMALDISPLAY).unwrap_or_default();
        let path = idlist_name(pidl, SIGDN_FILESYSPATH).map(std::path::PathBuf::from);
        CoTaskMemFree(Some(pidl as _));
        if name.is_empty() {
            return Ok(None);
        }
        Ok(Some(RawTab {
            explorer: explorer.0 as isize,
            tab: tab.0 as isize,
            name,
            path,
        }))
    }
}

fn idlist_name(pidl: *const ITEMIDLIST, kind: SIGDN) -> Option<String> {
    unsafe {
        let p = SHGetNameFromIDList(pidl, kind).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s.filter(|s| !s.is_empty())
    }
}

// ----- select ---------------------------------------------------------------

pub fn select(uia: &Uia, explorer: isize, tab: isize, name: &str, nth: usize) -> Result<(), String> {
    if !is_window(explorer) {
        return Err("That Explorer window has closed.".into());
    }
    if !is_window(tab) {
        return Err(format!("The tab “{name}” is no longer open."));
    }
    if active_explorer_tab(explorer) == Some(tab) {
        return Ok(());
    }

    // A window that was just restored from minimized needs a moment
    // before its tab strip appears in UI Automation.
    let deadline = Instant::now() + Duration::from_millis(TAB_STRIP_WAIT_MS);
    let candidates = loop {
        let found = tab_items(uia, explorer, name).map_err(|e| format!("Could not read Explorer tabs: {e}"))?;
        if !found.is_empty() || Instant::now() >= deadline {
            break found;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    if candidates.is_empty() {
        return Err(format!("Could not find the tab “{name}” in its window."));
    }

    // Try the likeliest same-named item first, then the others.
    let first = nth.min(candidates.len() - 1);
    let mut order = vec![first];
    order.extend((0..candidates.len()).filter(|&i| i != first));
    let single = candidates.len() == 1;
    for i in order {
        uia::select_element(&candidates[i]).map_err(|e| format!("Could not switch to the tab: {e}"))?;
        if single || wait_until_active(explorer, tab) {
            return Ok(());
        }
    }
    crate::os::log(&format!(
        "Switched to a tab named “{name}” but could not confirm it was the right one"
    ));
    Ok(())
}

fn wait_until_active(explorer: isize, tab: isize) -> bool {
    let deadline = Instant::now() + Duration::from_millis(CONFIRM_MS);
    loop {
        if active_explorer_tab(explorer) == Some(tab) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}

/// Tab items named `name` in one Explorer window, in strip order.
///
/// Searches only the tab-strip host windows, not the (possibly huge) file
/// list; falls back to the whole window if none is found.
fn tab_items(uia: &Uia, explorer: isize, name: &str) -> windows::core::Result<Vec<IUIAutomationElement>> {
    let mut roots = descendants_of_class(explorer, TAB_HOST_CLASS);
    if roots.is_empty() {
        roots.push(explorer);
    }
    let mut out = Vec::new();
    for root in roots {
        let element = unsafe { uia.automation.ElementFromHandle(hwnd(root))? };
        let found = unsafe { element.FindAll(TreeScope_Descendants, &uia.tab_condition)? };
        let len = unsafe { found.Length()? };
        for i in 0..len {
            let el = unsafe { found.GetElement(i)? };
            if uia::name(&el) == name {
                out.push(el);
            }
        }
        if len > 0 {
            break;
        }
    }
    Ok(out)
}
