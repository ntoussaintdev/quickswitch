//! Explorer tab discovery and selection on a dedicated COM MTA thread
//! (plan steps 1, 2 and 10).
//!
//! Verified on Windows 11 (Phase 0 spike, `examples/uia_spike.rs`): each
//! Explorer tab is a UIA `TabItem` (class `ListViewItem`) under
//! `CabinetWClass` → `Microsoft.UI.Content.DesktopChildSiteBridge` →
//! `TabView` → `TabListView`, and supports `SelectionItemPattern`.
//!
//! COM objects never leave this thread. The UI thread gets plain `TabInfo`
//! records and refers to tabs by an ephemeral token.

use std::collections::HashMap;
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender};

use super::TabInfo;
use crate::command::{AppCommand, CommandSender};

pub enum Request {
    /// Scan these Explorer windows. Older pending scans are dropped.
    Scan {
        generation: u64,
        windows: Vec<isize>,
    },
    /// Select a tab. If the token is stale, the worker rescans that window
    /// and looks the tab up by name and position.
    Select {
        explorer_hwnd: isize,
        token: u64,
        name: String,
        index: usize,
    },
    Shutdown,
}

pub struct TabWorker {
    tx: Sender<Request>,
    thread: Option<JoinHandle<()>>,
}

impl TabWorker {
    pub fn start(sender: CommandSender) -> TabWorker {
        let (tx, rx) = crossbeam_channel::unbounded();
        let thread = std::thread::Builder::new()
            .name("quickswitch-uia".into())
            .spawn(move || imp::run(rx, sender))
            .expect("spawn UIA worker");
        TabWorker {
            tx,
            thread: Some(thread),
        }
    }

    pub fn request(&self, req: Request) {
        let _ = self.tx.send(req);
    }

    /// Stop the worker. Waits briefly; a worker stuck inside a hung Explorer
    /// call is abandoned rather than blocking exit.
    pub fn shutdown(&mut self) {
        let _ = self.tx.send(Request::Shutdown);
        if let Some(t) = self.thread.take() {
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
            while !t.is_finished() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            if t.is_finished() {
                let _ = t.join();
            }
        }
    }
}

impl Drop for TabWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

mod imp {
    use super::*;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    };
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomation2, IUIAutomationCondition, IUIAutomationElement,
        IUIAutomationSelectionItemPattern, TreeScope_Descendants, UIA_ControlTypePropertyId,
        UIA_SelectionItemPatternId, UIA_TabItemControlTypeId,
    };
    use windows::Win32::UI::WindowsAndMessaging::EnumChildWindows;
    use windows::core::{BOOL, Interface};

    use crate::os::windows::{class_name, hwnd, is_window};

    /// Child window hosting the XAML tab strip.
    const TAB_HOST_CLASS: &str = "Microsoft.UI.Content.DesktopChildSiteBridge";
    /// Cap so one hung Explorer can't stall the worker for long.
    const UIA_TIMEOUT_MS: u32 = 1500;

    struct Uia {
        automation: IUIAutomation,
        tab_condition: IUIAutomationCondition,
    }

    struct Worker {
        uia: Uia,
        next_token: u64,
        /// Elements from the latest scan, by token.
        elements: HashMap<u64, IUIAutomationElement>,
    }

    pub fn run(rx: Receiver<Request>, sender: CommandSender) {
        if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
            crate::os::log("UIA worker: CoInitializeEx failed; Explorer tabs disabled");
            drain_without_uia(rx, sender);
            return;
        }
        match create_uia() {
            Ok(uia) => {
                let mut worker = Worker {
                    uia,
                    next_token: 1,
                    elements: HashMap::new(),
                };
                worker.serve(&rx, &sender);
            }
            Err(e) => {
                crate::os::log(&format!("UIA worker: could not create CUIAutomation: {e}"));
                drain_without_uia(rx, sender);
            }
        }
        unsafe { CoUninitialize() };
    }

    /// Keep answering so the UI never waits forever, just with no tabs.
    fn drain_without_uia(rx: Receiver<Request>, sender: CommandSender) {
        for req in rx.iter() {
            match req {
                Request::Scan { generation, .. } => sender.send(AppCommand::Tabs {
                    generation,
                    tabs: Vec::new(),
                }),
                Request::Select { explorer_hwnd, .. } => sender.send(AppCommand::TabSelected {
                    explorer_hwnd,
                    result: Err("Explorer tabs are unavailable".into()),
                }),
                Request::Shutdown => break,
            }
        }
    }

    fn create_uia() -> windows::core::Result<Uia> {
        let automation: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)? };
        if let Ok(a2) = automation.cast::<IUIAutomation2>() {
            unsafe {
                let _ = a2.SetConnectionTimeout(UIA_TIMEOUT_MS);
                let _ = a2.SetTransactionTimeout(UIA_TIMEOUT_MS);
            }
        }
        let tab_condition = unsafe {
            automation.CreatePropertyCondition(UIA_ControlTypePropertyId, &VARIANT::from(UIA_TabItemControlTypeId.0))?
        };
        Ok(Uia {
            automation,
            tab_condition,
        })
    }

    impl Worker {
        fn serve(&mut self, rx: &Receiver<Request>, sender: &CommandSender) {
            while let Ok(mut req) = rx.recv() {
                // Debounce: if several scans queued up, only run the newest.
                if matches!(req, Request::Scan { .. }) {
                    while let Ok(next) = rx.try_recv() {
                        let is_scan = matches!(next, Request::Scan { .. });
                        req = next;
                        if !is_scan {
                            break;
                        }
                    }
                }
                match req {
                    Request::Scan { generation, windows } => {
                        let tabs = self.scan(&windows);
                        sender.send(AppCommand::Tabs { generation, tabs });
                    }
                    Request::Select {
                        explorer_hwnd,
                        token,
                        name,
                        index,
                    } => {
                        let result = self.select(explorer_hwnd, token, &name, index);
                        sender.send(AppCommand::TabSelected { explorer_hwnd, result });
                    }
                    Request::Shutdown => break,
                }
            }
            self.elements.clear();
        }

        fn scan(&mut self, windows: &[isize]) -> Vec<TabInfo> {
            self.elements.clear();
            let mut out = Vec::new();
            for &w in windows {
                match self.tabs_of(w) {
                    Ok(elements) => {
                        let count = elements.len();
                        for (index, (el, name, selected)) in elements.into_iter().enumerate() {
                            let token = self.next_token;
                            self.next_token += 1;
                            self.elements.insert(token, el);
                            out.push(TabInfo {
                                explorer_hwnd: w,
                                token,
                                name,
                                index,
                                count,
                                selected,
                            });
                        }
                    }
                    Err(e) => crate::os::log(&format!("UIA scan of {w:#x} failed: {e}")),
                }
            }
            out
        }

        /// Tab elements of one Explorer window, in strip order.
        fn tabs_of(&self, explorer: isize) -> windows::core::Result<Vec<(IUIAutomationElement, String, bool)>> {
            if !is_window(explorer) {
                return Ok(Vec::new());
            }
            // Search only the tab-strip host windows, not the (possibly huge)
            // file list. Fall back to the whole window if none is found.
            let mut roots: Vec<HWND> = tab_hosts(hwnd(explorer));
            if roots.is_empty() {
                roots.push(hwnd(explorer));
            }
            let mut out = Vec::new();
            for root in roots {
                let element = unsafe { self.uia.automation.ElementFromHandle(root)? };
                let found = unsafe { element.FindAll(TreeScope_Descendants, &self.uia.tab_condition)? };
                for i in 0..unsafe { found.Length()? } {
                    let el = unsafe { found.GetElement(i)? };
                    let name = unsafe { el.CurrentName() }.map(|b| b.to_string()).unwrap_or_default();
                    if name.trim().is_empty() {
                        continue;
                    }
                    let selected = unsafe {
                        el.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId)
                            .and_then(|p| p.CurrentIsSelected())
                            .map(|b| b.as_bool())
                            .unwrap_or(false)
                    };
                    out.push((el, name, selected));
                }
                if !out.is_empty() {
                    break;
                }
            }
            Ok(out)
        }

        fn select(&mut self, explorer: isize, token: u64, name: &str, index: usize) -> Result<(), String> {
            if !is_window(explorer) {
                return Err("That Explorer window has closed.".into());
            }
            // Fast path: the element from the last scan, if it still has the same name.
            if let Some(el) = self.elements.get(&token) {
                let still_same = unsafe { el.CurrentName() }.map(|b| b.to_string()).ok().as_deref() == Some(name);
                if still_same && select_element(el).is_ok() {
                    return Ok(());
                }
            }
            // Slow path: rescan this window and pick the same-named tab
            // closest to the original position.
            let tabs = self
                .tabs_of(explorer)
                .map_err(|e| format!("Could not read Explorer tabs: {e}"))?;
            let best = tabs
                .iter()
                .enumerate()
                .filter(|(_, (_, n, _))| n == name)
                .min_by_key(|(i, _)| i.abs_diff(index))
                .map(|(_, (el, _, _))| el.clone());
            match best {
                Some(el) => select_element(&el).map_err(|e| format!("Could not select the tab: {e}")),
                None => Err(format!("The tab “{name}” is no longer open.")),
            }
        }
    }

    fn select_element(el: &IUIAutomationElement) -> windows::core::Result<()> {
        unsafe {
            let p = el.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId)?;
            p.Select()
        }
    }

    unsafe extern "system" fn collect_hosts(h: HWND, lparam: LPARAM) -> BOOL {
        let out = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
        if class_name(h) == TAB_HOST_CLASS {
            out.push(h);
        }
        true.into()
    }

    fn tab_hosts(explorer: HWND) -> Vec<HWND> {
        let mut out: Vec<HWND> = Vec::new();
        unsafe {
            let _ = EnumChildWindows(Some(explorer), Some(collect_hosts), LPARAM(&mut out as *mut _ as isize));
        }
        out
    }
}
