//! Phase 0 diagnostic (plan §6.1): list File Explorer windows and the tab items
//! UI Automation exposes for each one.
//!
//! Run with `cargo run --example uia_spike`. Read-only by default.
//! Pass `--select <window-index> <tab-index>` to try SelectionItemPattern::Select
//! on one tab and bring its Explorer window to the front (plan §6.1 step 2).

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationSelectionItemPattern, TreeScope_Descendants, UIA_ControlTypePropertyId,
    UIA_SelectionItemPatternId, UIA_TabItemControlTypeId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowTextW, IsWindowVisible, SetForegroundWindow,
};
use windows::core::BOOL;

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn title(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let out = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
    if unsafe { IsWindowVisible(hwnd) }.as_bool() && class_name(hwnd) == "CabinetWClass" {
        out.push(hwnd);
    }
    true.into()
}

fn main() -> windows::core::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let select: Option<(usize, usize)> = if args.get(1).map(String::as_str) == Some("--select") {
        Some((
            args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0),
            args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0),
        ))
    } else {
        None
    };

    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
    let uia: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)? };
    let cond =
        unsafe { uia.CreatePropertyCondition(UIA_ControlTypePropertyId, &VARIANT::from(UIA_TabItemControlTypeId.0))? };

    let mut windows: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut windows as *mut _ as isize));
    }
    println!("Explorer windows (CabinetWClass): {}", windows.len());

    for (wi, hwnd) in windows.iter().enumerate() {
        println!("\n[{wi}] hwnd={:?} title={:?}", hwnd.0, title(*hwnd));
        let started = std::time::Instant::now();
        let element = unsafe { uia.ElementFromHandle(*hwnd)? };
        let tabs = unsafe { element.FindAll(TreeScope_Descendants, &cond)? };
        let count = unsafe { tabs.Length()? };
        println!("  tab items: {count} (scan took {:?})", started.elapsed());
        for ti in 0..count {
            let tab = unsafe { tabs.GetElement(ti)? };
            let name = unsafe { tab.CurrentName() }.map(|b| b.to_string()).unwrap_or_default();
            let class = unsafe { tab.CurrentClassName() }
                .map(|b| b.to_string())
                .unwrap_or_default();
            let pattern =
                unsafe { tab.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId) };
            let selected = pattern
                .as_ref()
                .ok()
                .and_then(|p| unsafe { p.CurrentIsSelected() }.ok())
                .map(|b| b.as_bool());
            println!(
                "  - [{ti}] name={name:?} class={class:?} selection_pattern={} selected={selected:?}",
                pattern.is_ok()
            );
            if ti == 0 && args.iter().any(|a| a == "--tree") {
                let walker = unsafe { uia.ControlViewWalker()? };
                let mut cur = tab.clone();
                let mut depth = 0;
                while let Ok(parent) = unsafe { walker.GetParentElement(&cur) } {
                    depth += 1;
                    let n = unsafe { parent.CurrentName() }
                        .map(|b| b.to_string())
                        .unwrap_or_default();
                    let c = unsafe { parent.CurrentClassName() }
                        .map(|b| b.to_string())
                        .unwrap_or_default();
                    let t = unsafe { parent.CurrentControlType() }.map(|t| t.0).unwrap_or_default();
                    let h = unsafe { parent.CurrentNativeWindowHandle() }
                        .map(|h| h.0 as isize)
                        .unwrap_or_default();
                    let aid = unsafe { parent.CurrentAutomationId() }
                        .map(|b| b.to_string())
                        .unwrap_or_default();
                    println!("      ^{depth} type={t} class={c:?} aid={aid:?} hwnd={h:#x} name={n:?}");
                    if h == hwnd.0 as isize {
                        break;
                    }
                    cur = parent;
                }
            }
            if select == Some((wi, ti as usize)) {
                match pattern {
                    Ok(p) => {
                        let r = unsafe { p.Select() };
                        let fg = unsafe { SetForegroundWindow(*hwnd) }.as_bool();
                        println!("    -> Select(): {r:?}; SetForegroundWindow: {fg}");
                    }
                    Err(e) => println!("    -> no SelectionItemPattern: {e}"),
                }
            }
        }
    }
    Ok(())
}
