# Technical notes

These are implementation guidelines for Windows. The UI must use a native desktop window and system tray without WebView or an embedded browser.

## Current implementation slice

The prototype uses Slint 1.18 for its desktop window, controls, and system tray, and the windows crate for clipboard notifications, global shortcuts, target-window tracking, registry startup, and Ctrl+V injection. No WebView is used. The main window has a compact menu, toolbar, tabs, dense list, and bottom search field. Settings and template editing use dark modal overlays; settings has a section tree and a two-column shortcut editor. A worker reads text and CF_DIB/CF_DIBV5 images from the clipboard, saves one binary .clip file per event under %LOCALAPPDATA%\rustclip\history, and loads older .txt history files. Each write uses a temporary file and rename. The configured accounting limit covers text, image bytes, and metadata; the default is 64 MiB. Text is capped at 8 MiB and a DIB image at 48 MiB. The UI lists the newest 1,000 entries, searches at most 32,000 characters per entry, and builds small cached thumbnails off the UI thread.

The clipboard owner and sequence-number checks suppress rustclip's own clipboard writes and allow a later external copy of identical content to be a new event. Clipboard changes superseded before the worker reads them cannot be reconstructed from Windows' current clipboard state. Templates are stored in %LOCALAPPDATA%\rustclip\templates.json with atomic replacement and support nested groups, including direct paste. Settings are stored in %LOCALAPPDATA%\rustclip\settings.conf; shortcut conflicts and registration errors are reported. Paste targets are captured before showing the UI and checked again before Ctrl+V. Optional clipboard restoration snapshots all formats backed by movable global memory up to 64 MiB and only restores while the selected clip remains current. The compact quick-selection preview, application identity, and additional image formats remain unimplemented.

## Data flow

```text
Clipboard change → read supported formats (source application: planned)
                 → size checks and data reads
                 → bounded queue → persistent storage
                                 → search index and thumbnails

Global shortcut → capture original window → browse history
                → select → write clipboard → paste
```

Keyboard handlers and the UI thread should perform only short operations. Large image reads, thumbnail generation, search, and disk writes run separately. Queues need finite capacity; overload must not cause unbounded memory growth or present a partial entry as complete.

## Self-initiated clipboard writes

Placing a selected entry on the clipboard records the resulting sequence number, and the worker also checks clipboard ownership so its own notification is ignored. The worker checks that a queued external sequence is still current before reading it. This approach still needs Windows testing around delayed notifications, rapid copies, and writes by another application.

Comparing text, image data, or hashes alone is insufficient: a user may copy the same data again from another application. Test delayed and duplicate notifications, rapid alternation of self-initiated and external writes, and another application changing the clipboard during selection.

## Formats and storage

- A history entry contains supported representations, a timestamp, and source application identity when available. The data model must not assume the clipboard holds only one format.
- Validate text and images for size and decodability. Handle read and decode errors locally; an unknown format can be skipped without discarding other representations from the event.
- Write history atomically or transactionally so a failure cannot leave a half-saved entry. Load large data and thumbnails on demand so opening the window is not delayed.
- The storage limit covers actual space used by entries, including metadata and images. Prune whole entries from oldest to newest. Skip a new entry that exceeds the limit by itself without pruning existing history.
- Clipboard data may be sensitive: keep it local and out of ordinary logs. The tray and main window provide a way to clear history.

## Windows integration and keyboard commands

Separate components handle the clipboard, global shortcut registration, active-window detection, display above other windows, and paste. This allows Windows behavior to be validated independently of history and UI logic.

Keep a message window alive while the application runs in the tray. Closing the main window should hide it; explicit Exit should remove the tray icon, unregister hotkeys and clipboard listeners, and terminate the message loop. Slint draws the desktop controls using its software renderer with Fluent dark styling; the tray and window are native desktop integrations.

Capture the original window before showing rustclip. Before automatic paste, confirm the window still exists, belongs to another process, and can be activated. If activation fails, leave the clipboard unchanged. If the target disappears after the selected clip is written, leave that clip on the clipboard and report the failure. Check all keyboard bindings for conflicts before saving settings; show global registration errors.

## Responsiveness checks

Preliminary engineering targets on a reference test machine: the compact preview appears within 100 ms of the shortcut in 95% of measurements, and the visible entry changes within 50 ms in 95% of measurements. Define the machine and test history size before measuring; these numbers are not guarantees for every computer. Measure clipboard read time, disk writes, and large image handling separately.

Functional checks must cover rapid copies, identical content copied externally, races between external copies and self-initiated writes, restart after an interrupted write, malformed data, and closure of the original window.
