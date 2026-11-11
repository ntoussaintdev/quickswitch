//! Diagnostic: how to pair Explorer tab *paths* (Shell `ShellWindows`) with
//! the tab *strip order* (UI Automation). Read-only.
//!
//! For every shell browser it prints the top-level Explorer window, the
//! browser's own window (`ShellTabWindowClass`), its visibility and its folder.
//! For every Explorer window it prints the `ShellTabWindowClass` children in
//! `EnumChildWindows` order and the UIA tab names in strip order.
//!
//! Run with `cargo run --example tab_paths_spike`. Runs in an STA like the
//! app; `-- --mta` shows why an MTA doesn't work (`GetWindow` fails).

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::System::Com::{
    CLSCTX_ALL, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, IServiceProvider,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, TreeScope_Descendants, UIA_ControlTypePropertyId, UIA_TabItemControlTypeId,
};
use windows::Win32::UI::Shell::{
    IFolderView, IPersistFolder2, IShellBrowser, IShellWindows, IWebBrowserApp, SHGetNameFromIDList,
    SID_STopLevelBrowser, SIGDN_DESKTOPABSOLUTEPARSING, SIGDN_NORMALDISPLAY, ShellWindows,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GA_ROOT, GetAncestor, GetClassNameW, GetWindowTextW, IsWindowVisible,
};
use windows::core::{BOOL, Interface};

fn class_name(h: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn title(h: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

unsafe extern "system" fn collect_top(h: HWND, lparam: LPARAM) -> BOOL {
    let out = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
    if unsafe { IsWindowVisible(h) }.as_bool() && class_name(h) == "CabinetWClass" {
        out.push(h);
    }
    true.into()
}

unsafe extern "system" fn collect_tabs(h: HWND, lparam: LPARAM) -> BOOL {
    let out = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
    if class_name(h) == "ShellTabWindowClass" {
        out.push(h);
    }
    true.into()
}

fn folder_of(browser: &IShellBrowser) -> windows::core::Result<(String, String)> {
    unsafe {
        let view = browser.QueryActiveShellView()?;
        let fv: IFolderView = view.cast()?;
        let pf: IPersistFolder2 = fv.GetFolder()?;
        let pidl = pf.GetCurFolder()?;
        let parse = SHGetNameFromIDList(pidl, SIGDN_DESKTOPABSOLUTEPARSING)?;
        let display = SHGetNameFromIDList(pidl, SIGDN_NORMALDISPLAY)?;
        let out = (
            parse.to_string().unwrap_or_default(),
            display.to_string().unwrap_or_default(),
        );
        CoTaskMemFree(Some(parse.0 as _));
        CoTaskMemFree(Some(display.0 as _));
        CoTaskMemFree(Some(pidl as _));
        Ok(out)
    }
}

fn main() -> windows::core::Result<()> {
    let sta = !std::env::args().any(|a| a == "--mta");
    let model = if sta {
        COINIT_APARTMENTTHREADED
    } else {
        COINIT_MULTITHREADED
    };
    unsafe { CoInitializeEx(None, model).ok()? };
    println!("apartment: {}", if sta { "STA" } else { "MTA" });

    let started = std::time::Instant::now();
    let shell: IShellWindows = unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)? };
    let count = unsafe { shell.Count()? };
    println!("ShellWindows: {count} browsers");
    for i in 0..count {
        let disp = match unsafe { shell.Item(&VARIANT::from(i)) } {
            Ok(d) => d,
            Err(e) => {
                println!("  [{i}] Item failed: {e}");
                continue;
            }
        };
        let sp: IServiceProvider = match disp.cast() {
            Ok(s) => s,
            Err(e) => {
                println!("  [{i}] no IServiceProvider: {e}");
                continue;
            }
        };
        let browser: IShellBrowser = match unsafe { sp.QueryService(&SID_STopLevelBrowser) } {
            Ok(b) => b,
            Err(e) => {
                println!("  [{i}] no IShellBrowser: {e}");
                continue;
            }
        };
        let tab = unsafe { browser.GetWindow() };
        if let Err(e) = &tab {
            println!("  [{i}] GetWindow error: {e}");
        }
        let tab_hwnd = tab.unwrap_or_default();
        let app_hwnd = disp
            .cast::<IWebBrowserApp>()
            .and_then(|a| unsafe { a.HWND() })
            .map(|h| h.0)
            .unwrap_or(0);
        let top = if tab_hwnd.is_invalid() {
            HWND(app_hwnd as _)
        } else {
            unsafe { GetAncestor(tab_hwnd, GA_ROOT) }
        };
        let visible = unsafe { IsWindowVisible(tab_hwnd) }.as_bool();
        let folder = folder_of(&browser);
        println!(
            "  [{i}] app_hwnd={app_hwnd:#x} top={:#x} tab_hwnd={:#x} class={:?} visible={visible} folder={folder:?}",
            top.0 as isize,
            tab_hwnd.0 as isize,
            class_name(tab_hwnd)
        );
    }
    println!("ShellWindows pass took {:?}", started.elapsed());

    let uia: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)? };
    let cond =
        unsafe { uia.CreatePropertyCondition(UIA_ControlTypePropertyId, &VARIANT::from(UIA_TabItemControlTypeId.0))? };
    let mut tops: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect_top), LPARAM(&mut tops as *mut _ as isize));
    }
    for top in tops {
        println!("\nExplorer {:#x} {:?}", top.0 as isize, title(top));
        let mut tabs: Vec<HWND> = Vec::new();
        unsafe {
            let _ = EnumChildWindows(Some(top), Some(collect_tabs), LPARAM(&mut tabs as *mut _ as isize));
        }
        for t in &tabs {
            println!(
                "  ShellTabWindowClass {:#x} visible={} text={:?}",
                t.0 as isize,
                unsafe { IsWindowVisible(*t) }.as_bool(),
                title(*t)
            );
        }
        let el = unsafe { uia.ElementFromHandle(top)? };
        let found = unsafe { el.FindAll(TreeScope_Descendants, &cond)? };
        for i in 0..unsafe { found.Length()? } {
            let e = unsafe { found.GetElement(i)? };
            let name = unsafe { e.CurrentName() }.map(|b| b.to_string()).unwrap_or_default();
            let aid = unsafe { e.CurrentAutomationId() }
                .map(|b| b.to_string())
                .unwrap_or_default();
            let help = unsafe { e.CurrentHelpText() }
                .map(|b| b.to_string())
                .unwrap_or_default();
            let rid = unsafe { e.GetRuntimeId() }.ok();
            println!("  UIA tab [{i}] name={name:?} aid={aid:?} help={help:?} runtime_id={rid:?}");
        }
    }
    Ok(())
}
