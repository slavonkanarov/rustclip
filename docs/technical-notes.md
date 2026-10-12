# Technical notes

These are implementation guidelines for Windows. The UI must use native Windows components without WebView or an embedded browser.

## Current implementation slice

The prototype uses the `windows` crate for Win32 clipboard notifications, a native listbox, a tray icon, and a global shortcut. A worker thread reads text and writes one UTF-8 file per entry under `%LOCALAPPDATA%\rustclip\history`; temporary files are renamed after syncing so interrupted writes are ignored on restart. The fixed initial accounting cap is 64 MiB (UTF-8 text bytes plus 64 bytes per entry), with an 8 MiB per-text-entry cap. This does not yet measure actual filesystem allocation. The listbox currently shows at most the newest 1,000 entries. These are provisional implementation values, not final settings defaults.

The current sequence-number check suppresses rustclip's own text restoration and treats a later external copy of identical text as a new event. Clipboard changes that are superseded before the worker reads them cannot be reconstructed from Windows' current clipboard state; rapid-copy behavior still needs Windows testing and refinement. The prototype has not yet implemented images, search, quick selection, configurable bindings, application identity, or paste into the original window.

## Data flow

```text
Clipboard change → snapshot of formats and source application
                 → size checks and data reads
                 → bounded queue → persistent storage
                                 → search index and thumbnails

Global shortcut → capture original window → browse history
                → select → write clipboard → paste
```

Keyboard handlers and the UI thread should perform only short operations. Large image reads, thumbnail generation, search, and disk writes run separately. Queues need finite capacity; overload must not cause unbounded memory growth or present a partial entry as complete.

## Self-initiated clipboard writes

Placing a selected entry on the clipboard needs an explicit operation that expects a clipboard-change notification. The implementation must suppress only its own notification while retaining external changes before, during, and after the operation. One possible mechanism is matching a notification to a clipboard sequence number and the write operation's state; the exact approach needs validation in a Windows prototype.

Comparing text, image data, or hashes alone is insufficient: a user may copy the same data again from another application. Test delayed and duplicate notifications, rapid alternation of self-initiated and external writes, and another application changing the clipboard during selection.

## Formats and storage

- A history entry contains supported representations, a timestamp, and source application identity when available. The data model must not assume the clipboard holds only one format.
- Validate text and images for size and decodability. Handle read and decode errors locally; an unknown format can be skipped without discarding other representations from the event.
- Write history atomically or transactionally so a failure cannot leave a half-saved entry. Load large data and thumbnails on demand so opening the window is not delayed.
- The storage limit covers actual space used by entries, including metadata and images. Prune whole entries from oldest to newest. Skip a new entry that exceeds the limit by itself without pruning existing history.
- Clipboard data may be sensitive: keep it local and out of ordinary logs. When settings are implemented, provide a way to clear history.

## Windows integration and keyboard commands

Separate components handle the clipboard, global shortcut registration, active-window detection, display above other windows, and paste. This allows Windows behavior to be validated independently of history and UI logic.

Keep a message window alive while the application runs in the tray. Closing the main window should hide it; explicit Exit should remove the tray icon, unregister hotkeys and clipboard listeners, and terminate the message loop. Native Win32 controls are suitable for an initial implementation, but the requirements do not mandate a particular Rust wrapper.

Capture the original window before showing the preview. Before automatic paste, confirm the window still exists and that the action cannot be directed into rustclip. If it is unsafe to restore focus or paste, leave the selected data on the clipboard. Check all keyboard bindings for internal conflicts before saving settings; show global shortcut registration errors to the user.

## Responsiveness checks

Preliminary engineering targets on a reference test machine: the compact preview appears within 100 ms of the shortcut in 95% of measurements, and the visible entry changes within 50 ms in 95% of measurements. Define the machine and test history size before measuring; these numbers are not guarantees for every computer. Measure clipboard read time, disk writes, and large image handling separately.

Functional checks must cover rapid copies, identical content copied externally, races between external copies and self-initiated writes, restart after an interrupted write, malformed data, and closure of the original window.
