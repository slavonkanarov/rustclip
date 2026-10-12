# rustclip requirements

## Goal and first-release scope

rustclip is a local Rust application for Windows that saves clipboard history and lets users quickly paste previously copied text and images. Predictable, responsive operation in the background and during invocation is the main quality goal.

The first release includes persistent text and image history, nested text templates, a searchable main window, a compact selection preview above other windows, configurable keyboard commands, a storage size limit, active window detection, optional automatic paste, and a system tray icon. Per-application rules are planned for later.

The application must use a native desktop window and system tray integration. A Rust desktop UI toolkit may draw the controls. WebView, embedded browsers, and browser-based UI frameworks are not allowed.

## Behavior

### Background operation and system tray

- The application runs in the background with a visible system tray icon. Closing the main window hides it without stopping clipboard capture.
- The tray menu opens history, templates, and settings; clears history; and provides an explicit Exit action. Exiting removes the tray icon and releases Windows registrations.
- The application uses native desktop windows and tray integration; no WebView or embedded browser is created.

### Clipboard history

- On a new **external** clipboard change, the application reads available supported representations and saves one entry for that copy event. Text and images are required in the first release.
- If an event contains multiple supported representations, they belong to one entry. Selecting it should restore those representations as far as Windows permits.
- Empty, unsupported, oversized, or malformed data must not freeze or crash the application. An unknown format must not prevent supported representations from the same event from being processed.
- Writing a selected older entry to the clipboard must not add a history entry or move it in the history as a new copy.
- An external copy of content identical to a saved entry is a new event. Optional duplicate merging is a separate future decision.
- History persists across restarts. Entries saved before a failure must remain readable.

### Templates

- Users can create root and nested groups and save reusable text templates in them. Groups and templates persist across restarts.
- Users can rename, edit, search, copy, paste, and delete templates. Deleting a group removes its descendants after confirmation.
- Moving to another template or creating an item while edits are unsaved asks before discarding those edits.
- Copying or pasting a template is a self-initiated clipboard write and must not create a history entry.

### Main window

- A configurable global shortcut opens the window, showing history from newest to oldest.
- The dark native window follows a compact clipboard-manager layout: a menu bar, a small action toolbar, History and Templates tabs, a dense full-width list, and a search field along the bottom. Selection remains readable without oversized cards or a persistent navigation sidebar.
- Users can browse entries, preview text or image thumbnails, and search saved text.
- Users can copy a selected entry or paste it into the original window. Single click selects an entry; Enter copies it. A separate Paste action performs Ctrl+V when enabled in settings.
- Search, scrolling, and navigation remain responsive when storage is full; loading an image preview must not block input.

### Quick selection above other windows

- A configurable global shortcut opens a compact preview above the current window. While its invocation modifiers are held, Up and Down browse history. Releasing any required modifier commits the selection: the entry is placed on the clipboard and pasted into the original window.
- `Esc` cancels selection without changing the clipboard. The preview shows enough text or an image thumbnail to distinguish entries.
- The original window is identified before rustclip UI appears. If that window closes or cannot be used safely, automatic paste is skipped; the selected entry remains on the clipboard and the user can tell that paste did not occur.
- Empty history, browsing beyond the ends, loss of focus, and repeated shortcut presses must not lose data or freeze the application. The initially selected entry on the first press remains undecided.

### Keyboard commands, storage, and window context

- Every keyboard command can be changed in settings. Each binding has an independent Win modifier alongside Ctrl, Alt, and Shift. Conflicts and Windows global registration failures are shown to the user. Provisional global defaults are Win+Shift+J for history, Win+Shift+K for settings, and Win+Shift+Y for templates.
- Users can set a 16–4096 MiB maximum for local history. Text, images, and metadata count toward the limit; the default is 64 MiB. Oldest entries are removed when the limit is lowered or reached. A new entry larger than the limit is skipped without clearing existing history.
- The application captures the active target window before showing its UI and tracks external foreground changes. Paste is skipped if the original window closes, belongs to rustclip, or cannot be activated.
- Application identity supports future rules such as excluding an application from history. A rule configuration UI is not required in the first release.

### General and paste settings

- Settings open as a dark dialog over the main window, with a section tree on the left, a section panel on the right, and OK and Cancel actions. The keyboard section presents a compact command table and separate Win, Ctrl, Alt, and Shift controls for the selected command.
- Users can choose whether the main window opens at startup or rustclip starts hidden in the tray, and whether it starts with Windows for the current user.
- Users can enable automatic Ctrl+V paste, choose a compatibility delay from 0 to 2000 ms, and request restoration of the previous clipboard after paste.
- With restoration enabled, rustclip must capture the old clipboard safely before changing it. If it cannot preserve every available format, it declines the paste. It never restores over a newer external clipboard change. Restoration failures are reported.

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
7. Every keyboard command can be rebound, including an independent Win modifier. Bindings persist after restart; conflicts and global shortcut registration failures are reported clearly.
8. Quick selection identifies the original window. Paste reaches that window, or the entry safely remains only on the clipboard if the window is unavailable.
9. All visible application text and all project documentation are in English; communication with the project owner remains in Russian.
10. The application remains available in the system tray when its main window is closed. The tray menu opens history, templates, and settings, clears history, and exits cleanly; no WebView or embedded browser is used.

11. Nested template groups and text templates persist after restart; deleting a group removes its descendants after confirmation, and copying or pasting a template does not create a history entry.
12. Start-in-tray, start-with-Windows, paste, restoration, delay, and history-limit settings persist after restart. A paste into a closed or inaccessible original window is skipped safely.
13. The main window keeps the menu, action toolbar, History and Templates tabs, dense rows, and bottom search visible at its minimum size. The settings dialog keeps its section tree, controls, and OK and Cancel actions usable at that size.

## Open decisions

- Initial quick-preview selection: the latest entry or the entry preceding the current clipboard contents.
- Final default shortcuts and mouse behavior for the quick preview.
- Whether a separate entry-count limit is needed alongside the 64 MiB default size limit.
- Whether additional formats such as HTML/RTF or files need exact preservation. Text and images are required in the first release; other formats must be handled safely.
- Whether to persist the source window title. Application identity and a temporary window identifier are needed for current context; titles may contain sensitive information.
