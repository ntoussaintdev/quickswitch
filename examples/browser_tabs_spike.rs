//! Step 0 diagnostic (plan 2026-11-11-n1 §5): list browser windows and the
//! tabs QuickSwitch reads from them, using the app's own reader.
//!
//!   cargo run --example browser_tabs_spike                 list windows and tabs (cold, then warm timing)
//!   cargo run --example browser_tabs_spike -- --tree 2     path to window 2's tab strip, per-tab patterns
//!   cargo run --example browser_tabs_spike -- --select 2 5  switch window 2 to its tab 5
//!   cargo run --example browser_tabs_spike -- --repeat 5   time 5 scans of every window
//!
//! Read-only except `--select`. Note each browser's memory in Task Manager
//! before and after, and what chrome://accessibility (edge://accessibility)
//! shows as enabled afterwards.

use std::time::Instant;

use quickswitch::os::windows::enumerate;
use quickswitch::search::browser_tabs::BrowserReader;
use quickswitch::search::browsers::BrowserWindow;
use quickswitch::search::uia::Uia;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};

fn browser_windows() -> Vec<BrowserWindow> {
    let all = enumerate();
    for w in &all {
        if w.browser().is_none() && (w.class_name == "Chrome_WidgetWin_1" || w.class_name == "MozillaWindowClass") {
            println!("not a browser (kept as a window): {} [{}]", w.process, w.class_name);
        }
    }
    all.iter()
        .filter_map(|w| {
            w.browser().map(|spec| BrowserWindow {
                hwnd: w.hwnd,
                spec,
                title: w.title.clone(),
            })
        })
        .collect()
}

fn arg_num(args: &[String], i: usize) -> usize {
    args.get(i).and_then(|s| s.parse().ok()).unwrap_or_else(|| {
        eprintln!("expected a number after {}", args[i - 1]);
        std::process::exit(2)
    })
}

fn main() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let uia = Uia::new().expect("UI Automation");
    let mut reader = BrowserReader::new();
    let windows = browser_windows();
    if windows.is_empty() {
        println!("No browser windows found.");
        return;
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pick = |n: usize| -> &BrowserWindow {
        windows.get(n).unwrap_or_else(|| {
            eprintln!("no window {n}; run without arguments to list them");
            std::process::exit(2)
        })
    };

    match args.first().map(String::as_str) {
        Some("--tree") => {
            let w = pick(arg_num(&args, 1));
            println!("{} \u{201c}{}\u{201d}", w.spec.name, w.title);
            print!("{}", reader.diagnose(&uia, w));
        }
        Some("--select") => {
            let w = pick(arg_num(&args, 1));
            let t = arg_num(&args, 2);
            reader.begin_scan(&windows);
            let tabs = reader.scan_window(&uia, w);
            let Some(tab) = tabs.get(t) else {
                eprintln!("window has {} tabs", tabs.len());
                std::process::exit(2)
            };
            println!(
                "switching to \u{201c}{}\u{201d} (minimized windows are not restored here)",
                tab.title
            );
            let t0 = Instant::now();
            match reader.select(&uia, w.hwnd, tab.key, &tab.raw_name, tab.nth) {
                Ok(how) => println!("ok via {how} in {} ms", t0.elapsed().as_millis()),
                Err(e) => println!(
                    "failed in {} ms: {} (gone: {})",
                    t0.elapsed().as_millis(),
                    e.message,
                    e.gone
                ),
            }
        }
        Some("--repeat") => {
            let n = arg_num(&args, 1);
            for pass in 1..=n {
                reader.begin_scan(&windows);
                let t0 = Instant::now();
                let count: usize = windows.iter().map(|w| reader.scan_window(&uia, w).len()).sum();
                println!("pass {pass}: {count} tabs in {} ms", t0.elapsed().as_millis());
            }
        }
        _ => {
            for pass in ["cold", "warm"] {
                reader.begin_scan(&windows);
                println!("--- {pass} ---");
                for (i, w) in windows.iter().enumerate() {
                    let t0 = Instant::now();
                    let tabs = reader.scan_window(&uia, w);
                    println!(
                        "[{i}] {} \u{201c}{}\u{201d}: {} tabs in {} ms",
                        w.spec.name,
                        w.title,
                        tabs.len(),
                        t0.elapsed().as_millis()
                    );
                    if pass == "warm" {
                        for (j, t) in tabs.iter().enumerate() {
                            println!(
                                "    [{j:>2}] {} {} \u{2014} {}",
                                if t.selected { "*" } else { " " },
                                t.title,
                                t.browser
                            );
                        }
                    }
                }
            }
        }
    }
}
