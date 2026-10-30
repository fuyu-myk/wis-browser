# Changelog

What changed in each Wis release.

## 0.4.1

### Fixed

- Wis opens on macOS 14 again. Version 0.4.0 quit at launch there, and likely on macOS 15 before 15.4 too, because it linked WebKit extension APIs those versions lack.

### Platform notes

- On macOS 14, 0.4.0 cannot open to update itself, so download 0.4.1 from this release.

## 0.4.0

### Added

- **Canvas**. Canvases are whiteboard tabs for notes, planning, and work. Cards are like sticky notes and they can be linked together, framed, grouped, and pointed at each other with arrows. Live HTML cards are also available for both static and dynamic visuals. The built-in agent can lay out cards, search the web, and read attached text, PDF and Word files. You can also dictate into a canvas to control what the agent does. On top of that, canvases have undo, backups and organization. All features run on macOS, while Windows has no HTML cards, dictation or document reading, and Linux has no agent, dictation or document reading. HTML cards require WebKitGTK 2.42 or newer on Linux.
- **Extensions (macOS)**. Install WebExtensions from a folder or straight from the Chrome Web Store, with automatic updates. You can pin extensions, set per-site running rules, turn them on per workspace (private workspaces included) and run them from the toolbar menu.
- **Assistant**.
  - A research mode and a model picker are built into the panel, the former requiring web access to be enabled.
  - Using opt-in tools, the assistant can conduct web searches, read your tabs, history and bookmarks, and manage tabs when prompted. Results vary depending on the model.
- **Privacy**.
  - The site panel lets you change blocking per site. On macOS, it also shows what was blocked on the page and lets you switch JavaScript off.
  - Tracking parameters are stripped from links as you follow them.
  - Trackers hidden behind CNAME records are now caught.
  - New switches for cookie notice and annoyance lists, plus uBlock Origin scriptlets.
  - WebKit fingerprinting protection is on in every mode except Permissive (macOS).
  - Lockdown Mode can be turned on per workspace (macOS 14 or later).
- **Vault**.
  - Wis saves and fills addresses and payment cards, even on dynamic forms. Cards' security codes are never stored.
  - Wis offers to update saved passwords when it detects changes.
  - You can recover a vault with a recovery key, or reset it.
  - On Linux, vaults can be sealed with a vault password.
- **Usage**. Wis tracks active time per site, shown in a docked panel and on a weekly page with trends.
- **Import**. Bookmarks and recent history can be imported from Chromium-family browsers, Firefox, Zen and LibreWolf.
- **Recently closed**.
  - Browsing history now houses a dedicated view that lists closed tabs.
  - Idle tabs can be archived into it automatically.
  - Cmd+Shift+T (Ctrl+Shift+T on Windows and Linux) now reopens Wis pages and canvases too.
- **Link routing**. Links opened from other apps can go to a chosen workspace by site.
- **Settings**. The settings page now features a search bar, sticky section titles, and a refreshed layout.
- **Accessibility**.
  - Every control is reachable with the keyboard.
  - F6 cycles focus between areas of the window.
  - Tabs, widgets and panes can be moved or resized from the keyboard, and tabs can be moved into and out of folders.
  - Screen readers hear toasts, downloads and confirmations.
  - Reduced motion is honored.

### Changed

- On macOS, the window chrome and overlays render at the display's full refresh rate.
- Secondary text has more contrast in both light and dark mode.
- Model and server fields in the assistant settings use themed suggestion lists.
- Local pages and loopback addresses no longer show the "not secure" mark.

### Fixed

- Tabs in a private workspace now share one cookie store, so signing in carries across them.
- New workspaces use the default privacy mode.
- Compiled content-blocking lists survive a relaunch.
- Extension content scripts no longer run twice after a list change.
- Arrow keys no longer type stray characters into empty fields on macOS.
- Renaming a folder from its menu works again.
- Keyboard focus moves into menus and panels when they open and returns when they close. This now includes the site panel.
- The find bar's match counter on macOS is correct.
- History search treats wildcard characters literally.
- Usage views refresh after clearing history.
- Favicon fallbacks and some hover colors are fixed.

### Platform notes

- Extensions are macOS-only for now.
- The assistant is not available on Linux yet due to the lack of a key store.
- Focus return after menus and panels close is built for Windows and Linux but has only been tested on macOS.
- Builds are not code-signed yet; the README explains how to open Wis the first time.
