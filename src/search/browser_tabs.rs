//! Browser tab discovery and switching through UI Automation (plan
//! 2026-11-11-n1 §4.2, §4.3). Runs on the worker thread.
//!
//! - Each browser window's tab strip is found by a breadth-first walk of its
//!   control view that never enters page content (`Document` elements below
//!   the browser's own UI), then remembered per window.
//! - The tab items are read in one cached call: name and selected state.
//! - Tab elements from the latest scan are kept here under a numeric key;
//!   rows carry the key. Switching uses that element, or finds the tab again
//!   by name and rank when the element has gone.
//! - Switching tries SelectionItem.Select, then the legacy default action,
//!   then Invoke, and confirms each by the tab's selected state or the window
//!   title. No simulated mouse clicks.
//!
//! The details (walk limits, which method works) are what the
//! `browser_tabs_spike` example reports; adjust them here.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use windows::Win32::UI::Accessibility::{
    IUIAutomationElement, IUIAutomationInvokePattern, IUIAutomationLegacyIAccessiblePattern, TreeScope_Descendants,
    UIA_ButtonControlTypeId, UIA_CONTROLTYPE_ID, UIA_DocumentControlTypeId, UIA_GroupControlTypeId,
    UIA_InvokePatternId, UIA_LegacyIAccessiblePatternId, UIA_PaneControlTypeId, UIA_SelectionItemIsSelectedPropertyId,
    UIA_SelectionItemPatternId, UIA_TabControlTypeId, UIA_TabItemControlTypeId, UIA_TitleBarControlTypeId,
    UIA_ToolBarControlTypeId, UIA_WindowControlTypeId,
};

use super::BrowserTabInfo;
use super::browsers::{self, BrowserWindow, Engine};
use super::uia::{self, Uia};
use crate::os::windows::{hwnd, is_window, window_title};

/// Most elements visited while looking for a tab strip.
const MAX_NODES: usize = 1500;
/// Deepest level searched below the window.
const MAX_DEPTH: usize = 30;
/// Wall-clock cap on finding one window's tab strip.
const FIND_BUDGET_MS: u64 = 2000;
/// How long to wait for a switch to show.
const CONFIRM_MS: u64 = 400;
/// How long to wait for a restored (was minimized) window's tab strip.
const TAB_STRIP_WAIT_MS: u64 = 1500;

/// From this depth on, a `Document` is page content and is not entered.
/// Chromium's own UI has no documents; Firefox's UI is itself a document
/// near the top of the tree, with pages as documents further down.
fn document_cutoff(engine: Engine) -> usize {
    match engine {
        Engine::Chromium => 1,
        Engine::Gecko => 3,
        Engine::Terminal => 1,
    }
}

/// Why switching failed. `gone` means the tab or window no longer exists,
/// so its row should go.
#[derive(Debug)]
pub struct SelectError {
    pub message: String,
    pub gone: bool,
}

#[derive(Default)]
pub struct BrowserReader {
    next_key: u64,
    /// Tab elements from the latest scan, by key.
    elements: HashMap<u64, IUIAutomationElement>,
    /// Tab strip element per window.
    strips: HashMap<isize, IUIAutomationElement>,
    engines: HashMap<isize, Engine>,
    /// Browsers whose tab strip could not be found (logged once each).
    logged: HashSet<&'static str>,
}

impl BrowserReader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a new scan: elements of the previous one are dropped (their rows
    /// belong to an older popup opening), and so are strips of windows that
    /// are no longer listed.
    pub fn begin_scan(&mut self, windows: &[BrowserWindow]) {
        self.elements.clear();
        self.strips.retain(|h, _| windows.iter().any(|w| w.hwnd == *h));
        self.engines.retain(|h, _| windows.iter().any(|w| w.hwnd == *h));
    }

    /// All tabs of one browser window, in strip order. Empty when the strip
    /// can't be read; the window then keeps its plain window row.
    pub fn scan_window(&mut self, uia: &Uia, w: &BrowserWindow) -> Vec<BrowserTabInfo> {
        self.engines.insert(w.hwnd, w.spec.engine);
        let items = match self.read_items(uia, w.hwnd, w.spec.engine) {
            Some(items) => items,
            None => {
                if self.logged.insert(w.spec.name) {
                    crate::os::log(&format!(
                        "Could not find the tab strip of a {} window; showing it as a window",
                        w.spec.name
                    ));
                }
                return Vec::new();
            }
        };

        let mut tabs: Vec<BrowserTabInfo> = Vec::with_capacity(items.len());
        for (el, raw, selected) in items {
            let title = browsers::clean_tab_name(w.spec.engine, &raw);
            if title.is_empty() {
                continue;
            }
            let nth = tabs.iter().filter(|t| t.raw_name == raw).count();
            self.next_key += 1;
            self.elements.insert(self.next_key, el);
            tabs.push(BrowserTabInfo {
                window_hwnd: w.hwnd,
                key: self.next_key,
                raw_name: raw,
                title,
                browser: String::new(),
                nth,
                selected,
                terminal: w.spec.engine == Engine::Terminal,
            });
        }

        // A browser that doesn't report the selected tab: the window title
        // starts with the shown tab's title.
        if !tabs.iter().any(|t| t.selected)
            && let Some(i) = tabs
                .iter()
                .enumerate()
                .filter(|(_, t)| w.title.starts_with(&t.title))
                .max_by_key(|(_, t)| t.title.len())
                .map(|(i, _)| i)
        {
            tabs[i].selected = true;
        }
        let active = tabs
            .iter()
            .find(|t| t.selected)
            .map(|t| t.title.clone())
            .unwrap_or_default();
        let profile = browsers::profile_from_title(w.spec, &w.title, &active);
        let shown = browsers::display_name(w.spec, profile.as_deref());
        let titles: Vec<&str> = tabs.iter().map(|t| t.title.as_str()).collect();
        let labels = browsers::tab_labels(w.spec, &shown, &titles);
        for (t, label) in tabs.iter_mut().zip(labels) {
            t.browser = label;
        }
        tabs
    }

    /// Tab items of a window's strip with their raw names and selected state.
    fn read_items(
        &mut self,
        uia: &Uia,
        window: isize,
        engine: Engine,
    ) -> Option<Vec<(IUIAutomationElement, String, bool)>> {
        if let Some(strip) = self.strips.get(&window)
            && let Some(items) = read_strip(uia, strip)
            && !items.is_empty()
        {
            return Some(items);
        }
        self.strips.remove(&window);
        let strip = find_strip(uia, window, engine)?;
        let items = read_strip(uia, &strip)?;
        self.strips.insert(window, strip);
        Some(items)
    }

    /// Switch `window` to the tab named `name` (raw name). Returns how it was
    /// done, for the log.
    pub fn select(
        &mut self,
        uia: &Uia,
        window: isize,
        key: u64,
        name: &str,
        nth: usize,
    ) -> Result<&'static str, SelectError> {
        let engine = self.engines.get(&window).copied().unwrap_or(Engine::Chromium);
        let title = browsers::clean_tab_name(engine, name);
        if !is_window(window) {
            return Err(SelectError {
                message: "That window has closed.".into(),
                gone: true,
            });
        }

        let mut candidates = Vec::new();
        if let Some(el) = self.elements.get(&key)
            && uia::name(el) == name
        {
            candidates.push(el.clone());
        }
        if candidates.is_empty() {
            // The kept element is gone (or never was): find the tab again.
            let deadline = Instant::now() + Duration::from_millis(TAB_STRIP_WAIT_MS);
            loop {
                if let Some(items) = self.read_items(uia, window, engine) {
                    candidates = items
                        .into_iter()
                        .filter(|(_, n, _)| n == name)
                        .map(|(el, _, _)| el)
                        .collect();
                }
                if !candidates.is_empty() || Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if candidates.len() > 1 {
                let first = nth.min(candidates.len() - 1);
                let el = candidates.remove(first);
                candidates.insert(0, el);
            }
        }
        if candidates.is_empty() {
            return Err(SelectError {
                message: format!("The tab “{title}” is no longer open."),
                gone: true,
            });
        }

        for el in &candidates {
            if uia::is_selected(el) {
                return Ok("already shown");
            }
            for (label, method) in METHODS {
                if method(el).is_ok() && confirm(el, window, &title) {
                    return Ok(label);
                }
            }
        }
        crate::os::log(&format!(
            "Asked a {engine:?} window to show a tab but could not confirm it switched"
        ));
        Ok("unconfirmed")
    }

    /// Diagnostic report for one window: the path to its tab strip, timings,
    /// and per tab the raw and cleaned names, selected state and the switch
    /// methods it supports. Used by the `browser_tabs_spike` example.
    pub fn diagnose(&mut self, uia: &Uia, w: &BrowserWindow) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        let t0 = Instant::now();
        let strip = find_strip(uia, w.hwnd, w.spec.engine);
        let find_ms = t0.elapsed().as_millis();
        let Some(strip) = strip else {
            let _ = writeln!(out, "  no tab strip found ({find_ms} ms); top of the tree:");
            for line in outline(uia, w.hwnd, 16) {
                let _ = writeln!(out, "    {line}");
            }
            return out;
        };
        let _ = writeln!(out, "  tab strip found in {find_ms} ms; path from the window down:");
        for (depth, step) in path_to(uia, &strip, w.hwnd).iter().enumerate() {
            let _ = writeln!(out, "    {}{step}", "  ".repeat(depth));
        }
        self.strips.remove(&w.hwnd);
        let t1 = Instant::now();
        let tabs = self.scan_window(uia, w);
        let _ = writeln!(
            out,
            "  {} tabs read in {} ms; shown as \u{201c}{}\u{201d}",
            tabs.len(),
            t1.elapsed().as_millis(),
            tabs.first().map(|t| t.browser.as_str()).unwrap_or("")
        );
        for (i, t) in tabs.iter().enumerate() {
            let patterns = self.elements.get(&t.key).map(supported).unwrap_or_default();
            let _ = writeln!(
                out,
                "  [{i:>2}] {} raw=\u{201c}{}\u{201d} title=\u{201c}{}\u{201d} nth={} patterns={patterns}",
                if t.selected { "*" } else { " " },
                t.raw_name,
                t.title,
                t.nth,
            );
        }
        out
    }
}

type Method = fn(&IUIAutomationElement) -> windows::core::Result<()>;

/// Ways to switch to a tab, in the order they are tried.
const METHODS: [(&str, Method); 3] = [
    ("SelectionItem.Select", uia::select_element),
    ("LegacyIAccessible.DoDefaultAction", default_action),
    ("Invoke", invoke),
];

fn default_action(el: &IUIAutomationElement) -> windows::core::Result<()> {
    unsafe {
        el.GetCurrentPatternAs::<IUIAutomationLegacyIAccessiblePattern>(UIA_LegacyIAccessiblePatternId)?
            .DoDefaultAction()
    }
}

fn invoke(el: &IUIAutomationElement) -> windows::core::Result<()> {
    unsafe {
        el.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)?
            .Invoke()
    }
}

fn supported(el: &IUIAutomationElement) -> String {
    let mut out = Vec::new();
    unsafe {
        if el.GetCurrentPattern(UIA_SelectionItemPatternId).is_ok() {
            out.push("SelectionItem");
        }
        if el.GetCurrentPattern(UIA_LegacyIAccessiblePatternId).is_ok() {
            out.push("LegacyIAccessible");
        }
        if el.GetCurrentPattern(UIA_InvokePatternId).is_ok() {
            out.push("Invoke");
        }
    }
    out.join(",")
}

/// Wait until the tab reports itself selected or the window title shows it.
fn confirm(el: &IUIAutomationElement, window: isize, title: &str) -> bool {
    let deadline = Instant::now() + Duration::from_millis(CONFIRM_MS);
    loop {
        if uia::is_selected(el) || window_title(window).replace('\u{200b}', "").starts_with(title) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The strip's tab items with raw name and selected state, read in one call.
fn read_strip(uia: &Uia, strip: &IUIAutomationElement) -> Option<Vec<(IUIAutomationElement, String, bool)>> {
    unsafe {
        let found = strip
            .FindAllBuildCache(TreeScope_Descendants, &uia.tab_condition, &uia.tab_cache)
            .ok()?;
        let len = found.Length().ok()?;
        let mut out = Vec::with_capacity(len.max(0) as usize);
        for i in 0..len {
            let Ok(el) = found.GetElement(i) else { continue };
            let raw = el.CachedName().map(|b| b.to_string()).unwrap_or_default();
            let selected = el
                .GetCachedPropertyValue(UIA_SelectionItemIsSelectedPropertyId)
                .ok()
                .and_then(|v| bool::try_from(&v).ok())
                .unwrap_or(false);
            out.push((el, raw, selected));
        }
        Some(out)
    }
}

/// Breadth-first search of the window's control view for the element that
/// holds its tab items: a Tab control with TabItems below it, or else the
/// parent of the first TabItem met. Page content is not entered.
fn find_strip(uia: &Uia, window: isize, engine: Engine) -> Option<IUIAutomationElement> {
    let deadline = Instant::now() + Duration::from_millis(FIND_BUDGET_MS);
    unsafe {
        let root = uia.automation.ElementFromHandle(hwnd(window)).ok()?;
        let walker = uia.automation.ControlViewWalker().ok()?;
        let cutoff = document_cutoff(engine);
        let mut queue = VecDeque::from([(root, 0usize)]);
        let mut visited = 0;
        while let Some((el, depth)) = queue.pop_front() {
            visited += 1;
            if visited > MAX_NODES || Instant::now() > deadline {
                return None;
            }
            let Ok(ct) = el.CurrentControlType() else { continue };
            if ct == UIA_TabControlTypeId && el.FindFirst(TreeScope_Descendants, &uia.tab_condition).is_ok() {
                return Some(el);
            }
            if ct == UIA_TabItemControlTypeId {
                return walker.GetParentElement(&el).ok();
            }
            if (ct == UIA_DocumentControlTypeId && depth >= cutoff) || depth >= MAX_DEPTH {
                continue;
            }
            let mut child = walker.GetFirstChildElement(&el);
            while let Ok(c) = child {
                child = walker.GetNextSiblingElement(&c);
                queue.push_back((c, depth + 1));
            }
        }
        None
    }
}

/// The window's control view down to `max_depth` (diagnostics), skipping
/// page content.
fn outline(uia: &Uia, window: isize, max_depth: usize) -> Vec<String> {
    fn walk(
        walker: &windows::Win32::UI::Accessibility::IUIAutomationTreeWalker,
        el: &IUIAutomationElement,
        depth: usize,
        max_depth: usize,
        out: &mut Vec<String>,
    ) {
        if out.len() > 400 {
            return;
        }
        unsafe {
            let ct = el.CurrentControlType().ok();
            let class = el.CurrentClassName().map(|b| b.to_string()).unwrap_or_default();
            out.push(format!(
                "{}{} \u{201c}{}\u{201d} class={class}",
                "  ".repeat(depth),
                ct.map(type_name).unwrap_or_else(|| "?".into()),
                uia::name(el).chars().take(60).collect::<String>()
            ));
            if depth >= max_depth || ct == Some(UIA_DocumentControlTypeId) {
                return;
            }
            let mut child = walker.GetFirstChildElement(el);
            while let Ok(c) = child {
                walk(walker, &c, depth + 1, max_depth, out);
                child = walker.GetNextSiblingElement(&c);
            }
        }
    }
    let mut out = Vec::new();
    unsafe {
        if let (Ok(root), Ok(walker)) = (
            uia.automation.ElementFromHandle(hwnd(window)),
            // QS_RAW_VIEW=1: include elements the control view hides.
            if std::env::var_os("QS_RAW_VIEW").is_some() {
                uia.automation.RawViewWalker()
            } else {
                uia.automation.ControlViewWalker()
            },
        ) {
            walk(&walker, &root, 0, max_depth, &mut out);
        }
    }
    out
}

/// Control types and names from the window down to `el`.
fn path_to(uia: &Uia, el: &IUIAutomationElement, window: isize) -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let Ok(walker) = uia.automation.ControlViewWalker() else {
            return out;
        };
        let mut cur = Some(el.clone());
        while let Some(e) = cur {
            let ct = e.CurrentControlType().map(type_name).unwrap_or_else(|_| "?".into());
            let class = e.CurrentClassName().map(|b| b.to_string()).unwrap_or_default();
            out.push(format!("{ct} \u{201c}{}\u{201d} class={class}", uia::name(&e)));
            if e.CurrentNativeWindowHandle().is_ok_and(|h| h.0 as isize == window) || out.len() > MAX_DEPTH {
                break;
            }
            cur = walker.GetParentElement(&e).ok();
        }
    }
    out.reverse();
    out
}

fn type_name(ct: UIA_CONTROLTYPE_ID) -> String {
    let known = [
        (UIA_WindowControlTypeId, "Window"),
        (UIA_PaneControlTypeId, "Pane"),
        (UIA_GroupControlTypeId, "Group"),
        (UIA_ToolBarControlTypeId, "ToolBar"),
        (UIA_TitleBarControlTypeId, "TitleBar"),
        (UIA_TabControlTypeId, "Tab"),
        (UIA_TabItemControlTypeId, "TabItem"),
        (UIA_DocumentControlTypeId, "Document"),
        (UIA_ButtonControlTypeId, "Button"),
    ];
    known
        .iter()
        .find(|(id, _)| *id == ct)
        .map(|(_, n)| n.to_string())
        .unwrap_or_else(|| format!("ControlType({})", ct.0))
}
