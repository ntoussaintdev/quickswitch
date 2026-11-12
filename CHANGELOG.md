# Changelog

What's new in each QuickSwitch release. Downloads are on the [Releases page](https://github.com/ntoussaintdev/quickswitch/releases).

<!--
Each release needs a section headed "## [X.Y.Z] - YYYY-MM-DD" before its tag is pushed.
The Release workflow publishes that section as the GitHub release notes and fails without it.
-->

## [0.4.2] - 2026-11-11

### Added
- **Paste** into the search box with Ctrl+V (or Shift+Insert). Quotes from Explorer's "Copy as path" are removed, so a pasted path opens as a folder. Copying out of the box works too.
- **Screen reader support**: Narrator, NVDA and JAWS can read the search box, the results and Settings. Moving through results with the arrow keys reads out each row.
- **Text in other scripts and emoji** in window and tab titles (Chinese, Japanese, Arabic, ...) now shows properly instead of as boxes, using Windows' own fonts.

## [0.4.1] - 2026-11-11

### Added
- **General tab in Settings** (now the first tab): how many results to show, browser and Terminal tab search, the web search row, Start with Windows, and buttons to forget recent picks or remembered folders. No need to edit `config.toml`.

### Changed
- QuickSwitch now shows up to 15 results by default instead of 9, and you can choose up to 50 (was 20). If your settings file already has a number, it is kept. Change it in Settings → General.

### Fixed
- Pages titled like `owner/repo` (GitHub and similar) now rank higher when you search for the repo name. Before, a recently used folder whose path only contained that word could push them out of the results.

## [0.4.0] - 2026-11-11

Search your browser and Windows Terminal tabs, narrow results by type, and search the web straight from QuickSwitch.

### Added
- **Browser tabs** from Chrome, Edge, Brave, Vivaldi, Opera and Firefox-based browsers appear in results with a "Browser tab" badge. Pick one to jump straight to that tab. No browser extension needed.
- **Windows Terminal tabs** appear in results with a "Terminal tab" badge. Tabs with the same title (like "PowerShell") show their tab number.
- **Type filters**: start your search with `browser:` (or `web:`), `window:`, `explorer:`, `terminal:`, `tab:` or `folder:` (or `dir:`) to show only that kind of result. Two letters are enough, e.g. `br:`.
- **Web search**: a "Search Google for ..." row for whatever you type, or "Visit ..." when it looks like a web address. Ctrl+Enter runs it from any row. `gpt:` asks ChatGPT.
- **Settings window** (tray menu → Settings...) with a Shortcut tab and a Web search tab. Choose Google, Bing, DuckDuckGo, Brave Search, Ecosia, Startpage, Kagi, Yahoo, Perplexity, ChatGPT, Claude, or your own search address.
- Tray checkboxes to turn browser tab search and web search on or off.

### Changed
- "Settings..." replaces "Change shortcut..." in the tray menu.
- When nothing matches a filter, the message says what's missing (for example "No Terminal tabs are open").

### Privacy
- Recent picks store a hash of a browser tab's title, never the title itself.

## [0.3.0] - 2026-11-10

### Added
- Change the keyboard shortcut from the tray menu ("Change shortcut..."). The new shortcut works right away, with no restart. If the combination is already taken, the old shortcut keeps working and QuickSwitch tells you why.

## [0.2.1] - 2026-11-10

### Added
- Open folders from paths that start with an environment variable, like `%appdata%/quickswitch`, with suggestions and Tab completion.

## [0.2.0] - 2026-11-10

### Added
- File Explorer tabs show their folder path, and tabs in minimized windows are included.
- Typing a folder path lists matching subfolders as you go. Tab completes, Enter opens. If the folder is already open in a tab, QuickSwitch switches to that tab instead.
- Folders you've opened or seen in Explorer are remembered and searchable.
- Matched letters are highlighted in results.

### Changed
- Every word you type must match, so results are more precise. Tabs and folders match on their path.
- More reliable switching between Explorer tabs with the same name.

## [0.1.0] - 2026-11-10

First release.

- Press Ctrl+Win+K to open a search box over all open windows and individual File Explorer tabs, then pick one to switch to it.
- Fuzzy search, with recently used items ranked higher.
- Type a folder path (`~/`, `C:/`, `//server/share`) to open it.
- Lives in the system tray, with an optional "Start with Windows" setting.
- Per-user installer or portable ZIP.
