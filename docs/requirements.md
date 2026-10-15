# rustclip requirements

## Goal and first-release scope

rustclip is a local Rust application for Windows that saves clipboard history and lets users quickly paste previously copied text and images. Predictable, responsive operation in the background and during invocation is the main quality goal.

The first release includes persistent text and image history, nested text templates, a searchable main window, a compact selection preview above other windows, configurable keyboard commands, a storage size limit, active window detection, optional automatic paste, and a system tray icon. Per-application rules are planned for later.

The application must use a native desktop window and system tray integration. A Rust desktop UI toolkit may draw the controls. WebView, embedded browsers, and browser-based UI frameworks are not allowed.

## Behavior

### Background operation and system tray

- Launching the executable manually opens the main history window, including when rustclip is already running in the tray. A second launch reuses the existing process instead of creating another clipboard monitor.
- Windows startup can launch rustclip in the tray when the corresponding option is enabled. This does not hide the main window on a manual launch.
- The application runs in the background with a visible system tray icon. Closing the main window hides it without stopping clipboard capture.
- The tray menu opens history, templates, and settings and provides an explicit Exit action. Exiting removes the tray icon and releases Windows registrations. Clearing all history is available only in Settings, with confirmation.
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
- The dark native window provides permanent, labeled History, Templates, and Settings navigation at the top, a compact action toolbar, a dense full-width list, and a search field along the bottom. Settings must open with one click from the main window, without visiting the tray. Layouts must remain usable at 760 × 560 logical pixels.
- Users can browse entries, preview text or image thumbnails, and search saved text. A separate preview window shows the selected content to the left of the main window while history is open.
- Users can copy a selected entry or paste it into the original window. Single click selects an entry; double-click pastes it when automatic paste is enabled. Enter copies it. A separate Paste action performs Ctrl+V when enabled in settings.
- Search, scrolling, and navigation remain responsive when storage is full; loading an image preview must not block input.

### Quick selection above other windows

- Two configurable global shortcuts browse older and newer entries directly from another application. Defaults are `Alt+Shift+Up` and `Alt+Shift+Down`. There is no separate invocation shortcut or focus transfer to the preview.
- History is ordered newest first. The first older command selects the entry immediately before the newest copy; another older command moves one entry further back. Newer reverses that movement. Navigation clamps at the ends.
- While modifiers are held, selection changes only the preview. Releasing a required modifier commits exactly one paste into the original application after all modifier keys are released. Quick paste requires automatic paste to be enabled. Intermediate selections must not modify the clipboard or the target application.
- The preview appears at the center of the active monitor when the main window is hidden, or to the left of the visible main window. It does not activate or take keyboard focus. It shows text or an image thumbnail, the position in history, and the release/cancel hint.
- The configurable cancel command defaults to `Alt+Shift+Esc`. Cancellation, a change of the target application, or an external clipboard change during selection must leave the clipboard unchanged. A new external copy resets browsing, including when its content matches an existing entry.
- The original target is captured before showing the preview. A closed or inaccessible target prevents paste. A newer clipboard event must not be overwritten by a delayed quick-paste request.
- Empty history and repeated commands must not freeze the application. Releasing modifiers with empty history does nothing.

### Keyboard commands, storage, and window context

- Every keyboard command can be changed in settings. Each binding has an independent Win modifier alongside Ctrl, Alt, and Shift. Conflicts within each command scope and Windows global registration failures are shown to the user. Defaults are Win+Shift+H for the main window and Alt+Shift+Up/Down for quick selection; Settings and Templates have no global shortcut by default.
- Users can set a 16–4096 MiB maximum for local history. Text, images, and metadata count toward the limit; the default is 64 MiB. Oldest entries are removed when the limit is lowered or reached. A new entry larger than the limit is skipped without clearing existing history.
- The application captures the active target window before showing its UI and tracks external foreground changes. Paste is skipped if the original window closes, belongs to rustclip, or cannot be activated.
- Application identity supports future rules such as excluding an application from history. A rule configuration UI is not required in the first release.

### General and paste settings

- Settings open as a page in the main window, with a section list on the left, a panel on the right, and Save settings and Cancel actions. Main navigation remains visible. The keyboard section presents a scrollable command list and separate Win, Ctrl, Alt, and Shift controls for the selected command.
- Users can choose whether Windows startup opens the main window or starts rustclip hidden in the tray, and whether it starts with Windows for the current user. Manual launches always show the main window. Settings is accessible directly from the permanent top navigation and includes General, Keyboard shortcuts, Clipboard history, and Paste settings.
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
8. Quick selection identifies the original window. Paste reaches that window, or fails safely without overwriting the previous clipboard before a valid target is confirmed.
9. All visible application text and all project documentation are in English; communication with the project owner remains in Russian.
10. The application remains available in the system tray when its main window is closed. The tray menu opens history, templates, and settings and exits cleanly; no WebView or embedded browser is used. Clearing all history requires the Clipboard history section in Settings.

11. Nested template groups and text templates persist after restart; deleting a group removes its descendants after confirmation, and copying or pasting a template does not create a history entry.
12. Start-in-tray, start-with-Windows, paste, restoration, delay, and history-limit settings persist after restart. A paste into a closed or inaccessible original window is skipped safely.
13. At the minimum window size, History, Templates, and Settings navigation remains visible and clickable. The keyboard settings list, modifier editor, and Save settings and Cancel actions remain usable.
14. Opening the executable manually shows the main window on the first launch and raises the existing main window on a second launch. The second launch does not create duplicate tray icons or clipboard monitors. Windows startup uses the saved tray preference.
15. Selecting a row shows its text or image preview to the left of the main window. The older/newer shortcuts show a centered preview on the active monitor while the main window is hidden; navigation changes its content, release pastes exactly once, and cancel leaves the clipboard unchanged.
16. Double-clicking an initially unselected history row pastes that exact entry once, including after a search. The first click must not recreate the row or break double-click detection. Deleting one entry is available in the main window; clearing every entry requires Settings and a confirmation.

17. From another application, hold Alt+Shift, press Up twice and Down once, and release. The preview follows each step without taking focus, and exactly one entry is pasted. Cancelling or copying externally during selection prevents the paste.

## Open decisions

- Final shortcut and mouse behavior after Windows runtime testing.
- Whether a separate entry-count limit is needed alongside the 64 MiB default size limit.
- Whether additional formats such as HTML/RTF or files need exact preservation. Text and images are required in the first release; other formats must be handled safely.
- Whether to persist the source window title. Application identity and a temporary window identifier are needed for current context; titles may contain sensitive information.
