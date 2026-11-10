// No console window in release builds (debug builds keep one for diagnostics).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use quickswitch::app::QuickSwitchApp;
use quickswitch::os::{self, instance};
use quickswitch::{startup, ui};

/// glow by default (smaller binary); `--features wgpu` for comparison.
#[cfg(not(feature = "wgpu"))]
const RENDERER: eframe::Renderer = eframe::Renderer::Glow;
#[cfg(feature = "wgpu")]
const RENDERER: eframe::Renderer = eframe::Renderer::Wgpu;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let background = args.iter().any(|a| a == startup::BACKGROUND_ARG);
    if args.iter().any(|a| a == "--exit") {
        exit_running_instance();
        return;
    }

    // One resident per user session. A second launch asks the running one to
    // open its palette (unless it's the login autostart) and exits.
    let primary = match instance::acquire() {
        Ok(instance::Instance::Secondary) => {
            if !background && !instance::signal_existing(instance::Signal::Open) {
                os::log("Another QuickSwitch is running but did not answer the wake-up signal");
            }
            return;
        }
        Ok(instance::Instance::Primary(p)) => Some(p),
        Err(e) => {
            os::log(&format!("Single-instance check failed ({e}); continuing without it"));
            None
        }
    };

    let (rgba, w, h) = (include_bytes!("../resources/icon_64.rgba").to_vec(), 64, 64);
    let (logical_w, logical_h) = ui::styles::POPUP_SIZE;
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("QuickSwitch")
            .with_app_id("QuickSwitch")
            .with_inner_size([logical_w, logical_h])
            // Off-screen and non-activating for eframe's one forced first paint.
            .with_position([-10_000.0, -10_000.0])
            .with_active(false)
            .with_decorations(false)
            .with_resizable(false)
            .with_taskbar(false)
            .with_always_on_top()
            .with_visible(false)
            .with_icon(egui::IconData {
                rgba,
                width: w,
                height: h,
            }),
        renderer: RENDERER,
        ..Default::default()
    };

    let (tx, rx) = crossbeam_channel::unbounded();
    let result = eframe::run_native(
        "QuickSwitch",
        native_options,
        Box::new(move |cc| Ok(Box::new(QuickSwitchApp::new(cc, primary, tx, rx)))),
    );
    if let Err(e) = result {
        let msg = format!("QuickSwitch could not start: {e}");
        os::log(&msg);
        show_fatal(&msg);
    }
}

/// `QuickSwitch.exe --exit`: ask the running instance to exit and wait (up to
/// ~5 s) until it has. Used by the installer and uninstaller.
fn exit_running_instance() {
    if matches!(instance::acquire(), Ok(instance::Instance::Primary(_)) | Err(_)) {
        return; // nothing running (the temporary guard is dropped right away)
    }
    if !instance::signal_existing(instance::Signal::Exit) {
        return;
    }
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if matches!(instance::acquire(), Ok(instance::Instance::Primary(_))) {
            return;
        }
    }
}

fn show_fatal(msg: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    use windows::core::{HSTRING, w};
    unsafe {
        MessageBoxW(None, &HSTRING::from(msg), w!("QuickSwitch"), MB_OK | MB_ICONERROR);
    }
}
