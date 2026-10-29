use eframe::egui;
use std::{
    cell::RefCell,
    ptr::null_mut,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

struct Capture {
    window: HWND,
    pressed: [bool; 256],
    key: Option<u32>,
    value: String,
}

impl Capture {
    fn poll(&mut self, pressed: impl Fn(u16) -> bool) {
        // Ignore synthetic Unicode packets and reserved virtual-key codes.
        for key in (VK_LSHIFT..=VK_RMENU)
            .chain([VK_LWIN, VK_RWIN])
            .chain(VK_0..=VK_9)
            .chain(VK_A..=VK_Z)
            .chain(VK_F1..=VK_F24)
            .chain([
                VK_UP, VK_DOWN, VK_LEFT, VK_RIGHT, VK_SPACE, VK_BACK, VK_TAB, VK_ESCAPE, VK_RETURN,
                VK_INSERT, VK_DELETE, VK_HOME, VK_END, VK_PRIOR, VK_NEXT,
            ])
        {
            self.update(key as u32, pressed(key));
        }
    }

    fn update(&mut self, key: u32, down: bool) {
        let Some(pressed) = self.pressed.get_mut(key as usize) else {
            return;
        };
        if *pressed == down {
            return;
        }
        *pressed = down;
        let modifier = matches!(
            key as u16,
            VK_CONTROL
                | VK_LCONTROL
                | VK_RCONTROL
                | VK_SHIFT
                | VK_LSHIFT
                | VK_RSHIFT
                | VK_MENU
                | VK_LMENU
                | VK_RMENU
                | VK_LWIN
                | VK_RWIN
        );
        if down {
            self.key = if modifier {
                self.key.filter(|&key| self.pressed[key as usize])
            } else {
                Some(key)
            };
        } else if self.key.is_some() {
            return;
        }
        let mut parts = Vec::new();
        for (name, keys) in [
            ("Ctrl", &[VK_CONTROL, VK_LCONTROL, VK_RCONTROL][..]),
            ("Shift", &[VK_SHIFT, VK_LSHIFT, VK_RSHIFT][..]),
            ("Alt", &[VK_MENU, VK_LMENU, VK_RMENU][..]),
            ("Win", &[VK_LWIN, VK_RWIN][..]),
        ] {
            if keys.iter().any(|&key| self.pressed[key as usize]) {
                parts.push(name.to_owned());
            }
        }
        if let Some(key) = self.key {
            parts.push(key_name(key));
        }
        self.value = parts.join("+");
    }
}

fn key_name(key: u32) -> String {
    if (0x30..=0x39).contains(&key) || (0x41..=0x5a).contains(&key) {
        return char::from_u32(key).unwrap().to_string();
    }
    if (VK_F1 as u32..=VK_F24 as u32).contains(&key) {
        return format!("F{}", key - VK_F1 as u32 + 1);
    }
    match key as u16 {
        VK_UP => "Up",
        VK_DOWN => "Down",
        VK_LEFT => "Left",
        VK_RIGHT => "Right",
        VK_SPACE => "Space",
        VK_BACK => "Backspace",
        VK_TAB => "Tab",
        VK_ESCAPE => "Esc",
        VK_RETURN => "Enter",
        VK_INSERT => "Insert",
        VK_DELETE => "Delete",
        VK_HOME => "Home",
        VK_END => "End",
        VK_PRIOR => "PageUp",
        VK_NEXT => "PageDown",
        _ => return format!("Клавиша 0x{key:02X}"),
    }
    .into()
}

struct HookCapture {
    capture: Capture,
    updates: mpsc::Sender<String>,
    ctx: egui::Context,
    viewport: egui::ViewportId,
    receiving_events: bool,
}

thread_local! {
    static CAPTURE: RefCell<Option<HookCapture>> = const { RefCell::new(None) };
}

unsafe extern "system" fn keyboard_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // User32 calls may dispatch another hook callback on this thread.
        let foreground = unsafe { GetForegroundWindow() };
        let repaint = CAPTURE.with_borrow_mut(|state| {
            let state = state.as_mut()?;
            let capture = &mut state.capture;
            if foreground != capture.window {
                capture.pressed.fill(false);
                capture.key = None;
                return None;
            }
            let event = unsafe { &*(l as *const KBDLLHOOKSTRUCT) };
            capture.update(event.vkCode, event.flags & LLKHF_UP == 0);
            state.receiving_events = true;
            let _ = state.updates.send(capture.value.clone());
            Some((state.ctx.clone(), state.viewport))
        });
        if let Some((ctx, viewport)) = repaint {
            ctx.request_repaint_of(viewport);
            return 1;
        }
    }
    unsafe { CallNextHookEx(null_mut(), code, w, l) }
}

pub struct Recorder {
    thread: Option<thread::JoinHandle<()>>,
    thread_id: u32,
    updates: mpsc::Receiver<String>,
    value: String,
}

static RECORDING: AtomicBool = AtomicBool::new(false);

pub fn is_recording() -> bool {
    RECORDING.load(Ordering::Relaxed)
}

impl Recorder {
    pub fn new(ctx: &egui::Context) -> Result<Self, String> {
        let window = unsafe { GetForegroundWindow() } as usize;
        let viewport = ctx.viewport_id();
        let ctx = ctx.clone();
        let (ready, started) = mpsc::channel();
        let (updates, rx) = mpsc::channel();
        let thread = thread::spawn(move || unsafe {
            let mut msg = MSG::default();
            // Create the thread message queue before publishing its ID.
            PeekMessageW(&mut msg, null_mut(), 0, 0, PM_NOREMOVE);
            CAPTURE.set(Some(HookCapture {
                capture: Capture {
                    window: window as HWND,
                    pressed: [false; 256],
                    key: None,
                    value: String::new(),
                },
                updates,
                ctx,
                viewport,
                receiving_events: false,
            }));
            let hook = SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(keyboard_hook),
                GetModuleHandleW(null_mut()),
                0,
            );
            if hook.is_null() {
                let _ = ready.send(Err(format!(
                    "Не удалось начать запись: {}",
                    std::io::Error::last_os_error()
                )));
                CAPTURE.set(None);
                return;
            }
            let _ = ready.send(Ok(GetCurrentThreadId()));
            'recording: loop {
                while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                    if msg.message == WM_QUIT {
                        break 'recording;
                    }
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                let foreground = GetForegroundWindow();
                let pressed =
                    std::array::from_fn::<_, 256, _>(|key| GetAsyncKeyState(key as i32) < 0);
                let repaint = CAPTURE.with_borrow_mut(|state| {
                    let state = state.as_mut().unwrap();
                    if state.receiving_events || foreground != state.capture.window {
                        return None;
                    }
                    // Some Windows environments install the hook without delivering events.
                    // Read modifiers first so a simultaneous key press includes them.
                    let previous = state.capture.value.clone();
                    state.capture.poll(|key| pressed[key as usize]);
                    if state.capture.value != previous {
                        let _ = state.updates.send(state.capture.value.clone());
                        return Some((state.ctx.clone(), state.viewport));
                    }
                    None
                });
                if let Some((ctx, viewport)) = repaint {
                    ctx.request_repaint_of(viewport);
                }
                thread::sleep(Duration::from_millis(8));
            }
            UnhookWindowsHookEx(hook);
            CAPTURE.set(None);
        });
        let thread_id = started.recv().map_err(|error| error.to_string())??;
        RECORDING.store(true, Ordering::Relaxed);
        Ok(Self {
            thread: Some(thread),
            thread_id,
            updates: rx,
            value: String::new(),
        })
    }

    pub fn value(&mut self) -> &str {
        for value in self.updates.try_iter() {
            self.value = value;
        }
        &self.value
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        RECORDING.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    fn capture() -> Capture {
        Capture {
            window: null_mut(),
            pressed: [false; 256],
            key: None,
            value: String::new(),
        }
    }

    #[test]
    fn polling_records_modifiers_before_keys_and_ignores_unicode_packets() {
        let mut capture = capture();
        capture.poll(|key| {
            [VK_LCONTROL, VK_LSHIFT, VK_RMENU, VK_RWIN, VK_K, VK_PACKET].contains(&key)
        });
        assert_eq!(capture.value, "Ctrl+Shift+Alt+Win+K");
        capture.poll(|_| false);
        assert_eq!(capture.value, "Ctrl+Shift+Alt+Win+K");
        capture.poll(|key| key == VK_RSHIFT);
        assert_eq!(capture.value, "Shift");
    }

    #[test]
    fn hook_delivers_each_key_down_and_requests_repaint() {
        let ctx = egui::Context::default();
        let repaints = Arc::new(AtomicUsize::new(0));
        let requests = repaints.clone();
        ctx.set_request_repaint_callback(move |_| {
            // A repaint callback must be able to access capture state without a borrow panic.
            CAPTURE.with_borrow(|state| assert!(state.as_ref().unwrap().receiving_events));
            requests.fetch_add(1, Ordering::Relaxed);
        });
        let (updates, rx) = mpsc::channel();
        CAPTURE.set(Some(HookCapture {
            capture: Capture {
                window: unsafe { GetForegroundWindow() },
                ..capture()
            },
            updates,
            ctx,
            viewport: egui::ViewportId::ROOT,
            receiving_events: false,
        }));
        for (key, expected) in [
            (VK_RCONTROL, "Ctrl"),
            (VK_RSHIFT, "Ctrl+Shift"),
            (VK_RMENU, "Ctrl+Shift+Alt"),
            (VK_RWIN, "Ctrl+Shift+Alt+Win"),
            (VK_F12, "Ctrl+Shift+Alt+Win+F12"),
        ] {
            let event = KBDLLHOOKSTRUCT {
                vkCode: key as u32,
                ..Default::default()
            };
            assert_eq!(
                unsafe {
                    keyboard_hook(
                        HC_ACTION as i32,
                        WM_KEYDOWN as usize,
                        &event as *const _ as LPARAM,
                    )
                },
                1
            );
            assert_eq!(rx.try_recv().unwrap(), expected);
        }
        // egui coalesces requests until the next frame.
        assert!(repaints.load(Ordering::Relaxed) > 0);
        CAPTURE.set(None);
    }

    #[test]
    fn recorder_starts_and_stops_without_a_ui_message_loop() {
        let mut recorder = Recorder::new(&egui::Context::default()).unwrap();
        assert_ne!(recorder.thread_id, unsafe { GetCurrentThreadId() });
        assert_eq!(recorder.value(), "");
        drop(recorder);
    }

    #[test]
    fn records_modifiers_and_keeps_combination_after_release() {
        let mut capture = capture();
        for key in [VK_RCONTROL, VK_LSHIFT, VK_RMENU, VK_LWIN, VK_UP] {
            capture.update(key as u32, true);
        }
        assert_eq!(capture.value, "Ctrl+Shift+Alt+Win+Up");
        assert!(crate::hotkeys::parse(&capture.value).is_ok());
        for key in [VK_RCONTROL, VK_LSHIFT, VK_RMENU, VK_LWIN, VK_UP] {
            capture.update(key as u32, false);
        }
        assert_eq!(capture.value, "Ctrl+Shift+Alt+Win+Up");
        capture.update(VK_RWIN as u32, true);
        assert_eq!(capture.value, "Win");
        capture.update(VK_F24 as u32, true);
        assert_eq!(capture.value, "Win+F24");
    }

    #[test]
    fn modifier_only_and_unsupported_keys_cannot_be_saved() {
        let mut capture = capture();
        capture.update(VK_LSHIFT as u32, true);
        assert_eq!(capture.value, "Shift");
        assert!(crate::hotkeys::parse(&capture.value).is_err());
        capture.update(VK_LSHIFT as u32, false);
        assert!(capture.value.is_empty());
        capture.update(VK_LCONTROL as u32, true);
        capture.update(VK_OEM_1 as u32, true);
        assert!(crate::hotkeys::parse(&capture.value).is_err());
    }

    #[test]
    fn supported_key_names_roundtrip() {
        for key in (0x30..=0x39)
            .chain(0x41..=0x5a)
            .chain(VK_F1 as u32..=VK_F24 as u32)
            .chain(
                [
                    VK_UP, VK_DOWN, VK_LEFT, VK_RIGHT, VK_SPACE, VK_BACK, VK_TAB, VK_ESCAPE,
                    VK_RETURN, VK_INSERT, VK_DELETE, VK_HOME, VK_END, VK_PRIOR, VK_NEXT,
                ]
                .map(u32::from),
            )
        {
            let parsed = crate::hotkeys::parse(&format!("Ctrl+{}", key_name(key))).unwrap();
            assert_eq!(parsed.key, key);
        }
    }
}
