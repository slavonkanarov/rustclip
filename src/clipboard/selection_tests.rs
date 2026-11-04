use super::*;
use std::sync::mpsc;

struct Desktop {
    parent: HWND,
    owner: HWND,
    edit: HWND,
    foreground: HWND,
    clipboard: Vec<(String, Vec<u8>)>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Desktop {
    fn new() -> Self {
        let foreground = unsafe { GetForegroundWindow() };
        let clipboard = Selection::new()
            .snapshot(null_mut())
            .expect("Clipboard must contain restorable data for this desktop test");
        let (tx, rx) = mpsc::channel();
        let thread = thread::spawn(move || unsafe {
            let instance = GetModuleHandleW(null());
            let class = wide("RustClip.LayoutTest");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(DefWindowProcW),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                ..Default::default()
            };
            RegisterClassW(&wc);
            let parent = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class.as_ptr(),
                wide("RustClip layout test").as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                100,
                100,
                500,
                300,
                null_mut(),
                null_mut(),
                instance,
                null(),
            );
            let edit = CreateWindowExW(
                0,
                wide("EDIT").as_ptr(),
                wide("").as_ptr(),
                WS_CHILD | WS_VISIBLE | ES_MULTILINE as u32,
                0,
                0,
                450,
                230,
                parent,
                null_mut(),
                instance,
                null(),
            );
            SetForegroundWindow(parent);
            SetFocus(edit);
            tx.send((parent as usize, edit as usize)).unwrap();
            while IsWindow(parent) != 0 {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                thread::sleep(Duration::from_millis(1));
            }
        });
        let (parent, edit) = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let parent = parent as HWND;
        let edit = edit as HWND;
        assert!(!parent.is_null() && !edit.is_null());
        let owner = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                wide("RustClip.LayoutTest").as_ptr(),
                wide("RustClip clipboard owner").as_ptr(),
                WS_POPUP,
                -32000,
                -32000,
                1,
                1,
                null_mut(),
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            )
        };
        assert!(!owner.is_null());
        unsafe {
            let current = GetCurrentThreadId();
            let foreground_thread = GetWindowThreadProcessId(GetForegroundWindow(), null_mut());
            let attached = foreground_thread != current
                && AttachThreadInput(current, foreground_thread, 1) != 0;
            let focused = SetForegroundWindow(parent);
            if attached {
                AttachThreadInput(current, foreground_thread, 0);
            }
            assert!(
                focused != 0 || GetForegroundWindow() == parent,
                "parent={parent:?}, foreground={:?}, owner={owner:?}, visible={}, valid={}",
                GetForegroundWindow(),
                IsWindowVisible(parent),
                IsWindow(parent)
            );
        }
        Self {
            parent,
            owner,
            edit,
            foreground,
            clipboard,
            thread: Some(thread),
        }
    }

    fn text(&self) -> String {
        let mut buffer = [0u16; 4096];
        let count = unsafe { GetWindowTextW(self.edit, buffer.as_mut_ptr(), buffer.len() as i32) };
        String::from_utf16_lossy(&buffer[..count as usize])
    }

    fn selected(&self) -> (u32, u32) {
        let mut start = 0u32;
        let mut end = 0u32;
        unsafe {
            SendMessageW(
                self.edit,
                0x00b0,
                &mut start as *mut u32 as usize,
                &mut end as *mut u32 as isize,
            );
        }
        (start, end)
    }

    fn wait_text(&self, expected: &str) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while self.text() != expected && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(self.text(), expected);
    }

    fn caret_at_selection_end(&self) -> bool {
        let (_, end) = self.selected();
        let position = unsafe { SendMessageW(self.edit, 0x00d6, end as usize, 0) } as u32;
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        let thread_id = unsafe { GetWindowThreadProcessId(self.parent, null_mut()) };
        assert_ne!(unsafe { GetGUIThreadInfo(thread_id, &mut info) }, 0);
        info.rcCaret.left == (position as u16 as i16) as i32
            && info.rcCaret.top == ((position >> 16) as u16 as i16) as i32
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        unsafe {
            UnregisterHotKey(null_mut(), 0x5aaa);
        }
        let inputs: Vec<_> = MODIFIER_KEYS
            .into_iter()
            .chain([VK_F24])
            .map(|key| keyboard(key, modifier_flags(key) | KEYEVENTF_KEYUP))
            .collect();
        let _ = send(&inputs);
        if let Err(error) = publish_formats(self.owner, &self.clipboard) {
            eprintln!("Desktop test clipboard restore failed: {error}");
        }
        unsafe {
            SetForegroundWindow(self.foreground);
            PostMessageW(self.parent, WM_CLOSE, 0, 0);
            DestroyWindow(self.owner);
        }
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

#[test]
#[ignore = "requires an unlocked interactive Windows desktop and English/Russian layouts"]
fn held_hotkey_converts_and_reselects_twice() {
    let desktop = Desktop::new();
    let layouts = crate::layouts::installed().unwrap();
    let settings = Settings {
        layout_first: layouts
            .iter()
            .find(|layout| layout.id & 0xffff == 0x0409)
            .expect("English layout")
            .id,
        layout_second: layouts
            .iter()
            .find(|layout| layout.id & 0xffff == 0x0419)
            .expect("Russian layout")
            .id,
        ..Settings::default()
    };
    let mut selection = Selection::new();
    publish(desktop.owner, &text_clip("clipboard sentinel".into()), true).unwrap();
    unsafe {
        assert_ne!(
            RegisterHotKey(
                null_mut(),
                0x5aaa,
                MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT,
                VK_F24 as u32
            ),
            0
        );
    }
    for (original, converted, clipboard) in [
        ("руддщ", "hello", false),
        ("Руддщ 123!", "Hello 123!", false),
        ("руддщ\r\nцщкдв", "hello\r\nworld", false),
        ("руддщ 🦀", "hello 🦀", false),
        ("ру\u{301}ддщ", "he\u{301}llo", false),
        ("руддщ", "hello", true),
        ("Руддщ 123!", "Hello 123!", true),
        ("руддщ\r\nцщкдв", "hello\r\nworld", true),
    ] {
        unsafe {
            SendMessageW(
                desktop.edit,
                WM_SETTEXT,
                0,
                wide(&format!("before {original} after")).as_ptr() as isize,
            );
            SendMessageW(
                desktop.edit,
                0x00b1,
                7,
                (7 + original.encode_utf16().count()) as isize,
            );
        }
        for expected in [converted, original] {
            send(&[
                keyboard(VK_LCONTROL, 0),
                keyboard(VK_LSHIFT, 0),
                keyboard(VK_F24, 0),
            ])
            .unwrap();
            let start = Instant::now();
            let mut found = false;
            while start.elapsed() < Duration::from_millis(300) {
                let mut msg = MSG::default();
                if unsafe { PeekMessageW(&mut msg, null_mut(), WM_HOTKEY, WM_HOTKEY, PM_REMOVE) }
                    != 0
                {
                    found = true;
                    break;
                }
                thread::sleep(Duration::from_millis(1));
            }
            assert!(found, "Hotkey must fire before any keys are released");
            let result = if clipboard {
                let thread_id = unsafe { GetWindowThreadProcessId(desktop.parent, null_mut()) };
                let active = unsafe { GetKeyboardLayout(thread_id) } as usize as u32;
                selection.switch_clipboard(desktop.owner, desktop.parent, &settings, active)
            } else {
                selection.switch(desktop.owner, desktop.parent, &settings)
            };
            result.unwrap_or_else(|error| {
                panic!(
                    "{original:?} -> {expected:?}: {error}; selection={:?}",
                    desktop.selected()
                )
            });
            desktop.wait_text(&format!("before {expected} after"));
            let deadline = Instant::now() + Duration::from_millis(300);
            let expected_selection = (7, 7 + expected.encode_utf16().count() as u32);
            while desktop.selected() != expected_selection && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(desktop.selected(), expected_selection);
            assert!(
                desktop.caret_at_selection_end(),
                "Caret must be at the right edge of the selection"
            );
            assert!(
                start.elapsed() < Duration::from_millis(300),
                "Conversion must not wait for modifier release or paste delay"
            );
            unsafe {
                assert!(GetAsyncKeyState(VK_LCONTROL as i32) < 0);
                assert!(GetAsyncKeyState(VK_LSHIFT as i32) < 0);
            }
            let _guard = lock(desktop.owner).unwrap();
            let bytes = read_bytes(13).unwrap();
            assert_eq!(bytes, text_clip("clipboard sentinel".into()).formats[0].1);
            drop(_guard);
            send(&[keyboard(VK_F24, KEYEVENTF_KEYUP)]).unwrap();
            thread::sleep(Duration::from_millis(10));
        }
        send(&[
            keyboard(VK_LSHIFT, KEYEVENTF_KEYUP),
            keyboard(VK_LCONTROL, KEYEVENTF_KEYUP),
        ])
        .unwrap();
        thread::sleep(Duration::from_millis(10));
    }
    for (text, max_text_kb, expected_error) in [
        ("123!", 100, "Selection has no letter"),
        ("руддщ", 0, "Selected text exceeds"),
    ] {
        unsafe {
            SendMessageW(desktop.edit, WM_SETTEXT, 0, wide(text).as_ptr() as isize);
            SendMessageW(desktop.edit, 0x00b1, 0, -1);
        }
        let settings = Settings {
            max_text_kb,
            ..settings.clone()
        };
        let error = selection
            .switch(desktop.owner, desktop.parent, &settings)
            .unwrap_err();
        assert!(error.starts_with(expected_error), "{error}");
        assert_eq!(desktop.text(), text);
        let _guard = lock(desktop.owner).unwrap();
        assert_eq!(
            read_bytes(13).unwrap(),
            text_clip("clipboard sentinel".into()).formats[0].1
        );
    }
}
