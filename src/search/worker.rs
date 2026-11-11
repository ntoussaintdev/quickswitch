//! The worker thread that reads and switches Explorer and browser tabs.
//!
//! It is a single-threaded COM apartment that pumps messages while it waits
//! (Explorer's shell objects need that; see `explorer_tabs`). COM objects
//! never leave it; the UI thread gets plain records through `AppCommand`.
//!
//! Order of work (plan 2026-11-11-n1 §4.4): Explorer tabs first, then
//! waiting switch requests, then browser windows one at a time. Between
//! browser windows, new switch requests run at once, and a newer scan request
//! (the popup was reopened) abandons the rest of the old browser scan.

use std::collections::VecDeque;
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender};

use super::browsers::BrowserWindow;
use crate::command::{AppCommand, CommandSender};

pub enum Request {
    /// List all Explorer tabs. Older pending scans are dropped.
    Scan {
        generation: u64,
    },
    /// Read the tabs of these browser windows. Older pending scans are dropped.
    ScanBrowsers {
        generation: u64,
        windows: Vec<BrowserWindow>,
    },
    /// Show this Explorer tab in its window.
    Select {
        explorer_hwnd: isize,
        tab_hwnd: isize,
        name: String,
        nth: usize,
    },
    /// Show this browser tab in its window.
    SelectBrowserTab {
        window_hwnd: isize,
        key: u64,
        name: String,
        nth: usize,
    },
    Shutdown,
}

impl Request {
    fn is_select(&self) -> bool {
        matches!(self, Request::Select { .. } | Request::SelectBrowserTab { .. })
    }
}

pub struct TabWorker {
    tx: Sender<Request>,
    wake: imp::Event,
    thread: Option<JoinHandle<()>>,
}

impl TabWorker {
    pub fn start(sender: CommandSender) -> TabWorker {
        let (tx, rx) = crossbeam_channel::unbounded();
        let wake = imp::Event::new();
        let thread_wake = wake.clone();
        let thread = std::thread::Builder::new()
            .name("quickswitch-shell".into())
            .spawn(move || imp::run(rx, thread_wake, sender))
            .expect("spawn shell worker");
        TabWorker {
            tx,
            wake,
            thread: Some(thread),
        }
    }

    pub fn request(&self, req: Request) {
        if self.tx.send(req).is_ok() {
            self.wake.set();
        }
    }

    /// Stop the worker. Waits briefly; a worker stuck inside a hung Explorer
    /// or browser call is abandoned rather than blocking exit.
    pub fn shutdown(&mut self) {
        self.request(Request::Shutdown);
        if let Some(t) = self.thread.take() {
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
            while !t.is_finished() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(10));
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
    use std::sync::Arc;

    use super::*;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
    use windows::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent};
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT, TranslateMessage,
    };

    use crate::search::browser_tabs::BrowserReader;
    use crate::search::explorer_tabs;
    use crate::search::uia::Uia;

    /// Auto-reset Win32 event that wakes the worker when a request arrives.
    #[derive(Clone)]
    pub struct Event(Arc<OwnedHandle>);

    struct OwnedHandle(HANDLE);
    // An event handle may be signalled from any thread.
    unsafe impl Send for OwnedHandle {}
    unsafe impl Sync for OwnedHandle {}

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    impl Event {
        pub fn new() -> Event {
            let h = unsafe { CreateEventW(None, false, false, None) }.unwrap_or_default();
            Event(Arc::new(OwnedHandle(h)))
        }

        pub fn set(&self) {
            unsafe {
                let _ = SetEvent(self.0.0);
            }
        }

        fn handle(&self) -> HANDLE {
            self.0.0
        }
    }

    struct Worker {
        uia: Option<Uia>,
        browsers: BrowserReader,
        sender: CommandSender,
    }

    impl Worker {
        fn uia(&mut self) -> Result<&Uia, String> {
            if self.uia.is_none() {
                self.uia = Some(Uia::new().map_err(|e| format!("UI Automation is unavailable: {e}"))?);
            }
            Ok(self.uia.as_ref().expect("just set"))
        }

        fn select(&mut self, req: Request) {
            match req {
                Request::Select {
                    explorer_hwnd,
                    tab_hwnd,
                    name,
                    nth,
                } => {
                    let result = self
                        .uia()
                        .and_then(|uia| explorer_tabs::select(uia, explorer_hwnd, tab_hwnd, &name, nth));
                    self.sender.send(AppCommand::TabSelected { explorer_hwnd, result });
                }
                Request::SelectBrowserTab {
                    window_hwnd,
                    key,
                    name,
                    nth,
                } => {
                    let ready = self.uia().map(|_| ());
                    let (result, gone) = match ready {
                        Err(e) => (Err(e), false),
                        Ok(_) => {
                            let uia = self.uia.as_ref().expect("just set");
                            match self.browsers.select(uia, window_hwnd, key, &name, nth) {
                                Ok(_) => (Ok(()), false),
                                Err(e) => (Err(e.message), e.gone),
                            }
                        }
                    };
                    self.sender.send(AppCommand::BrowserTabSelected {
                        window_hwnd,
                        result,
                        gone,
                    });
                }
                _ => {}
            }
        }
    }

    pub fn run(rx: Receiver<Request>, wake: Event, sender: CommandSender) {
        if unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_err() {
            crate::os::log("Shell worker: CoInitializeEx failed; Explorer and browser tabs disabled");
        }
        let mut worker = Worker {
            uia: None,
            browsers: BrowserReader::new(),
            sender: sender.clone(),
        };
        let mut backlog: VecDeque<Request> = VecDeque::new();
        'outer: loop {
            backlog.extend(rx.try_iter());
            // Take everything queued; only the newest scan of each kind matters.
            let mut scan = None;
            let mut browser_scan = None;
            let mut selects = Vec::new();
            for req in backlog.drain(..) {
                match req {
                    Request::Scan { generation } => scan = Some(generation),
                    Request::ScanBrowsers { generation, windows } => browser_scan = Some((generation, windows)),
                    Request::Shutdown => break 'outer,
                    other => selects.push(other),
                }
            }
            if let Some(generation) = scan {
                let tabs = explorer_tabs::scan_tabs();
                sender.send(AppCommand::Tabs { generation, tabs });
            }
            for req in selects {
                worker.select(req);
            }
            if let Some((generation, windows)) = browser_scan {
                let mut tabs = Vec::new();
                let mut superseded = false;
                worker.browsers.begin_scan(&windows);
                for w in &windows {
                    // Switch requests jump the queue; a newer scan ends this one.
                    for req in rx.try_iter() {
                        match req {
                            Request::Shutdown => break 'outer,
                            r if r.is_select() => worker.select(r),
                            r => {
                                superseded = true;
                                backlog.push_back(r);
                            }
                        }
                    }
                    if superseded {
                        break;
                    }
                    if worker.uia().is_err() {
                        break;
                    }
                    let uia = worker.uia.as_ref().expect("checked");
                    tabs.extend(worker.browsers.scan_window(uia, w));
                }
                if !superseded {
                    sender.send(AppCommand::BrowserTabs { generation, tabs });
                }
                if !backlog.is_empty() {
                    continue;
                }
            }
            // Sleep until a request arrives, pumping messages as an STA must.
            let handles = [wake.handle()];
            let r = unsafe { MsgWaitForMultipleObjects(Some(&handles), false, INFINITE, QS_ALLINPUT) };
            if r.0 == WAIT_OBJECT_0.0 + 1 {
                pump();
            }
        }
        worker.uia = None;
        worker.browsers = BrowserReader::new();
        unsafe { CoUninitialize() };
    }

    fn pump() {
        let mut msg = MSG::default();
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}
