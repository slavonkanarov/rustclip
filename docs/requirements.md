# rustclip requirements

## Goal and first-release scope

rustclip is a local Rust application for Windows that saves clipboard history and lets users quickly paste previously copied text and images. Predictable, responsive operation in the background and during invocation is the main quality goal.

The first release includes persistent history, a searchable main window, a compact selection preview above other windows, configurable keyboard commands, a storage size limit, active window and application detection, and a system tray icon. Per-application rules, including ignoring an application, are planned for later; the design must allow them to be added.

The desktop interface must use native Windows UI components. WebView, embedded browsers, and browser-based UI frameworks are not allowed.

## Behavior

### Background operation and system tray

- The application runs in the background with a visible system tray icon. Closing the main window hides it without stopping clipboard capture.
- The tray menu opens the main window and provides an explicit Exit action. Exiting removes the tray icon and releases Windows registrations.
- Tray controls and all other application windows use native Windows UI components; no WebView or embedded browser is created.

### Clipboard history

- On a new **external** clipboard change, the application reads available supported representations and saves one entry for that copy event. Text and images are required in the first release.
- If an event contains multiple supported representations, they belong to one entry. Selecting it should restore those representations as far as Windows permits.
- Empty, unsupported, oversized, or malformed data must not freeze or crash the application. An unknown format must not prevent supported representations from the same event from being processed.
- Writing a selected older entry to the clipboard must not add a history entry or move it in the history as a new copy.
- An external copy of content identical to a saved entry is a new event. Optional duplicate merging is a separate future decision.
- History persists across restarts. Entries saved before a failure must remain readable.

### Main window

- A configurable global shortcut opens the window, showing history from newest to oldest.
- Users can browse entries, preview text or image thumbnails, and search saved text.
- Users can return a selected entry to the clipboard and paste it into the target window. The exact Enter and mouse-click behavior must be decided before implementation.
- Search, scrolling, and navigation remain responsive when storage is full; loading an image preview must not block input.

### Quick selection above other windows

- `Ctrl+Shift+Up` opens a compact preview above the current window. While Ctrl and Shift are held, `Up` and `Down` browse history. Releasing **either** modifier commits the selection: the entry is placed on the clipboard and pasted into the original window.
- `Esc` cancels selection without changing the clipboard. The preview shows enough text or an image thumbnail to distinguish entries.
- The original window is identified before rustclip UI appears. If that window closes or cannot be used safely, automatic paste is skipped; the selected entry remains on the clipboard and the user can tell that paste did not occur.
- Empty history, browsing beyond the ends, loss of focus, and repeated shortcut presses must not lose data or freeze the application. The initially selected entry on the first press remains undecided.

### Keyboard commands, storage, and window context

- Every keyboard command can be changed in settings: opening the main window, invoking quick selection, browsing and cancelling there, searching, and navigating the main window. Releasing a modifier to commit quick selection belongs to the chosen invocation shortcut; it is not a separate key binding. Conflicts with other rustclip commands and Windows global shortcut registration failures are shown to the user.
- Users can set the maximum size of local history. Text, images, and metadata count toward the limit. The oldest entries are removed when the limit is reached; a new entry larger than the limit is not saved. Lowering the limit also applies to existing history.
- The application determines the active window and application on copy, quick-selection invocation, and focus changes when Windows provides that information. The paste target is captured before rustclip shows its own UI.
- Application identity supports future rules such as excluding an application from history. A rule configuration UI is not required in the first release.

## Language policy

- All application text must be in English, including window titles, menus, settings, notifications, errors, and help text.
- Documentation, code comments, test names, and other project text must also be in English. Conversation with the project owner is in Russian.

## Acceptance criteria

1. Text copied from another application appears in history, survives a restart, and can be pasted again.
2. An image appears with a thumbnail, survives a restart, and returns to the clipboard in a form that can be pasted.
3. Selecting an old entry in either UI does not add an entry or change history order. A later external copy of the same text is saved as a new event.
4. A rapid series of copies does not freeze the UI or corrupt saved history.
5. An empty clipboard, unknown or malformed format, and large image do not terminate the process; later supported events continue to be handled.
6. Lowering the storage limit removes old entries until the history fits. An individually oversized new entry is skipped without clearing all history.
7. Every keyboard command can be rebound; conflicts and global shortcut registration failures are reported clearly.
8. Quick selection identifies the original window. Paste reaches that window, or the entry safely remains only on the clipboard if the window is unavailable.
9. All visible application text and all project documentation are in English; communication with the project owner remains in Russian.
10. The application remains available in the system tray when the main window is closed. The tray menu opens history and exits cleanly, and no WebView or embedded browser is used.

## Open decisions

- Initial quick-preview selection: the latest entry or the entry preceding the current clipboard contents.
- Default shortcut for the main window, and Enter and mouse-click behavior within it.
- Default storage limit and whether a separate entry-count limit is needed.
- Whether additional formats such as HTML/RTF or files need exact preservation. Text and images are required in the first release; other formats must be handled safely.
- Whether to persist the source window title. Application identity and a temporary window identifier are needed for current context; titles may contain sensitive information.
