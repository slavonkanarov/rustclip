use super::*;
use crate::layouts::Converter;
use unicode_segmentation::UnicodeSegmentation;

#[cfg(test)]
#[path = "selection_tests.rs"]
mod desktop_tests;

const MODIFIER_KEYS: [u16; 8] = [
    VK_LCONTROL,
    VK_RCONTROL,
    VK_LSHIFT,
    VK_RSHIFT,
    VK_LMENU,
    VK_RMENU,
    VK_LWIN,
    VK_RWIN,
];

fn keyboard(vk: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> Result<(), String> {
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    if sent != inputs.len() as u32 {
        return Err(
            "SendInput: layout conversion blocked (the application may have higher privileges)"
                .into(),
        );
    }
    Ok(())
}

fn modifier_flags(key: u16) -> u32 {
    if [VK_RCONTROL, VK_RMENU, VK_LWIN, VK_RWIN].contains(&key) {
        KEYEVENTF_EXTENDEDKEY
    } else {
        0
    }
}

fn shortcut(key: u16, modifier: u16, extended: bool, held: &[u16]) -> Vec<INPUT> {
    let mut inputs: Vec<_> = held
        .iter()
        .map(|&key| keyboard(key, modifier_flags(key) | KEYEVENTF_KEYUP))
        .collect();
    inputs.push(keyboard(modifier, 0));
    let flags = if extended { KEYEVENTF_EXTENDEDKEY } else { 0 };
    inputs.push(keyboard(key, flags));
    inputs.push(keyboard(key, flags | KEYEVENTF_KEYUP));
    inputs.push(keyboard(modifier, KEYEVENTF_KEYUP));
    inputs.extend(held.iter().map(|&key| keyboard(key, modifier_flags(key))));
    inputs
}

fn ensure_target(target: HWND) -> Result<(), String> {
    if target.is_null() || unsafe { GetForegroundWindow() } != target {
        return Err("The active window changed during layout conversion".into());
    }
    Ok(())
}

fn selection_steps(text: &str) -> usize {
    text.graphemes(true).count()
}

fn message(hwnd: HWND, id: u32, w: WPARAM, l: LPARAM) -> Result<usize, String> {
    let mut result = 0;
    if unsafe {
        SendMessageTimeoutW(
            hwnd,
            id,
            w,
            l,
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            300,
            &mut result,
        )
    } == 0
    {
        return Err("The text field did not respond during layout conversion".into());
    }
    Ok(result)
}

fn native_edit(target: HWND) -> Result<Option<HWND>, String> {
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    let thread_id = unsafe { GetWindowThreadProcessId(target, null_mut()) };
    if unsafe { GetGUIThreadInfo(thread_id, &mut info) } == 0 {
        return Err("Could not locate the focused text field".into());
    }
    let mut class = [0u16; 256];
    let len = unsafe { GetClassNameW(info.hwndFocus, class.as_mut_ptr(), class.len() as i32) };
    if len == 0 || !String::from_utf16_lossy(&class[..len as usize]).eq_ignore_ascii_case("Edit") {
        return Ok(None);
    }
    if unsafe { GetWindowLongW(info.hwndFocus, GWL_STYLE) } & ES_READONLY != 0 {
        return Err("The selected text field is read-only".into());
    }
    if unsafe { GetWindowLongW(info.hwndFocus, GWL_STYLE) } & ES_PASSWORD != 0 {
        return Err("Layout conversion is not supported in password fields".into());
    }
    Ok(Some(info.hwndFocus))
}

fn replace_native_edit(target: HWND, original: &str, replacement: &str) -> Result<bool, String> {
    let Some(edit) = native_edit(target)? else {
        return Ok(false);
    };
    let mut start = 0u32;
    let mut end = 0u32;
    message(
        edit,
        0x00b0,
        &mut start as *mut u32 as usize,
        &mut end as *mut u32 as isize,
    )?;
    if end.saturating_sub(start) as usize != original.encode_utf16().count() {
        return Err("The text selection changed during layout conversion".into());
    }
    // EM_REPLACESEL and EM_SETSEL use UTF-16 offsets, independent of caret shaping.
    message(edit, 0x00c2, 1, wide(replacement).as_ptr() as isize)?;
    message(
        edit,
        0x00b1,
        start as usize,
        (start as usize + replacement.encode_utf16().count()) as isize,
    )?;
    Ok(true)
}

fn read_native_edit(target: HWND, settings: &Settings) -> Result<Option<String>, String> {
    let Some(edit) = native_edit(target)? else {
        return Ok(None);
    };
    let mut start = 0u32;
    let mut end = 0u32;
    message(
        edit,
        0x00b0,
        &mut start as *mut u32 as usize,
        &mut end as *mut u32 as isize,
    )?;
    if start == end {
        return Err("No selected text was copied; select editable text first".into());
    }
    if (end.saturating_sub(start) as usize).saturating_mul(2)
        > settings.max_text_kb.saturating_mul(1024)
    {
        return Err("Selected text exceeds the configured text size limit".into());
    }
    let length = message(edit, WM_GETTEXTLENGTH, 0, 0)?;
    if length > 16 * 1024 * 1024 {
        return Err("Native text field exceeds the 32 MiB read limit".into());
    }
    let mut buffer = vec![0u16; length + 1];
    let copied = message(edit, WM_GETTEXT, buffer.len(), buffer.as_mut_ptr() as isize)?;
    if copied > length || end as usize > copied || start > end {
        return Err("The text selection changed during layout conversion".into());
    }
    String::from_utf16(&buffer[start as usize..end as usize])
        .map(Some)
        .map_err(|_| "Selection contains invalid Unicode text".into())
}

pub(super) struct Selection {
    converter: Option<(u32, u32, Converter)>,
    previous: Option<(usize, String, u32)>,
}

impl Selection {
    pub fn new() -> Self {
        Self {
            converter: None,
            previous: None,
        }
    }

    pub fn switch(&mut self, hwnd: HWND, target: HWND, settings: &Settings) -> Result<(), String> {
        if self
            .converter
            .as_ref()
            .is_none_or(|(a, b, _)| *a != settings.layout_first || *b != settings.layout_second)
        {
            self.converter = Some((
                settings.layout_first,
                settings.layout_second,
                Converter::new(settings.layout_first, settings.layout_second)?,
            ));
            self.previous = None;
        }
        ensure_target(target)?;
        let thread_id = unsafe { GetWindowThreadProcessId(target, null_mut()) };
        let active = unsafe { GetKeyboardLayout(thread_id) } as usize as u32;
        if let Some(text) = read_native_edit(target, settings)? {
            let (converted, destination) = self.convert(target, &text, active)?;
            ensure_target(target)?;
            if !replace_native_edit(target, &text, &converted)? {
                return Err("The text selection changed during layout conversion".into());
            }
            self.previous = Some((target as usize, converted, destination));
            return Ok(());
        }
        self.switch_clipboard(hwnd, target, settings, active)
    }

    fn switch_clipboard(
        &mut self,
        hwnd: HWND,
        target: HWND,
        settings: &Settings,
        active: u32,
    ) -> Result<(), String> {
        let old = if settings.restore {
            Some(self.snapshot(hwnd)?)
        } else {
            None
        };
        let before = unsafe { GetClipboardSequenceNumber() };
        let mut sequence = before;
        let held: Vec<_> = MODIFIER_KEYS
            .into_iter()
            .filter(|&key| unsafe { GetAsyncKeyState(key as i32) } < 0)
            .collect();
        let result = (|| {
            // Each batch restores held modifiers immediately, rather than waiting for key-up.
            send(&shortcut(VK_INSERT, VK_CONTROL, true, &held))?;
            let deadline = Instant::now() + Duration::from_millis(300);
            loop {
                // EmptyClipboard sends WM_DESTROYCLIPBOARD to the previous owner.
                // Serve sent messages without consuming queued hotkeys or capture events.
                dispatch_sent_messages();
                if unsafe { GetClipboardSequenceNumber() } != before {
                    break;
                }
                ensure_target(target)?;
                if Instant::now() >= deadline {
                    return Err("No selected text was copied; select editable text first".into());
                }
                thread::sleep(Duration::from_millis(1));
            }
            sequence = unsafe { GetClipboardSequenceNumber() };
            self.replace(hwnd, target, settings, active, &mut sequence)
        })();
        let restored = if unsafe { GetClipboardSequenceNumber() } == sequence {
            match old {
                Some(formats) => publish_formats(hwnd, &formats).map(|_| ()),
                None => Ok(()),
            }
        } else {
            Ok(())
        };
        match (result, restored) {
            (Err(error), Err(restore)) => Err(format!("{error}\nClipboard restore: {restore}")),
            (Err(error), _) => Err(error),
            (_, Err(error)) => Err(format!("Clipboard restore: {error}")),
            _ => Ok(()),
        }
    }

    fn convert(&self, target: HWND, text: &str, active: u32) -> Result<(String, u32), String> {
        let active = self
            .previous
            .as_ref()
            .filter(|(window, previous, _)| *window == target as usize && previous == text)
            .map_or(active, |(_, _, layout)| *layout);
        self.converter.as_ref().unwrap().2.convert(text, active)
    }

    fn snapshot(&self, hwnd: HWND) -> Result<Vec<(String, Vec<u8>)>, String> {
        let _guard = lock(hwnd)?;
        let mut formats = Vec::new();
        let mut total = 0usize;
        let mut id = 0;
        loop {
            id = unsafe { EnumClipboardFormats(id) };
            if id == 0 {
                break;
            }
            let name = format_name(id);
            if !matches!(id, 1 | 7 | 8 | 13 | 15 | 16 | 17) && id < 0xc000 {
                return Err("Clipboard contains formats that cannot be safely restored; copy plain text first or disable clipboard restoration".into());
            }
            if name == "RustClip/Owner" {
                continue;
            }
            let bytes = read_bytes(id).ok_or("Could not back up the clipboard")?;
            total = total.saturating_add(bytes.len());
            if total > 64 * 1024 * 1024 {
                return Err("Clipboard backup exceeds the 64 MiB limit".into());
            }
            formats.push((name, bytes));
        }
        Ok(formats)
    }

    fn replace(
        &mut self,
        hwnd: HWND,
        target: HWND,
        settings: &Settings,
        active: u32,
        sequence: &mut u32,
    ) -> Result<(), String> {
        let text = {
            let _guard = lock(hwnd)?;
            let bytes = read_bytes(13);
            // Delayed rendering may advance the sequence while GetClipboardData runs.
            *sequence = unsafe { GetClipboardSequenceNumber() };
            let bytes = bytes.ok_or("Selection has no Unicode text")?;
            if bytes.len() > settings.max_text_kb.saturating_mul(1024) {
                return Err("Selected text exceeds the configured text size limit".into());
            }
            let words: Vec<_> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .take_while(|&word| word != 0)
                .collect();
            String::from_utf16(&words).map_err(|_| "Selection contains invalid Unicode text")?
        };
        let (converted, destination) = self.convert(target, &text, active)?;
        ensure_target(target)?;
        let held: Vec<_> = MODIFIER_KEYS
            .into_iter()
            .filter(|&key| unsafe { GetAsyncKeyState(key as i32) } < 0)
            .collect();
        let mut inputs: Vec<_> = held
            .iter()
            .map(|&key| keyboard(key, modifier_flags(key) | KEYEVENTF_KEYUP))
            .collect();
        for word in converted.replace("\r\n", "\n").encode_utf16() {
            let word = if word == 10 { 13 } else { word };
            for flags in [KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP] {
                let mut input = keyboard(0, flags);
                input.Anonymous.ki.wScan = word;
                inputs.push(input);
            }
        }
        for _ in 0..selection_steps(&converted) {
            inputs.push(keyboard(VK_LEFT, KEYEVENTF_EXTENDEDKEY));
            inputs.push(keyboard(VK_LEFT, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP));
        }
        inputs.push(keyboard(VK_SHIFT, 0));
        for _ in 0..selection_steps(&converted) {
            inputs.push(keyboard(VK_RIGHT, KEYEVENTF_EXTENDEDKEY));
            inputs.push(keyboard(VK_RIGHT, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP));
        }
        inputs.push(keyboard(VK_SHIFT, KEYEVENTF_KEYUP));
        inputs.extend(held.iter().map(|&key| keyboard(key, modifier_flags(key))));
        send(&inputs)?;
        self.previous = Some((target as usize, converted, destination));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_counts_crlf_once() {
        assert_eq!(selection_steps("hello\r\nworld"), 11);
        assert_eq!(selection_steps("a🦀\nb"), 4);
        assert_eq!(selection_steps("he\u{301}llo"), 5);
        assert_eq!(selection_steps("a👨‍👩‍👧‍👦b"), 3);
    }

    #[test]
    fn copy_restores_held_modifiers_in_same_batch() {
        let inputs = shortcut(VK_INSERT, VK_CONTROL, true, &[VK_LCONTROL, VK_LSHIFT]);
        let keys: Vec<_> = inputs
            .iter()
            .map(|input| unsafe { (input.Anonymous.ki.wVk, input.Anonymous.ki.dwFlags) })
            .collect();
        assert_eq!(keys.first(), Some(&(VK_LCONTROL, KEYEVENTF_KEYUP)));
        assert_eq!(keys.last(), Some(&(VK_LSHIFT, 0)));
        assert_eq!(keys.len(), 8);
    }
}
