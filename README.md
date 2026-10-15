# rustclip

A Rust desktop application for clipboard history. Its priorities are responsiveness, stability, and reliable handling of text and images. Windows is the first target platform.

**Status:** a Windows prototype now captures text and DIB images together, saves them locally, shows image thumbnails, and offers searchable history, nested text-template groups, editable shortcuts, paste into the original window, and a system tray. The dark Slint interface has permanent History, Templates, and Settings navigation, a compact action toolbar, a dense history list, and a separate selection preview. It uses no WebView. Application-specific rules and richer image formats are still planned.

## Build and run

On Windows with Rust installed:

```text
cargo run
```

Opening the executable manually shows history immediately. Opening it again raises the same window and keeps one tray icon and one clipboard monitor. The Start in tray preference applies to Windows startup, which launches with `--background`.

The tray menu opens history, templates, or settings and exits. Clicking the tray icon opens history. Default global shortcuts are Win+Shift+H (main history), Alt+Shift+Up (older clip), Alt+Shift+Down (newer clip), and Alt+Shift+Esc (cancel quick selection). Settings and Templates have no global shortcut by default. The history page can navigate, copy, paste, delete, and search clips. A selected clip appears in a preview to the left of the main window; double-clicking its row pastes it. Clearing all history is available only from Settings > Clipboard history. The templates page can create nested groups and text templates, edit, delete, search, copy, and paste them. Settings include start in tray, start with Windows, independent Win/Ctrl/Alt/Shift shortcut modifiers, paste behavior, and a 16–4096 MiB history limit. The initial limit is 64 MiB. History is stored under %LOCALAPPDATA%\rustclip\history, templates under %LOCALAPPDATA%\rustclip\templates.json, and settings under %LOCALAPPDATA%\rustclip\settings.conf. The window shows the newest 1,000 entries and searches up to the first 32,000 characters of each entry. Text is limited to 8 MiB per entry and raw DIB images to 48 MiB.

## Main workflow

1. The application saves supported text and image representations from each clipboard event as one persistent entry.
2. A configurable global shortcut opens history; selecting a clip can copy it or paste it into the window that was active before rustclip opened.
3. Templates store reusable text in nested groups. Copying or pasting a template does not add a history entry.
4. Hold Alt+Shift and press Up to browse older clips or Down to return toward newer clips. The first Up selects the clip before the newest one. A compact preview appears at the center of the active monitor when the main window is hidden, without taking focus. Releasing a required modifier pastes only the selected entry after all modifiers are released. Alt+Shift+Esc cancels without changing the clipboard. Quick paste requires the paste setting to be enabled.

Clipboard restoration after automatic paste is optional and off by default. When enabled, rustclip first captures all clipboard formats it can safely preserve. It refuses the paste if the old clipboard cannot be preserved, and it does not restore over a newer external copy. Windows integration still needs interactive checks with target applications, multiple monitors, and different DPI settings.

## Language

All project text must be in English, including the interface, settings, notifications, errors, documentation, code comments, and other user-facing or developer-facing text. Conversation with the project owner is in Russian.

## Documentation

- [Requirements and acceptance criteria](docs/requirements.md)
- [Technical notes](docs/technical-notes.md)
- [Agent instructions](AGENTS.md)

## Code organization

`ui/main.slint` contains the window shell and navigation. History, templates, settings, shortcut editing, the preview, and tray each have their own Slint component. Layouts control sizing and spacing.

`src/windows_app.rs` starts the application and connects controllers. The `src/windows_app/` modules separate history, settings, templates, keyboard handling, preview behavior, Win32 monitoring, window placement, and background storage work. `src/history_selection.rs` contains the platform-independent browsing rules.

Run `cargo test` for data and navigation tests. On Windows, the tests also dispatch pointer events through the real UI and save review images to `target/ui-review/`. They use a test command queue and do not change the system clipboard.
