//! Single instance per user session, plus "open the palette" wake-up for a
//! second launch (plan step 14).
//!
//! A named mutex marks the running instance. A named auto-reset event is the
//! wake-up channel: a second launch sets it and exits. Both live in the
//! session-local namespace (`Local\`) and carry the user name, and the default
//! security descriptor limits access to the same user, so only the user's
//! own processes can signal it.

use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, SetEvent, WaitForMultipleObjects,
};
use windows::core::HSTRING;

fn object_name(kind: &str) -> HSTRING {
    let user: String = std::env::var("USERNAME")
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    HSTRING::from(format!("Local\\QuickSwitch.{kind}.{user}"))
}

/// Owned kernel handle that closes on drop.
struct Owned(HANDLE);
// Kernel handles may be used from any thread.
unsafe impl Send for Owned {}
unsafe impl Sync for Owned {}
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Held by the running instance for its whole lifetime.
pub struct PrimaryInstance {
    _mutex: Owned,
    open_event: std::sync::Arc<Owned>,
    exit_event: std::sync::Arc<Owned>,
    quit_event: std::sync::Arc<Owned>,
    listener: Option<std::thread::JoinHandle<()>>,
}

pub enum Instance {
    Primary(PrimaryInstance),
    /// Another QuickSwitch is already running for this user.
    Secondary,
}

pub fn acquire() -> Result<Instance, String> {
    let mutex = unsafe { CreateMutexW(None, true, &object_name("Instance")) }.map_err(|e| e.message())?;
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            let _ = CloseHandle(mutex);
        }
        return Ok(Instance::Secondary);
    }
    let open_event = unsafe { CreateEventW(None, false, false, &object_name("Open")) }.map_err(|e| e.message())?;
    let exit_event = unsafe { CreateEventW(None, false, false, &object_name("Exit")) }.map_err(|e| e.message())?;
    let quit_event = unsafe { CreateEventW(None, true, false, None) }.map_err(|e| e.message())?;
    Ok(Instance::Primary(PrimaryInstance {
        _mutex: Owned(mutex),
        open_event: std::sync::Arc::new(Owned(open_event)),
        exit_event: std::sync::Arc::new(Owned(exit_event)),
        quit_event: std::sync::Arc::new(Owned(quit_event)),
        listener: None,
    }))
}

impl PrimaryInstance {
    /// Call `on_signal` (on a background thread) whenever a second launch
    /// asks the running instance to open its palette or to exit.
    pub fn listen(&mut self, on_signal: impl Fn(Signal) + Send + 'static) {
        let open = self.open_event.clone();
        let exit = self.exit_event.clone();
        let quit = self.quit_event.clone();
        self.listener = Some(
            std::thread::Builder::new()
                .name("quickswitch-instance".into())
                .spawn(move || {
                    loop {
                        let r = unsafe { WaitForMultipleObjects(&[open.0, exit.0, quit.0], false, INFINITE) };
                        if r == WAIT_OBJECT_0 {
                            on_signal(Signal::Open);
                        } else if r.0 == WAIT_OBJECT_0.0 + 1 {
                            on_signal(Signal::Exit);
                        } else {
                            break; // quit signalled, or a wait error
                        }
                    }
                })
                .expect("spawn instance listener"),
        );
    }

    pub fn stop(&mut self) {
        unsafe {
            let _ = SetEvent(self.quit_event.0);
        }
        if let Some(t) = self.listener.take() {
            let _ = t.join();
        }
    }
}

impl Drop for PrimaryInstance {
    fn drop(&mut self) {
        self.stop();
    }
}

/// What a second launch asks of the running instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Open,
    Exit,
}

/// Signal the running instance. Retries briefly to cover a sign-in race
/// where the first instance holds the mutex but has not yet created its
/// events.
pub fn signal_existing(signal: Signal) -> bool {
    let name = object_name(match signal {
        Signal::Open => "Open",
        Signal::Exit => "Exit",
    });
    for _ in 0..20 {
        if let Ok(ev) = unsafe { OpenEventW(EVENT_MODIFY_STATE, false, &name) } {
            let ok = unsafe { SetEvent(ev) }.is_ok();
            unsafe {
                let _ = CloseHandle(ev);
            }
            return ok;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}
