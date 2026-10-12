# rustclip

A planned Rust desktop application for clipboard history. Its priorities are responsiveness, stability, and reliable handling of text and images. Windows is the first target platform.

**Status:** this repository currently contains documentation only. The application, libraries, and storage format have not been implemented or selected.

## Main workflow

1. The application saves new clipboard content to a persistent history.
2. A configurable global shortcut opens the main window for browsing and searching history.
3. `Ctrl+Shift+Up` opens a compact preview above other windows. Arrow keys browse entries; releasing Ctrl or Shift puts the selected entry on the clipboard and pastes it into the original window.
4. Selecting an older entry does not create a new history entry.

Users can configure every keyboard command and the maximum history size. The application tracks the active window and application so that application-specific rules can be added later.

## Language

All project text must be in English, including the interface, settings, notifications, errors, documentation, code comments, and other user-facing or developer-facing text. Conversation with the project owner is in Russian.

## Documentation

- [Requirements and acceptance criteria](docs/requirements.md)
- [Technical notes](docs/technical-notes.md)
- [Agent instructions](AGENTS.md)
