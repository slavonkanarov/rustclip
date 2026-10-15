use super::*;
use crate::history_selection::Direction;
use crate::settings::Action;
use crate::windows_paste;
use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, GetClipboardSequenceNumber, RemoveClipboardFormatListener,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

const MONITOR_APPLY: u32 = WM_APP + 10;
const MONITOR_OPEN_MAIN: u32 = WM_APP + 11;
thread_local! {
    static MONITOR: RefCell<Option<MonitorState>> = const { RefCell::new(None) };
}

pub(super) enum MonitorCommand {
    Apply(Settings, Sender<Result<(), String>>),
}

#[derive(Clone)]
pub(super) struct MonitorHandle {
    pub(super) window: usize,
    pub(super) sender: Sender<MonitorCommand>,
}

impl MonitorHandle {
    pub(super) fn apply(&self, settings: Settings) -> Result<(), String> {
        let (reply, response) = mpsc::channel();
        self.sender
            .send(MonitorCommand::Apply(settings, reply))
            .map_err(|_| "Shortcut monitor is unavailable.".to_string())?;
        let window = HWND(self.window as *mut c_void);
        unsafe { PostMessageW(Some(window), MONITOR_APPLY, WPARAM(0), LPARAM(0)) }
            .map_err(|error| error.to_string())?;
        response
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "Shortcut monitor did not respond.".to_string())?
    }

    pub(super) fn stop(&self) {
        let window = HWND(self.window as *mut c_void);
        let _ = unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
    }
}

struct MonitorState {
    worker: SyncSender<WorkerCommand>,
    own_sequence: Arc<AtomicU32>,
    ui: slint::Weak<MainWindow>,
    preview: slint::Weak<PreviewWindow>,
    ui_state: Arc<Mutex<UiState>>,
    ui_data: Arc<Mutex<UiData>>,
    commands: Receiver<MonitorCommand>,
    settings: Settings,
    registered: [bool; 6],
    foreground_hook: HWINEVENTHOOK,
}

pub(super) fn monitor_loop(
    worker: SyncSender<WorkerCommand>,
    own_sequence: Arc<AtomicU32>,
    ui: slint::Weak<MainWindow>,
    preview: slint::Weak<PreviewWindow>,
    ui_state: Arc<Mutex<UiState>>,
    ui_data: Arc<Mutex<UiData>>,
    commands: Receiver<MonitorCommand>,
    settings: Settings,
    ready: Sender<Result<(usize, Option<String>), String>>,
) {
    let result = unsafe {
        let module = match GetModuleHandleW(None) {
            Ok(module) => module,
            Err(error) => {
                let _ = ready.send(Err(error.to_string()));
                return;
            }
        };
        let class = WNDCLASSW {
            lpfnWndProc: Some(monitor_proc),
            hInstance: HINSTANCE(module.0),
            lpszClassName: w!("RustclipMessageWindow"),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            let _ = ready.send(Err("Could not register Windows message window.".into()));
            return;
        }
        let window = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("RustclipMessageWindow"),
            w!(""),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(HINSTANCE(module.0)),
            None,
        ) {
            Ok(window) => window,
            Err(error) => {
                let _ = ready.send(Err(error.to_string()));
                return;
            }
        };
        MONITOR.with(|cell| {
            *cell.borrow_mut() = Some(MonitorState {
                worker,
                own_sequence,
                ui,
                preview,
                ui_state,
                ui_data,
                commands,
                settings: settings.clone(),
                registered: [false; 6],
                foreground_hook: HWINEVENTHOOK::default(),
            });
        });
        if let Err(error) = AddClipboardFormatListener(window) {
            let _ = ready.send(Err(format!(
                "Could not listen to clipboard changes: {error}"
            )));
            let _ = DestroyWindow(window);
            return;
        }
        let hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(foreground_event),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        MONITOR.with(|cell| {
            if let Some(state) = cell.borrow_mut().as_mut() {
                state.foreground_hook = hook;
            }
        });
        let initial_target = GetForegroundWindow();
        if windows_paste::valid_target(initial_target) {
            MONITOR.with(|cell| {
                if let Some(state) = cell.borrow().as_ref() {
                    lock(&state.ui_state).last_external_window = initial_target.0 as usize;
                }
            });
        }
        let warning = MONITOR.with(|cell| {
            let mut state = cell.borrow_mut();
            let state = state.as_mut().unwrap();
            register_hotkeys(window, &settings, &mut state.registered).err()
        });
        let _ = ready.send(Ok((window.0 as usize, warning)));
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        Ok::<(), String>(())
    };
    let _ = result;
}

unsafe extern "system" fn monitor_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CLIPBOARDUPDATE => {
            let sequence = unsafe { GetClipboardSequenceNumber() };
            MONITOR.with(|cell| {
                if let Some(state) = cell.borrow().as_ref() {
                    if sequence != state.own_sequence.load(Ordering::Acquire) {
                        let _ = state
                            .worker
                            .try_send(WorkerCommand::ClipboardChanged(sequence));
                    }
                }
            });
            LRESULT(0)
        }
        WM_HOTKEY | MONITOR_OPEN_MAIN => {
            let action = if message == MONITOR_OPEN_MAIN {
                Action::OpenHistory
            } else {
                let Some(action) = Action::GLOBAL.get(wparam.0.wrapping_sub(1)).copied() else {
                    return LRESULT(0);
                };
                action
            };
            let target = unsafe { GetForegroundWindow() };
            let context = MONITOR.with(|cell| {
                let borrowed = cell.borrow();
                let monitor = borrowed.as_ref()?;
                let mut guard = lock(&monitor.ui_state);
                if windows_paste::valid_target(target) {
                    guard.original_window = target.0 as usize;
                    guard.last_external_window = target.0 as usize;
                } else {
                    guard.original_window = guard.last_external_window;
                }
                Some((
                    monitor.ui.clone(),
                    monitor.preview.clone(),
                    Arc::clone(&monitor.ui_state),
                    Arc::clone(&monitor.ui_data),
                ))
            });
            if let Some((weak, preview_weak, state, data)) = context {
                let _ = slint::invoke_from_event_loop(move || {
                    let (Some(ui), Some(preview)) = (weak.upgrade(), preview_weak.upgrade()) else {
                        return;
                    };
                    let mut guard = lock(&state);
                    if let Some(capture) = guard.capture.take() {
                        guard.draft.bindings[capture.index()] = guard.saved.binding(action);
                        drop(guard);
                        render_shortcuts(&ui, &state);
                        return;
                    }
                    drop(guard);
                    match action {
                        Action::BrowseOlder => {
                            preview::browse(&ui, &preview, &state, &data, Direction::Older)
                        }
                        Action::BrowseNewer => {
                            preview::browse(&ui, &preview, &state, &data, Direction::Newer)
                        }
                        Action::CancelQuick => preview::cancel(&ui, &preview, &state),
                        Action::OpenHistory => show_page(&ui, 0),
                        Action::OpenTemplates => show_page(&ui, 1),
                        Action::OpenSettings => show_page(&ui, 2),
                        _ => {}
                    }
                });
            }
            LRESULT(0)
        }
        MONITOR_APPLY => {
            MONITOR.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    while let Ok(MonitorCommand::Apply(settings, reply)) = state.commands.try_recv()
                    {
                        let result = change_hotkeys(window, state, settings);
                        let _ = reply.send(result);
                    }
                }
            });
            LRESULT(0)
        }
        WM_DESTROY => {
            MONITOR.with(|cell| {
                if let Some(mut state) = cell.borrow_mut().take() {
                    unsafe {
                        let _ = RemoveClipboardFormatListener(window);
                        unregister_hotkeys(window, &mut state.registered);
                        if !state.foreground_hook.0.is_null() {
                            let _ = UnhookWinEvent(state.foreground_hook);
                        }
                    }
                }
            });
            unsafe {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

unsafe extern "system" fn foreground_event(
    _hook: HWINEVENTHOOK,
    _event: u32,
    window: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    if windows_paste::valid_target(window) {
        MONITOR.with(|cell| {
            if let Some(state) = cell.borrow().as_ref() {
                lock(&state.ui_state).last_external_window = window.0 as usize;
            }
        });
    }
}

fn change_hotkeys(
    window: HWND,
    state: &mut MonitorState,
    settings: Settings,
) -> Result<(), String> {
    let old = state.settings.clone();
    unsafe {
        unregister_hotkeys(window, &mut state.registered);
    }
    if let Err(error) = unsafe { register_hotkeys(window, &settings, &mut state.registered) } {
        unsafe {
            unregister_hotkeys(window, &mut state.registered);
        }
        if let Err(rollback) = unsafe { register_hotkeys(window, &old, &mut state.registered) } {
            return Err(format!(
                "{error} Previous shortcuts could not be restored: {rollback}"
            ));
        }
        return Err(error);
    }
    state.settings = settings;
    Ok(())
}

unsafe fn register_hotkeys(
    window: HWND,
    settings: &Settings,
    registered: &mut [bool; 6],
) -> Result<(), String> {
    let mut errors = Vec::new();
    for (index, action) in Action::GLOBAL.into_iter().enumerate() {
        let binding = settings.binding(action);
        if binding.key == 0 {
            continue;
        }
        let modifiers = HOT_KEY_MODIFIERS(binding.modifiers as u32) | MOD_NOREPEAT;
        if let Err(error) =
            unsafe { RegisterHotKey(Some(window), index as i32 + 1, modifiers, binding.key) }
        {
            errors.push(format!(
                "{} ({}) could not be registered: {error}",
                action.name(),
                binding.label()
            ));
        } else {
            registered[index] = true;
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

unsafe fn unregister_hotkeys(window: HWND, registered: &mut [bool; 6]) {
    for (index, flag) in registered.iter_mut().enumerate() {
        if *flag {
            let _ = unsafe { UnregisterHotKey(Some(window), index as i32 + 1) };
            *flag = false;
        }
    }
}
