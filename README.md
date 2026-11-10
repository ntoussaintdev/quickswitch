# QuickSwitch

A small Spotlight-style launcher for Windows 11. Press **Ctrl + Win + K**, type, press **Enter**:

- switch to any open window (minimized windows are restored),
- switch to an individual **File Explorer tab**,
- open a folder from Linux-friendly input such as `~/Documents`, `C:/dev/work`, `./notes` or `//server/share`.

It runs in the notification area (system tray) until you choose **Exit**. Built with Rust and egui/eframe; one executable, no admin rights.

## 1. Install

Pick one:

| Package | What it does |
|---|---|
| `QuickSwitch-<version>-setup.exe` | Per-user installer. Installs to `%LOCALAPPDATA%\Programs\QuickSwitch`, adds a Start menu entry, optional "Start when I sign in" (unchecked by default). Uninstall from *Settings → Apps*; this also removes the startup entry. |
| `QuickSwitch-<version>-portable.zip` | Unzip anywhere and run `QuickSwitch.exe`. The `QuickSwitch.portable` file next to the exe keeps settings and the log in that folder. |

Neither needs administrator rights. The binaries are not code-signed, so SmartScreen may warn on first run.

## 2. Use

| Action | Result |
|---|---|
| Run `QuickSwitch.exe` | Starts hidden in the tray. Running it again opens the search popup. |
| **Ctrl + Win + K** | Opens the popup on the monitor you are using. Press again to close. |
| Type | Filters open windows and Explorer tabs (fuzzy; prefix matches first; recent picks get a boost). |
| **↑ / ↓**, **Tab**, **PgUp / PgDn** | Move through results. |
| **Enter** or click | Switch to the window / tab, or open the folder. |
| **Esc**, click elsewhere | Hide the popup. QuickSwitch keeps running. |
| Tray icon left-click | Open search. |
| Tray icon right-click | *Open search*, *Start with Windows* (checkbox), *Exit QuickSwitch*. |

### 2.1 Folder input

Input is treated as a folder path only when it starts with one of these:

| Input | Opens |
|---|---|
| `~`, `~/docs` | `%USERPROFILE%`, `%USERPROFILE%\docs` |
| `./notes` | `%USERPROFILE%\notes` (relative paths are resolved from your home folder, never from the app's working directory) |
| `C:/dev/work`, `C:\dev\work`, `D:` | that drive path (slashes normalized) |
| `//server/share/dir`, `\\server\share` | a UNC network path |

Spaces are fine, surrounding quotes are ignored. A missing folder shows "Folder not found" and opens nothing. Not translated on purpose: `~otheruser`, WSL paths like `/home/alice`, environment variables. Network folders are checked when you press Enter, not while typing.

### 2.2 Start with Windows

The tray checkbox adds or removes the value `QuickSwitch` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` with the data `"<path to QuickSwitch.exe>" --background`. The checkbox always shows the real registry state. It is off by default and stays as you set it when you exit. With the portable build, the entry points at wherever the exe is now; re-enable it after moving the folder.

### 2.3 Command line

| Argument | Effect |
|---|---|
| *(none)* | Start in the tray, or open the popup of the running copy. |
| `--background` | Start in the tray; if already running, do nothing (used at sign-in). |
| `--exit` | Ask the running copy to exit and wait for it (used by the installer). |

## 3. Settings

`%APPDATA%\QuickSwitch\config.toml` (portable: next to the exe). Created on first use; every key is optional.

```toml
hotkey = "Ctrl+Win+K"              # main shortcut
fallback_hotkey = "Ctrl+Alt+Space"  # optional second shortcut, registered only if set
max_results = 9                     # rows in the popup (1–20)
recent = []                         # managed by QuickSwitch: recently picked results
```

Shortcut syntax: modifiers `Ctrl`, `Alt`, `Shift`, `Win` joined with `+`, then one key (`K`, `Space`, `F1`, ...). If a shortcut is already taken by another app, QuickSwitch says so once and keeps running; open it from the tray or set `fallback_hotkey`. Restart QuickSwitch after editing the file.

Problems are logged to `quickswitch.log` in the same folder.

## 4. Known limitations

- **Explorer tabs** come from UI Automation. On the Windows 11 build used for development every tab is a UIA `TabItem` with `SelectionItemPattern` (checked with `examples/uia_spike.rs`). Other builds may differ; when no tabs are found the Explorer window is listed as a normal window.
- **Minimized Explorer windows** don't expose their tab strip, so they appear as one window row; switching restores the window.
- **Opening a folder** uses the shell's normal "open". Whether that makes a new Explorer window or a new tab depends on your Explorer settings and Windows build.
- **Focus rules.** Windows decides whether an app may take the foreground. QuickSwitch only switches in response to your key press or click and never simulates keystrokes to get around this; if Windows refuses, the popup shows an error.
- **Elevated (administrator) windows** are listed but some may refuse switching from a non-elevated QuickSwitch. Windows on other virtual desktops are not listed.
- Searching inside browser tabs, files on disk or WSL paths is out of scope for v1.

## 5. Build

Requirements: Rust (MSVC toolchain, edition 2024, Rust 1.88+) and the Windows SDK (for the icon/version resource). For the installer: Inno Setup 6 (`winget install JRSoftware.InnoSetup --scope user`).

```powershell
cargo run                         # debug build, with a console for diagnostics
cargo test                        # unit tests (paths, ranking, settings, result lifecycle, shortcuts)
cargo run --example uia_spike     # list Explorer windows and the tabs UIA exposes
cargo run --example uia_spike -- --select 0 1   # try selecting tab 1 of window 0
.\scripts\package.ps1             # fmt + clippy + tests + release build + ZIP + installer into dist\
cargo build --release --features wgpu   # alternative renderer, for comparison
```

The default renderer is glow (OpenGL). The `wgpu` feature switches to wgpu (Direct3D 12/Vulkan) for the comparison in plan §9.2 A15.

`scripts/make_icon.py` regenerates `resources/` (needs Pillow).

## 6. Code map

| Path | Role |
|---|---|
| `src/main.rs` | Entry: single-instance check, hidden eframe viewport, CLI flags. |
| `src/app.rs` | `eframe::App`: commands and lifecycle in `logic`, the palette in `ui`. |
| `src/command.rs` | Command queue from tray/hotkey/worker/second launch into the app. |
| `src/ui/` | Palette widget and styles. |
| `src/os/` | Tray, global hotkey, Win32 windows (enumerate, activate, place), single instance, log. |
| `src/search/` | Result model and snapshot merge, nucleo-based ranking, UIA Explorer-tab worker (COM MTA thread). |
| `src/actions/` | Path resolution and opening folders. |
| `src/config.rs`, `src/startup.rs` | Settings file, HKCU Run value. |
| `packaging/QuickSwitch.iss` | Inno Setup installer. |
| `docs/plans/implemented/` | The implementation plan this follows. |
