# rustclip

A Rust desktop application for clipboard history. Its priorities are responsiveness, stability, and reliable handling of text and images. Windows is the first target platform.

**Status:** a Windows prototype now captures text and DIB images together, saves them locally, shows image thumbnails, and offers searchable history, nested text-template groups, editable shortcuts, paste into the original window, and a system tray. The dark Slint interface uses a compact menu, toolbar, History and Templates tabs, a dense list, bottom search, and a settings dialog with a section tree. It uses no WebView. The compact quick-selection preview, application-specific rules, and richer image formats are still planned.

## Build and run

On Windows with Rust installed:

```text
cargo run
```

The tray menu opens history, templates, or settings, clears saved history, and exits. Clicking the tray icon opens history. Provisional global shortcuts are Win+Shift+J (history), Win+Shift+K (settings), and Win+Shift+Y (templates). The history page can navigate, copy, paste, delete, and search clips. The templates page can create nested groups and text templates, edit, delete, search, copy, and paste them. Settings include start in tray, start with Windows, independent Win/Ctrl/Alt/Shift shortcut modifiers, paste behavior, and a 16–4096 MiB history limit. The initial limit is 64 MiB. History is stored under %LOCALAPPDATA%\rustclip\history, templates under %LOCALAPPDATA%\rustclip\templates.json, and settings under %LOCALAPPDATA%\rustclip\settings.conf. The window shows the newest 1,000 entries and searches up to the first 32,000 characters of each entry. Text is limited to 8 MiB per entry and raw DIB images to 48 MiB.

## Main workflow

1. The application saves supported text and image representations from each clipboard event as one persistent entry.
2. A configurable global shortcut opens history; selecting a clip can copy it or paste it into the window that was active before rustclip opened.
3. Templates store reusable text in nested groups. Copying or pasting a template does not add a history entry.
4. A compact quick-selection preview is planned; it will navigate and paste without opening the full window.

Clipboard restoration after automatic paste is optional and off by default. When enabled, rustclip first captures all clipboard formats it can safely preserve. It refuses the paste if the old clipboard cannot be preserved, and it does not restore over a newer external copy. Windows runtime behavior still needs testing on a Windows machine.

## Language

All project text must be in English, including the interface, settings, notifications, errors, documentation, code comments, and other user-facing or developer-facing text. Conversation with the project owner is in Russian.

## Documentation

- [Requirements and acceptance criteria](docs/requirements.md)
- [Technical notes](docs/technical-notes.md)
- [Agent instructions](AGENTS.md)
