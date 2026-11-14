QuickSwitch
===========

Press Ctrl + Win + K, type, press Enter:

  - switch to any open window (minimized windows are restored)
  - switch to an individual File Explorer tab
  - switch to a tab in Chrome, Edge, Brave, Vivaldi, Opera or Firefox
  - switch to a Windows Terminal tab
  - open a folder by typing its path, e.g. ~/Documents or C:/dev/work

QuickSwitch runs in the notification area (system tray, bottom right of the
taskbar; it may be under the ^ arrow) until you choose Exit.


Searching
---------

  Type                Filters open windows, Explorer tabs, browser tabs and
                      folders you have used before. Every word you type must match, in
                      any order. Matched letters are highlighted.
  Up / Down, PgUp / PgDn
                      Move through the results (Tab also moves).
  Enter or click      Switch to the window or tab, or open the folder or file.
  Shift + Enter       On a file: show it in File Explorer instead.
  Ctrl + Enter        Search the web for what you typed.
  Ctrl + V            Paste. Quotes around a pasted path (Explorer's
                      "Copy as path") are removed.
  Esc, click elsewhere
                      Hide the popup. QuickSwitch keeps running.

Windows are found by their title or app name. Explorer tabs are found by the
folder they show: each tab row shows that folder's path, so typing any folder
name along the path finds it ("peace plans" finds ~/Projects/Peace-of-mind/
docs/plans). Your home folder is written as ~.

Browser tabs are found by their title or the browser's name ("edge mail"
finds the mail tab in Edge). With nothing typed, each browser window shows as
one row for the tab it is showing; its other tabs appear as soon as you type.
Web addresses are not searched, only titles.

Windows Terminal tabs work the same way. Their titles come from the shell,
so several tabs are often just "PowerShell"; those rows also show the tab
number ("Terminal · tab 3"), which you can type too.

Web search: what you type also gets a row "Search Google for ..." (or
"Visit ..." when it is a web address like github.com/x). It comes last, or
first when nothing else matches, so Enter still goes to your best match;
Ctrl + Enter searches the web from any row. "web: ..." puts the web row
first, and "gpt: ..." asks ChatGPT instead. Pick another search engine in
the tray menu's Settings...; untick "Web search in results" to hide the row.

To search one kind of result only, start with a filter word and a colon:

  browser: mail       browser tabs only (also web:)
  window: code        windows only
  explorer: plans     Explorer tabs only
  terminal: pwsh      Windows Terminal tabs only
  tab: x              all kinds of tab
  folder: x           folders (remembered ones and open Explorer tabs)

A filter on its own ("browser:") lists everything of that kind. The first
two or more letters are enough ("br:", "term:").

QuickSwitch also remembers folders you opened with it or had open in
Explorer, so you can find them again later even when no Explorer window shows
them.


Opening folders
---------------

Start typing with one of these to browse folders instead of searching:

  ~  or  ~/docs            your user folder (C:\Users\<you>), and below it
  ./notes                  also relative to your user folder
  C:/dev/work  C:\dev  D:  a drive path (/ and \ both work)
  %appdata%/quickswitch    a folder from an environment variable
  //server/share           a network folder

As you type, the popup lists the matching subfolders, like a wildcard "ls":
"~/proj" lists "Projects", "projects-old" and so on. Press Tab to complete the
selected folder and keep typing the next level, or Enter to open it. Hidden
folders (such as AppData) appear once you type the start of their name.

Paste or type the full path of a file to open it: the file is the first row,
Enter opens it with its default app (as a double-click would), and
Shift + Enter shows it in File Explorer instead.

If the folder is already open in an Explorer tab, Enter switches to that tab
instead of opening another window. Network folders are not listed while you
type; they are checked when you press Enter.


Tray icon
---------

  Left-click     Open search.
  Right-click    Open search, Settings..., Start with Windows (checkbox),
                 Search browser tabs (checkbox), Web search in results
                 (checkbox), Exit QuickSwitch.

Settings... has three tabs (Ctrl + Tab switches).

General sets how many results to show (default 15), whether to search
browser and Windows Terminal tabs, the web search row and Start with
Windows. It can also forget your recent picks or remembered folders.

Shortcut lets you pick up to three modifier keys (Ctrl, Alt,
Shift, Win) and a key from dropdowns. Save switches to it right away. If
Windows or another app already uses that combination, your old shortcut
keeps working and QuickSwitch tells you to pick another.

The Web search tab picks where web searches go: Google (default), Bing,
DuckDuckGo, Brave Search, Ecosia, Startpage, Kagi, Yahoo, Perplexity,
ChatGPT, Claude, or your own address with {} where the text goes.

"Search browser tabs" is on unless you turn it off. To read tab titles,
QuickSwitch uses Windows accessibility support, which makes the browser use a
little more memory. Turn it off if you don't need browser tabs.

"Start with Windows" is off unless you turn it on (here or in the installer).
It always shows the real setting. If you use the portable version and move
its folder, turn the setting off and on again so it points at the new place.


Settings
--------

Settings are in config.toml: %APPDATA%\QuickSwitch\config.toml, or next to
QuickSwitch.exe in the portable version. Settings... changes all of these
except fallback_hotkey, so you rarely need to edit it. Every key is optional.
Restart QuickSwitch after editing it (tray menu > Exit, then start it again).

  hotkey = "Ctrl+Win+K"               main shortcut
  fallback_hotkey = "Ctrl+Alt+Space"  optional second shortcut
  max_results = 15                    rows in the popup (1-50)
  browser_tabs = true                 list browser tabs
  terminal_tabs = true                list Windows Terminal tabs

A shortcut is modifiers (Ctrl, Alt, Shift, Win) joined with +, then one key
(K, Space, F1, ...). If another app already uses the shortcut, QuickSwitch
says so once and keeps running; open it from the tray icon and use
Settings..., or set fallback_hotkey.

QuickSwitch also stores the results you picked recently ("recent") and the
folders it remembers ("folders") in this file. Browser tab titles are not
stored, only a code made from them. You can delete those lines to
clear them.

Problems are written to quickswitch.log in the same folder.


Portable version
----------------

The file QuickSwitch.portable next to QuickSwitch.exe keeps the settings and
the log in that folder. Delete it to use %APPDATA%\QuickSwitch instead.


Good to know
------------

  - Windows decides whether an app may bring another window to the front.
    QuickSwitch only does it right after your key press or click and never
    fakes keystrokes; if Windows refuses, the popup shows a message.
  - Windows of programs running as administrator may refuse to be switched
    to. Windows on other virtual desktops are not listed.
  - Tabs inside a collapsed tab group may not be listed until you expand it.
  - QuickSwitch is not code-signed, so Windows SmartScreen may warn the first
    time you run it or its installer.


Uninstall
---------

Installed version: Settings > Apps > Installed apps > QuickSwitch >
Uninstall. This also removes the "Start with Windows" entry.

Portable version: turn off "Start with Windows" in the tray menu if you turned
it on, exit QuickSwitch, then delete its folder.
