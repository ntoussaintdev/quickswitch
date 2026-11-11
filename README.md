# QuickSwitch

A small Spotlight-style launcher for Windows 11. Press **Ctrl + Win + K**, type, press **Enter**:

- switch to any open window (minimized windows are restored),
- switch to an individual **File Explorer tab**,
- open a folder from Linux-friendly input such as `~/Documents`, `C:/dev/work`, `./notes`, `%APPDATA%/x` or `//server/share`.

It runs in the notification area (system tray) until you choose **Exit**. Built with Rust and egui/eframe; one executable, no admin rights.

This is the developer README. The packages ship [`packaging/README.txt`](packaging/README.txt) instead, which covers only what a user of the release needs.

## 1. Install

**Download the latest release:** [Installer (QuickSwitch-setup.exe)](https://github.com/ntoussaintdev/quickswitch/releases/latest/download/QuickSwitch-setup.exe) · [Portable (QuickSwitch-portable.zip)](https://github.com/ntoussaintdev/quickswitch/releases/latest/download/QuickSwitch-portable.zip) · [All releases](https://github.com/ntoussaintdev/quickswitch/releases)

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
| Type | Filters open windows, Explorer tabs and remembered folders. Every word must match, in any order; matched letters are highlighted. |
| **↑ / ↓**, **Tab**, **PgUp / PgDn** | Move through results. In folder input, **Tab** completes the selected folder instead. |
| **Enter** or click | Switch to the window / tab, or open the folder. |
| **Esc**, click elsewhere | Hide the popup. QuickSwitch keeps running. |
| Tray icon left-click | Open search. |
| Tray icon right-click | *Open search*, *Change shortcut…*, *Start with Windows* (checkbox), *Exit QuickSwitch*. |
| *Change shortcut…* | Pick up to three modifiers (Ctrl, Alt, Shift, Win) and a key from dropdowns; **Save** switches at once, no restart. If Windows or another app owns the combination, the old shortcut keeps working and the view says so. |

### 2.1 What is matched

- **Windows**: title and app name.
- **Explorer tabs**: only the folder path, shown on the row with the home folder as `~` (the `~` part is not searched). A match in the tab's own folder name ranks above a match higher up the path.
- **Remembered folders**: folders opened from QuickSwitch or seen open in Explorer (up to 200, stored in `config.toml`). They are searched like tabs but not listed for an empty query, and a folder that is open in a tab is shown as that tab instead.
- A word matches as a substring, or fuzzily (letters in order with gaps) **within one word or folder name**. Fuzzy matches never jump across words or folders.
- Ties: recent picks first, then window Z-order.

### 2.2 Folder input

Input is treated as a folder path only when it starts with one of these:

| Input | Opens |
|---|---|
| `~`, `~/docs` | `%USERPROFILE%`, `%USERPROFILE%\docs` |
| `./notes` | `%USERPROFILE%\notes` (relative paths are resolved from your home folder, never from the app's working directory) |
| `C:/dev/work`, `C:\dev\work`, `D:` | that drive path (slashes normalized) |
| `%APPDATA%/quickswitch`, `%localappdata%` | the folder an environment variable points to (only at the start; case doesn't matter) |
| `//server/share/dir`, `\\server\share` | a UNC network path |

While you type, the popup lists the subfolders of the folder before the last `/` that match the part after it, like a wildcard `ls`: `~/proj` lists `Projects`, `projects-old`, ... (exact name, then prefix, substring and fuzzy matches). Right after a `/` the folder itself comes first, then all its subfolders. Rows use the real capitalisation.

- **Tab** completes the selected folder (`~/Projects/`) so you can keep typing the next level; **Enter** opens it.
- Hidden and system folders (e.g. `AppData`) appear only once the typed part is a prefix of their name.
- If the folder is already open in an Explorer tab, the row is that tab and **Enter** switches to it (no new window).
- A listing is cached while you type inside the same folder and refreshed each time the popup opens.

Spaces are fine, surrounding quotes are ignored. A missing folder shows "Folder not found" and opens nothing. Not translated on purpose: `~otheruser`, WSL paths like `/home/alice`, environment variables. Network folders are not listed while typing (a slow server could stall the popup); they are checked when you press Enter.

### 2.3 Start with Windows

The tray checkbox adds or removes the value `QuickSwitch` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` with the data `"<path to QuickSwitch.exe>" --background`. The checkbox always shows the real registry state. It is off by default and stays as you set it when you exit. With the portable build, the entry points at wherever the exe is now; re-enable it after moving the folder.

### 2.4 Command line

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
folders = []                        # managed by QuickSwitch: remembered folders (max 200)
```

The main shortcut is easiest to change from the tray menu (*Change shortcut…*), which also saves it here. Shortcut syntax: modifiers `Ctrl`, `Alt`, `Shift`, `Win` joined with `+`, then one key (`K`, `Space`, `F1`, ...). If a shortcut is already taken by another app, QuickSwitch says so once and keeps running; open it from the tray, pick another, or set `fallback_hotkey`. Restart QuickSwitch after editing the file.

Problems are logged to `quickswitch.log` in the same folder.

## 4. Known limitations

- **Explorer tabs** are read from the Shell's `ShellWindows` list, which has one entry per tab with its folder and its own `ShellTabWindowClass` window (also for minimized windows). Switching uses UI Automation: the tab strip's `TabItem` with the same name gets `SelectionItemPattern.Select()`. UIA exposes no path, so with several same-named tabs in one window QuickSwitch tries the likeliest one, checks which tab window came to the front, and tries the next if needed; you may see a brief flicker. Checked on Windows 11 with `examples/tab_paths_spike.rs` and `examples/uia_spike.rs`; other builds may differ, and an Explorer window with no readable tabs is listed as a normal window.
- **Virtual locations** (Home, This PC, Gallery, ...) have no folder path; their tabs are matched by name.
- **Opening a folder** that no tab shows uses the shell's normal "open". Whether that makes a new Explorer window or a new tab depends on your Explorer settings and Windows build.
- **Focus rules.** Windows decides whether an app may take the foreground. QuickSwitch only switches in response to your key press or click and never simulates keystrokes to get around this; if Windows refuses, the popup shows an error.
- **Elevated (administrator) windows** are listed but some may refuse switching from a non-elevated QuickSwitch. Windows on other virtual desktops are not listed.
- Searching inside browser tabs, files on disk or WSL paths is out of scope for v1.

## 5. Build

Requirements: Rust (MSVC toolchain, edition 2024, Rust 1.88+) and the Windows SDK (for the icon/version resource). For the installer: Inno Setup 6 (`winget install JRSoftware.InnoSetup --scope user`).

```powershell
cargo run                         # debug build, with a console for diagnostics
cargo test                        # unit tests (paths, ranking, settings, result lifecycle, shortcuts)
cargo run --example tab_paths_spike   # list Explorer tabs with their folders and tab windows (read-only)
cargo run --example uia_spike     # list Explorer windows and the tabs UIA exposes (read-only)
cargo run --example uia_spike -- --select 0 1   # try selecting tab 1 of window 0
.\scripts\package.ps1             # fmt + clippy + tests + release build + ZIP + installer into dist\ (ships packaging\README.txt)
cargo build --release --features wgpu   # alternative renderer, for comparison
```

The default renderer is glow (OpenGL). The `wgpu` feature switches to wgpu (Direct3D 12/Vulkan) for the comparison in plan §9.2 A15.

`scripts/make_icon.py` regenerates `resources/` (needs Pillow).

### 5.1 Releases

GitHub Actions builds every release. `CI` runs fmt, clippy and tests on each push to `main` and on pull requests. To publish a release, bump `version` in `Cargo.toml`, commit, then push a matching tag:

```powershell
git tag v0.3.0
git push origin v0.3.0
```

The `Release` workflow runs `scripts/package.ps1` on Windows and attaches the installer and portable ZIP (versioned, plus unversioned copies for the stable download links above) to a GitHub Release.

## 6. Code map

| Path | Role |
|---|---|
| `src/main.rs` | Entry: single-instance check, hidden eframe viewport, CLI flags. |
| `src/app.rs` | `eframe::App`: commands and lifecycle in `logic`, the palette in `ui`. |
| `src/command.rs` | Command queue from tray/hotkey/worker/second launch into the app. |
| `src/ui/` | Palette widget, shortcut settings view and styles. |
| `src/os/` | Tray, global hotkey, Win32 windows (enumerate, activate, place), single instance, log. |
| `src/search/` | Result model and snapshot merge, ranking and highlighting (nucleo), Explorer-tab worker (COM STA thread: ShellWindows + UI Automation). |
| `src/actions/` | Path resolution, folder suggestions and opening folders. |
| `src/config.rs`, `src/startup.rs` | Settings file, HKCU Run value. |
| `packaging/QuickSwitch.iss` | Inno Setup installer. |
| `packaging/README.txt` | The README shipped to users (installer and portable ZIP). |
| `docs/plans/implemented/` | The implementation plan this follows. |

## 7. License

[MIT](LICENSE)
