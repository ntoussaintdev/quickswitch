QuickSwitch
===========

Press Ctrl + Win + K, type, press Enter:

  - switch to any open window (minimized windows are restored)
  - switch to an individual File Explorer tab
  - open a folder by typing its path, e.g. ~/Documents or C:/dev/work

QuickSwitch runs in the notification area (system tray, bottom right of the
taskbar; it may be under the ^ arrow) until you choose Exit.


Searching
---------

  Type                Filters open windows, Explorer tabs and folders you
                      have used before. Every word you type must match, in
                      any order. Matched letters are highlighted.
  Up / Down, PgUp / PgDn
                      Move through the results (Tab also moves).
  Enter or click      Switch to the window or tab, or open the folder.
  Esc, click elsewhere
                      Hide the popup. QuickSwitch keeps running.

Windows are found by their title or app name. Explorer tabs are found by the
folder they show: each tab row shows that folder's path, so typing any folder
name along the path finds it ("peace plans" finds ~/Projects/Peace-of-mind/
docs/plans). Your home folder is written as ~.

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

If the folder is already open in an Explorer tab, Enter switches to that tab
instead of opening another window. Network folders are not listed while you
type; they are checked when you press Enter.


Tray icon
---------

  Left-click     Open search.
  Right-click    Open search, Change shortcut..., Start with Windows
                 (checkbox), Exit QuickSwitch.

"Change shortcut..." lets you pick up to three modifier keys (Ctrl, Alt,
Shift, Win) and a key from dropdowns. Save switches to it right away. If
Windows or another app already uses that combination, your old shortcut
keeps working and QuickSwitch tells you to pick another.

"Start with Windows" is off unless you turn it on (here or in the installer).
It always shows the real setting. If you use the portable version and move
its folder, turn the setting off and on again so it points at the new place.


Settings
--------

Settings are in config.toml: %APPDATA%\QuickSwitch\config.toml, or next to
QuickSwitch.exe in the portable version. Every key is optional. Restart
QuickSwitch after editing it (tray menu > Exit, then start it again).

  hotkey = "Ctrl+Win+K"               main shortcut
  fallback_hotkey = "Ctrl+Alt+Space"  optional second shortcut
  max_results = 9                     rows in the popup (1-20)

A shortcut is modifiers (Ctrl, Alt, Shift, Win) joined with +, then one key
(K, Space, F1, ...). If another app already uses the shortcut, QuickSwitch
says so once and keeps running; open it from the tray icon and use
"Change shortcut...", or set fallback_hotkey.

QuickSwitch also stores the results you picked recently ("recent") and the
folders it remembers ("folders") in this file. You can delete those lines to
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
  - QuickSwitch is not code-signed, so Windows SmartScreen may warn the first
    time you run it or its installer.


Uninstall
---------

Installed version: Settings > Apps > Installed apps > QuickSwitch >
Uninstall. This also removes the "Start with Windows" entry.

Portable version: turn off "Start with Windows" in the tray menu if you turned
it on, exit QuickSwitch, then delete its folder.
