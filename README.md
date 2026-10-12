# rustclip

A Rust desktop application for clipboard history. Its priorities are responsiveness, stability, and reliable handling of text and images. Windows is the first target platform.

**Status:** an initial Windows prototype is implemented. It uses native Win32 controls and a system tray icon, without WebView or an embedded browser. Text history is saved locally; image handling, search, quick selection, configurable shortcuts, and automatic paste are still planned.

## Build and run

On Windows with Rust installed:

```text
cargo run
```

The tray menu opens history, clears saved history, and exits. Double-clicking the tray icon also opens history. `Ctrl+Shift+V` is the current provisional shortcut for the history window. Select an entry and click **Copy to clipboard** to restore its text. Closing the window leaves rustclip running in the tray. History is stored under `%LOCALAPPDATA%\rustclip\history` and currently has a fixed 64 MiB accounting cap (text bytes plus 64 bytes per entry). Each text entry is limited to 8 MiB. The prototype window currently lists only the newest 1,000 entries.

## Main workflow

1. The application saves new clipboard content to a persistent history.
2. A configurable global shortcut will open the main window for browsing and searching history.
3. `Ctrl+Shift+Up` will open a compact preview above other windows. Arrow keys will browse entries; releasing Ctrl or Shift will put the selected entry on the clipboard and paste it into the original window.
4. Selecting an older entry must not create a new history entry.

The planned settings will allow users to configure every keyboard command and the maximum history size. Application context will support application-specific rules later.

## Language

All project text must be in English, including the interface, settings, notifications, errors, documentation, code comments, and other user-facing or developer-facing text. Conversation with the project owner is in Russian.

## Documentation

- [Requirements and acceptance criteria](docs/requirements.md)
- [Technical notes](docs/technical-notes.md)
- [Agent instructions](AGENTS.md)
