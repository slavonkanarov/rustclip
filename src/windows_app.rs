use crate::image_preview;
use crate::settings::{ALT, Action, CTRL, SHIFT, Settings, WIN, key_name, valid_key};
use crate::store::{self, Store};
use crate::templates::{Group, Snippet, TemplateStore};
use crate::windows_autostart;
use crate::windows_clipboard;
use crate::windows_paste;
use slint::{
    CloseRequestResponse, ComponentHandle, Image as SlintImage, ModelRc, Rgba8Pixel,
    SharedPixelBuffer, VecModel,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, GetClipboardOwner, GetClipboardSequenceNumber,
    RemoveClipboardFormatListener,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

slint::include_modules!();

const MONITOR_APPLY: u32 = WM_APP + 10;
const HOTKEY_HISTORY: i32 = 1;
const HOTKEY_SETTINGS: i32 = 2;
const HOTKEY_TEMPLATES: i32 = 3;

#[derive(Clone)]
struct Summary {
    id: u128,
    preview: String,
    search: String,
    kind: &'static str,
    thumbnail: Option<SharedPixelBuffer<Rgba8Pixel>>,
}

#[derive(Default)]
struct UiData {
    entries: Vec<Summary>,
    groups: Vec<Group>,
    snippets: Vec<Snippet>,
    template_changed: bool,
    error: Option<String>,
    pending: bool,
}

struct UiState {
    saved: Settings,
    draft: Settings,
    capture: Option<Action>,
    visible_ids: Vec<u128>,
    selected: Option<u128>,
    original_window: usize,
    last_external_window: usize,
    saving: bool,
    visible_template_nodes: Vec<TemplateNode>,
    selected_template: TemplateNode,
    collapsed_groups: HashSet<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TemplateNode {
    Root,
    Group(u64),
    Snippet(u64),
}

enum WorkerCommand {
    ClipboardChanged(u32),
    Restore(u128),
    Paste(u128, usize),
    PasteText(String, usize),
    RestoreText(String),
    Delete(u128),
    Clear,
    TemplateAddGroup(Option<u64>),
    TemplateRenameGroup(u64, String),
    TemplateRemoveGroup(u64),
    TemplateAddSnippet(u64),
    TemplateSaveSnippet(u64, String, String),
    TemplateRemoveSnippet(u64),
    SetLimit(u64),
}

enum MonitorCommand {
    Apply(Settings, Sender<Result<(), String>>),
}

#[derive(Clone)]
struct MonitorHandle {
    window: usize,
    sender: Sender<MonitorCommand>,
}

impl MonitorHandle {
    fn apply(&self, settings: Settings) -> Result<(), String> {
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

    fn stop(&self) {
        let window = HWND(self.window as *mut c_void);
        let _ = unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
    }
}

struct MonitorState {
    worker: SyncSender<WorkerCommand>,
    own_sequence: Arc<AtomicU32>,
    ui: slint::Weak<MainWindow>,
    ui_state: Arc<Mutex<UiState>>,
    commands: Receiver<MonitorCommand>,
    settings: Settings,
    registered: [bool; 3],
    foreground_hook: HWINEVENTHOOK,
}

thread_local! {
    static MONITOR: RefCell<Option<MonitorState>> = const { RefCell::new(None) };
}

pub fn show_startup_error(message: &str) {
    let wide = wide(message);
    unsafe {
        MessageBoxW(None, PCWSTR(wide.as_ptr()), w!("rustclip"), MB_ICONERROR);
    }
}

fn show_startup_warning(message: &str) {
    let wide = wide(message);
    unsafe {
        MessageBoxW(None, PCWSTR(wide.as_ptr()), w!("rustclip"), MB_ICONWARNING);
    }
}

pub fn run() -> Result<(), String> {
    let settings_path = store::default_directory()
        .map_err(|error| error.to_string())?
        .parent()
        .ok_or("Could not locate local settings directory.")?
        .join("settings.conf");
    let (settings, load_warning) = match Settings::load(&settings_path) {
        Ok(settings) => (settings, None),
        Err(_) => (
            Settings::default(),
            Some("Settings could not be loaded; defaults are active.".to_string()),
        ),
    };
    let show_on_start = !settings.start_hidden;
    let worker_limit = u64::from(settings.history_limit_mb) * 1024 * 1024;
    let ui = MainWindow::new().map_err(|error| error.to_string())?;
    let tray = TrayIcon::new().map_err(|error| error.to_string())?;
    let ui_state = Arc::new(Mutex::new(UiState {
        saved: settings.clone(),
        draft: settings.clone(),
        capture: None,
        visible_ids: Vec::new(),
        selected: None,
        original_window: 0,
        last_external_window: 0,
        saving: false,
        visible_template_nodes: Vec::new(),
        selected_template: TemplateNode::Root,
        collapsed_groups: HashSet::new(),
    }));
    let data = Arc::new(Mutex::new(UiData::default()));
    render_shortcuts(&ui, &ui_state);
    render_general_settings(&ui, &ui_state);
    render_history(&ui, &ui_state, &data);
    render_templates(&ui, &ui_state, &data);

    let (worker_sender, worker_receiver) = mpsc::sync_channel(256);
    let own_sequence = Arc::new(AtomicU32::new(0));
    let (monitor_sender, monitor_receiver) = mpsc::channel();
    let (ready_sender, ready_receiver) = mpsc::channel();
    let monitor_thread = {
        let worker_sender = worker_sender.clone();
        let own_sequence = Arc::clone(&own_sequence);
        let ui_weak = ui.as_weak();
        let ui_state = Arc::clone(&ui_state);
        thread::spawn(move || {
            monitor_loop(
                worker_sender,
                own_sequence,
                ui_weak,
                ui_state,
                monitor_receiver,
                settings,
                ready_sender,
            )
        })
    };
    let (monitor_window, registration_warning) = ready_receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "Windows integration did not start.".to_string())??;
    let monitor = MonitorHandle {
        window: monitor_window,
        sender: monitor_sender,
    };
    let worker_thread = {
        let ui_weak = ui.as_weak();
        let ui_state = Arc::clone(&ui_state);
        let data = Arc::clone(&data);
        thread::spawn(move || {
            worker_loop(
                monitor_window,
                worker_receiver,
                own_sequence,
                ui_weak,
                ui_state,
                data,
                worker_limit,
            )
        })
    };
    wire_ui(
        &ui,
        &tray,
        &ui_state,
        &data,
        &worker_sender,
        &monitor,
        settings_path,
    );
    if let Some(warning) = registration_warning.as_ref().or(load_warning.as_ref()) {
        set_status(&ui, warning);
    }
    let weak_for_close = ui.as_weak();
    ui.window().on_close_requested(move || {
        if let Some(ui) = weak_for_close.upgrade() {
            if ui.get_settings_open() {
                ui.invoke_cancel_settings();
                return CloseRequestResponse::KeepWindowShown;
            }
            if ui.get_template_editor_open() {
                ui.invoke_cancel_template_edit();
                return CloseRequestResponse::KeepWindowShown;
            }
        }
        CloseRequestResponse::HideWindow
    });
    let result = match tray.show() {
        Ok(()) => {
            if show_on_start {
                capture_target(&ui_state);
                show_page(&ui, 0);
            }
            if let Some(warning) = registration_warning.as_deref() {
                show_startup_warning(warning);
            }
            slint::run_event_loop().map_err(|error| error.to_string())
        }
        Err(error) => Err(error.to_string()),
    };
    let _ = tray.hide();
    monitor.stop();
    let _ = monitor_thread.join();
    drop(ui);
    drop(tray);
    drop(monitor);
    drop(worker_sender);
    let _ = worker_thread.join();
    result
}

fn wire_ui(
    ui: &MainWindow,
    tray: &TrayIcon,
    state: &Arc<Mutex<UiState>>,
    data: &Arc<Mutex<UiData>>,
    worker: &SyncSender<WorkerCommand>,
    monitor: &MonitorHandle,
    settings_path: PathBuf,
) {
    let weak = ui.as_weak();
    ui.on_open_history(move || {
        if let Some(ui) = weak.upgrade() {
            show_page(&ui, 0);
        }
    });
    let weak = ui.as_weak();
    ui.on_open_settings(move || {
        if let Some(ui) = weak.upgrade() {
            show_page(&ui, 2);
        }
    });
    let weak = ui.as_weak();
    let state_for_cancel_settings = Arc::clone(state);
    ui.on_cancel_settings(move || {
        if let Some(ui) = weak.upgrade() {
            let mut guard = lock(&state_for_cancel_settings);
            if guard.saving {
                return;
            }
            guard.draft = guard.saved.clone();
            guard.capture = None;
            drop(guard);
            render_shortcuts(&ui, &state_for_cancel_settings);
            render_general_settings(&ui, &state_for_cancel_settings);
            ui.set_settings_open(false);
        }
    });
    let weak = ui.as_weak();
    ui.on_open_templates(move || {
        if let Some(ui) = weak.upgrade() {
            show_page(&ui, 1);
        }
    });
    let weak = ui.as_weak();
    let state_for_select = Arc::clone(state);
    let data_for_select = Arc::clone(data);
    ui.on_select_clip(move |index| {
        if let Some(ui) = weak.upgrade() {
            let mut state = lock(&state_for_select);
            state.selected = state.visible_ids.get(index as usize).copied();
            drop(state);
            render_history(&ui, &state_for_select, &data_for_select);
        }
    });
    let weak = ui.as_weak();
    let state_for_copy = Arc::clone(state);
    let worker_for_copy = worker.clone();
    ui.on_copy_selected(move || {
        if let Some(ui) = weak.upgrade() {
            if let Some(id) = lock(&state_for_copy).selected {
                if let Err(error) = send_worker(&worker_for_copy, WorkerCommand::Restore(id)) {
                    set_status(&ui, error);
                } else {
                    set_status(&ui, "Copying clip...");
                }
            }
        }
    });
    let weak = ui.as_weak();
    let state_for_paste = Arc::clone(state);
    let worker_for_paste = worker.clone();
    ui.on_paste_selected(move || {
        if let Some(ui) = weak.upgrade() {
            let guard = lock(&state_for_paste);
            let (Some(id), target) = (guard.selected, guard.original_window) else {
                return;
            };
            drop(guard);
            if target == 0 {
                set_status(&ui, "Open history from the target app before pasting.");
                return;
            }
            if let Err(error) = send_worker(&worker_for_paste, WorkerCommand::Paste(id, target)) {
                set_status(&ui, error);
            } else {
                let _ = ui.hide();
            }
        }
    });
    let weak = ui.as_weak();
    let state_for_delete = Arc::clone(state);
    let worker_for_delete = worker.clone();
    ui.on_delete_selected(move || {
        if let Some(ui) = weak.upgrade() {
            let Some(id) = lock(&state_for_delete).selected else {
                return;
            };
            let answer = unsafe {
                MessageBoxW(
                    None,
                    w!("Delete the selected clip?"),
                    w!("rustclip"),
                    MB_YESNO | MB_ICONQUESTION,
                )
            };
            if answer == IDYES {
                if let Err(error) = send_worker(&worker_for_delete, WorkerCommand::Delete(id)) {
                    set_status(&ui, error);
                } else {
                    set_status(&ui, "Deleting clip...");
                }
            }
        }
    });
    let weak = ui.as_weak();
    let state_for_nav = Arc::clone(state);
    let data_for_nav = Arc::clone(data);
    ui.on_navigate_clip(move |step| {
        if let Some(ui) = weak.upgrade() {
            navigate_history(&ui, &state_for_nav, &data_for_nav, step);
        }
    });
    let weak = ui.as_weak();
    let worker_for_clear = worker.clone();
    ui.on_clear_history(move || {
        if let Some(ui) = weak.upgrade() {
            confirm_clear(&ui, &worker_for_clear);
        }
    });
    let weak = ui.as_weak();
    let state_for_search = Arc::clone(state);
    let data_for_search = Arc::clone(data);
    ui.on_search_changed(move || {
        if let Some(ui) = weak.upgrade() {
            render_history(&ui, &state_for_search, &data_for_search);
        }
    });
    let weak = ui.as_weak();
    let state_for_toggle = Arc::clone(state);
    ui.on_toggle_modifier(move |index, flag, enabled| {
        if let Some(ui) = weak.upgrade() {
            if let Some(action) = Action::ALL.get(index as usize) {
                let mut state = lock(&state_for_toggle);
                let binding = &mut state.draft.bindings[action.index()];
                if enabled {
                    binding.modifiers |= flag as u8;
                } else {
                    binding.modifiers &= !(flag as u8);
                }
                drop(state);
                render_shortcuts(&ui, &state_for_toggle);
            }
        }
    });
    let weak = ui.as_weak();
    let state_for_capture = Arc::clone(state);
    ui.on_capture_key(move |index| {
        if let Some(ui) = weak.upgrade() {
            if let Some(action) = Action::ALL.get(index as usize) {
                lock(&state_for_capture).capture = Some(*action);
                render_shortcuts(&ui, &state_for_capture);
                set_status(&ui, "Press a key; Esc cancels.");
            }
        }
    });
    let weak = ui.as_weak();
    let state_for_save = Arc::clone(state);
    let monitor_for_save = monitor.clone();
    let worker_for_save = worker.clone();
    ui.on_save_settings(move || {
        let Some(ui) = weak.upgrade() else { return };
        let delay = match ui.get_paste_delay_text().parse::<u32>() {
            Ok(value) => value,
            Err(_) => {
                set_status(&ui, "Enter a valid paste delay.");
                return;
            }
        };
        let limit = match ui.get_history_limit_text().parse::<u32>() {
            Ok(value) => value,
            Err(_) => {
                set_status(&ui, "Enter a valid history limit.");
                return;
            }
        };
        let (draft, old) = {
            let mut state = lock(&state_for_save);
            if state.saving {
                return;
            }
            state.draft.start_hidden = ui.get_start_hidden();
            state.draft.start_with_windows = ui.get_start_with_windows();
            state.draft.paste_enabled = ui.get_paste_enabled();
            state.draft.restore_clipboard = ui.get_restore_clipboard();
            state.draft.paste_delay_ms = delay;
            state.draft.history_limit_mb = limit;
            state.saving = true;
            (state.draft.clone(), state.saved.clone())
        };
        ui.set_saving(true);
        if let Err(error) = draft.validate() {
            lock(&state_for_save).saving = false;
            ui.set_saving(false);
            set_status(&ui, &error);
            return;
        }
        set_status(&ui, "Saving settings...");
        let state = Arc::clone(&state_for_save);
        let weak = ui.as_weak();
        let monitor = monitor_for_save.clone();
        let worker = worker_for_save.clone();
        let path = settings_path.clone();
        thread::spawn(move || {
            let result = monitor.apply(draft.clone()).and_then(|()| {
                if let Err(error) = windows_autostart::set_enabled(draft.start_with_windows) {
                    let _ = monitor.apply(old.clone());
                    return Err(error);
                }
                if let Err(error) = draft.save(&path) {
                    let _ = windows_autostart::set_enabled(old.start_with_windows);
                    let rollback = monitor.apply(old.clone());
                    let detail = if rollback.is_err() {
                        " Previous shortcuts could not be restored."
                    } else {
                        ""
                    };
                    return Err(format!("Could not save settings: {error}.{detail}"));
                }
                let _ = worker.send(WorkerCommand::SetLimit(
                    u64::from(draft.history_limit_mb) * 1024 * 1024,
                ));
                Ok(())
            });
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    let mut guard = lock(&state);
                    guard.saving = false;
                    if result.is_ok() {
                        guard.saved = draft.clone();
                        guard.draft = draft;
                        guard.capture = None;
                        ui.set_settings_open(false);
                    }
                    drop(guard);
                    ui.set_saving(false);
                    render_shortcuts(&ui, &state);
                    render_general_settings(&ui, &state);
                    set_status(
                        &ui,
                        match &result {
                            Ok(()) => "Settings saved.",
                            Err(error) => error.as_str(),
                        },
                    );
                }
            });
        });
    });
    let weak = ui.as_weak();
    let state_for_reset = Arc::clone(state);
    ui.on_reset_settings(move || {
        if let Some(ui) = weak.upgrade() {
            let mut state = lock(&state_for_reset);
            state.draft = Settings::default();
            state.capture = None;
            drop(state);
            render_shortcuts(&ui, &state_for_reset);
            render_general_settings(&ui, &state_for_reset);
            set_status(&ui, "Defaults ready. Save to apply.");
        }
    });
    let weak = ui.as_weak();
    let state_for_templates = Arc::clone(state);
    let data_for_templates = Arc::clone(data);
    ui.on_select_template(move |index| {
        if let Some(ui) = weak.upgrade() {
            let node = {
                lock(&state_for_templates)
                    .visible_template_nodes
                    .get(index as usize)
                    .copied()
            };
            if let Some(node) = node {
                let selected = lock(&state_for_templates).selected_template;
                if node != selected
                    && !confirm_template_discard(&ui, &state_for_templates, &data_for_templates)
                {
                    return;
                }
                lock(&state_for_templates).selected_template = node;
                render_templates(&ui, &state_for_templates, &data_for_templates);
                render_template_editor(&ui, &state_for_templates, &data_for_templates);
            }
        }
    });
    let weak = ui.as_weak();
    let state_for_templates = Arc::clone(state);
    let data_for_templates = Arc::clone(data);
    ui.on_toggle_template_group(move |index| {
        if let Some(ui) = weak.upgrade() {
            let node = {
                lock(&state_for_templates)
                    .visible_template_nodes
                    .get(index as usize)
                    .copied()
            };
            if let Some(TemplateNode::Group(id)) = node {
                let selected = lock(&state_for_templates).selected_template;
                if TemplateNode::Group(id) != selected
                    && !confirm_template_discard(&ui, &state_for_templates, &data_for_templates)
                {
                    return;
                }
                let mut guard = lock(&state_for_templates);
                guard.selected_template = TemplateNode::Group(id);
                if !guard.collapsed_groups.insert(id) {
                    guard.collapsed_groups.remove(&id);
                }
                drop(guard);
                render_templates(&ui, &state_for_templates, &data_for_templates);
                render_template_editor(&ui, &state_for_templates, &data_for_templates);
            }
        }
    });
    let weak = ui.as_weak();
    let state_for_templates = Arc::clone(state);
    let data_for_templates = Arc::clone(data);
    let worker_for_templates = worker.clone();
    ui.on_new_template_group(move || {
        if let Some(ui) = weak.upgrade() {
            if !confirm_template_discard(&ui, &state_for_templates, &data_for_templates) {
                return;
            }
            let parent = selected_group(&state_for_templates, &data_for_templates);
            if let Err(error) = send_worker(
                &worker_for_templates,
                WorkerCommand::TemplateAddGroup(parent),
            ) {
                set_status(&ui, error);
            } else {
                set_status(&ui, "Creating group...");
            }
        }
    });
    let weak = ui.as_weak();
    let state_for_templates = Arc::clone(state);
    let data_for_templates = Arc::clone(data);
    let worker_for_templates = worker.clone();
    ui.on_new_template(move || {
        if let Some(ui) = weak.upgrade() {
            if !confirm_template_discard(&ui, &state_for_templates, &data_for_templates) {
                return;
            }
            let Some(group) = selected_group(&state_for_templates, &data_for_templates) else {
                set_status(&ui, "Select a group first.");
                return;
            };
            if let Err(error) = send_worker(
                &worker_for_templates,
                WorkerCommand::TemplateAddSnippet(group),
            ) {
                set_status(&ui, error);
            } else {
                set_status(&ui, "Creating template...");
            }
        }
    });
    let weak = ui.as_weak();
    let state_for_templates = Arc::clone(state);
    let worker_for_templates = worker.clone();
    ui.on_save_template(move || {
        if let Some(ui) = weak.upgrade() {
            let node = lock(&state_for_templates).selected_template;
            let name = ui.get_template_name().to_string();
            let command = match node {
                TemplateNode::Group(id) => WorkerCommand::TemplateRenameGroup(id, name),
                TemplateNode::Snippet(id) => WorkerCommand::TemplateSaveSnippet(
                    id,
                    name,
                    ui.get_template_content().to_string(),
                ),
                TemplateNode::Root => return,
            };
            if let Err(error) = send_worker(&worker_for_templates, command) {
                set_status(&ui, error);
            } else {
                ui.set_template_editor_open(false);
                set_status(&ui, "Saving template...");
            }
        }
    });
    let weak = ui.as_weak();
    let state_for_cancel_template = Arc::clone(state);
    let data_for_cancel_template = Arc::clone(data);
    ui.on_cancel_template_edit(move || {
        if let Some(ui) = weak.upgrade() {
            render_template_editor(&ui, &state_for_cancel_template, &data_for_cancel_template);
            ui.set_template_editor_open(false);
        }
    });
    let weak = ui.as_weak();
    let state_for_templates = Arc::clone(state);
    let worker_for_templates = worker.clone();
    ui.on_delete_template(move || {
        if let Some(ui) = weak.upgrade() {
            let node = lock(&state_for_templates).selected_template;
            let command = match node {
                TemplateNode::Group(id) => WorkerCommand::TemplateRemoveGroup(id),
                TemplateNode::Snippet(id) => WorkerCommand::TemplateRemoveSnippet(id),
                TemplateNode::Root => return,
            };
            let answer = unsafe {
                MessageBoxW(
                    None,
                    w!("Delete this group or template? Child items will also be deleted."),
                    w!("rustclip"),
                    MB_YESNO | MB_ICONQUESTION,
                )
            };
            if answer == IDYES {
                if let Err(error) = send_worker(&worker_for_templates, command) {
                    set_status(&ui, error);
                } else {
                    set_status(&ui, "Deleting template...");
                }
            }
        }
    });
    let weak = ui.as_weak();
    let worker_for_templates = worker.clone();
    ui.on_copy_template(move || {
        if let Some(ui) = weak.upgrade() {
            let content = ui.get_template_content().to_string();
            if content.is_empty() {
                return;
            }
            if let Err(error) =
                send_worker(&worker_for_templates, WorkerCommand::RestoreText(content))
            {
                set_status(&ui, error);
            } else {
                set_status(&ui, "Copying template...");
            }
        }
    });
    let weak = ui.as_weak();
    let state_for_template_paste = Arc::clone(state);
    let worker_for_template_paste = worker.clone();
    ui.on_paste_template(move || {
        if let Some(ui) = weak.upgrade() {
            let content = ui.get_template_content().to_string();
            if content.is_empty() {
                return;
            }
            let target = lock(&state_for_template_paste).original_window;
            if target == 0 {
                set_status(&ui, "Open templates from the target app before pasting.");
                return;
            }
            if let Err(error) = send_worker(
                &worker_for_template_paste,
                WorkerCommand::PasteText(content, target),
            ) {
                set_status(&ui, error);
            } else {
                let _ = ui.hide();
            }
        }
    });
    let weak = ui.as_weak();
    let state_for_templates = Arc::clone(state);
    let data_for_templates = Arc::clone(data);
    ui.on_template_search_changed(move || {
        if let Some(ui) = weak.upgrade() {
            render_templates(&ui, &state_for_templates, &data_for_templates);
        }
    });
    let weak = ui.as_weak();
    let state_for_key = Arc::clone(state);
    let data_for_key = Arc::clone(data);
    let worker_for_key = worker.clone();
    ui.on_handle_key(move |text, ctrl, alt, shift, win| {
        let Some(ui) = weak.upgrade() else {
            return false;
        };
        handle_key(
            &ui,
            &state_for_key,
            &data_for_key,
            &worker_for_key,
            &text,
            ctrl,
            alt,
            shift,
            win,
        )
    });
    ui.on_exit_app(|| {
        let _ = slint::quit_event_loop();
    });

    let weak = ui.as_weak();
    let state_for_tray = Arc::clone(state);
    tray.on_open_history(move || {
        if let Some(ui) = weak.upgrade() {
            capture_target(&state_for_tray);
            show_page(&ui, 0);
        }
    });
    let weak = ui.as_weak();
    let state_for_tray = Arc::clone(state);
    tray.on_open_settings(move || {
        if let Some(ui) = weak.upgrade() {
            capture_target(&state_for_tray);
            show_page(&ui, 2);
        }
    });
    let weak = ui.as_weak();
    let state_for_tray = Arc::clone(state);
    tray.on_open_templates(move || {
        if let Some(ui) = weak.upgrade() {
            capture_target(&state_for_tray);
            show_page(&ui, 1);
        }
    });
    let weak = ui.as_weak();
    let worker_for_tray = worker.clone();
    tray.on_clear_history(move || {
        if let Some(ui) = weak.upgrade() {
            confirm_clear(&ui, &worker_for_tray);
        }
    });
    tray.on_exit_app(|| {
        let _ = slint::quit_event_loop();
    });
}

fn show_page(ui: &MainWindow, page: i32) {
    if page == 2 {
        ui.set_settings_open(true);
        ui.set_template_editor_open(false);
    } else {
        ui.set_page(page);
        ui.set_settings_open(false);
        ui.set_template_editor_open(false);
    }
    ui.set_menu_open(0);
    if let Err(error) = ui.show() {
        set_status(ui, &format!("Could not show window: {error}"));
    }
}

fn capture_target(state: &Arc<Mutex<UiState>>) {
    let target = unsafe { GetForegroundWindow() };
    let mut guard = lock(state);
    if windows_paste::valid_target(target) {
        guard.original_window = target.0 as usize;
        guard.last_external_window = target.0 as usize;
    } else {
        guard.original_window = guard.last_external_window;
    }
}

fn confirm_clear(ui: &MainWindow, worker: &SyncSender<WorkerCommand>) {
    let answer = unsafe {
        MessageBoxW(
            None,
            w!("Delete all saved history?"),
            w!("rustclip"),
            MB_YESNO | MB_ICONQUESTION,
        )
    };
    if answer == IDYES {
        if let Err(error) = send_worker(worker, WorkerCommand::Clear) {
            set_status(ui, error);
        } else {
            set_status(ui, "Clearing history...");
        }
    }
}

fn send_worker(
    worker: &SyncSender<WorkerCommand>,
    command: WorkerCommand,
) -> Result<(), &'static str> {
    worker.try_send(command).map_err(|error| match error {
        mpsc::TrySendError::Full(_) => "rustclip is busy. Please try again.",
        mpsc::TrySendError::Disconnected(_) => "History worker is unavailable.",
    })
}

fn set_status(ui: &MainWindow, status: &str) {
    ui.set_status_text(status.into());
}

fn render_shortcuts(ui: &MainWindow, state: &Arc<Mutex<UiState>>) {
    let state = lock(state);
    let rows: Vec<ShortcutRow> = Action::ALL
        .into_iter()
        .map(|action| {
            let binding = state.draft.binding(action);
            ShortcutRow {
                name: action.name().into(),
                scope: if action.global() {
                    "GLOBAL"
                } else {
                    "IN WINDOW"
                }
                .into(),
                key: key_name(binding.key).into(),
                combo: binding.label().into(),
                win: binding.modifiers & WIN != 0,
                ctrl: binding.modifiers & CTRL != 0,
                alt: binding.modifiers & ALT != 0,
                shift: binding.modifiers & SHIFT != 0,
                capturing: state.capture == Some(action),
            }
        })
        .collect();
    drop(state);
    ui.set_shortcuts(ModelRc::new(VecModel::from(rows)));
}

fn render_general_settings(ui: &MainWindow, state: &Arc<Mutex<UiState>>) {
    let draft = lock(state).draft.clone();
    ui.set_start_hidden(draft.start_hidden);
    ui.set_start_with_windows(draft.start_with_windows);
    ui.set_paste_enabled(draft.paste_enabled);
    ui.set_restore_clipboard(draft.restore_clipboard);
    ui.set_paste_delay_text(draft.paste_delay_ms.to_string().into());
    ui.set_history_limit_text(draft.history_limit_mb.to_string().into());
}

fn render_history(ui: &MainWindow, state: &Arc<Mutex<UiState>>, data: &Arc<Mutex<UiData>>) {
    let query = ui.get_search_text().to_lowercase();
    let data_guard = lock(data);
    let mut state = lock(state);
    let visible: Vec<&Summary> = data_guard
        .entries
        .iter()
        .filter(|entry| entry.search.contains(&query))
        .collect();
    state.visible_ids = visible.iter().map(|entry| entry.id).collect();
    if !state.visible_ids.contains(&state.selected.unwrap_or(0)) {
        state.selected = state.visible_ids.first().copied();
    }
    let selected = state.selected;
    drop(state);
    let rows: Vec<ClipRow> = visible
        .into_iter()
        .map(|entry| ClipRow {
            preview: entry.preview.clone().into(),
            kind: entry.kind.into(),
            thumbnail: entry
                .thumbnail
                .clone()
                .map(SlintImage::from_rgba8)
                .unwrap_or_default(),
            selected: selected == Some(entry.id),
        })
        .collect();
    drop(data_guard);
    ui.set_has_selection(selected.is_some());
    ui.set_clips(ModelRc::new(VecModel::from(rows)));
}

fn navigate_history(
    ui: &MainWindow,
    state: &Arc<Mutex<UiState>>,
    data: &Arc<Mutex<UiData>>,
    step: i32,
) {
    let mut guard = lock(state);
    if guard.visible_ids.is_empty() {
        return;
    }
    let current = guard
        .visible_ids
        .iter()
        .position(|id| Some(*id) == guard.selected)
        .unwrap_or(0);
    let next = if step > 0 {
        (current + 1).min(guard.visible_ids.len() - 1)
    } else {
        current.saturating_sub(1)
    };
    guard.selected = guard.visible_ids.get(next).copied();
    drop(guard);
    render_history(ui, state, data);
}

fn selected_group(state: &Arc<Mutex<UiState>>, data: &Arc<Mutex<UiData>>) -> Option<u64> {
    match lock(state).selected_template {
        TemplateNode::Root => None,
        TemplateNode::Group(id) => Some(id),
        TemplateNode::Snippet(id) => lock(data)
            .snippets
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.group),
    }
}

fn render_templates(ui: &MainWindow, state: &Arc<Mutex<UiState>>, data: &Arc<Mutex<UiData>>) {
    let query = ui.get_template_search().to_lowercase();
    let data_guard = lock(data);
    let groups = &data_guard.groups;
    let snippets = &data_guard.snippets;
    let mut guard = lock(state);
    let selected = guard.selected_template;
    let mut rows = vec![TemplateRow {
        name: "All templates".into(),
        detail: "ROOT".into(),
        depth: 0,
        folder: true,
        collapsed: false,
        selected: selected == TemplateNode::Root,
    }];
    let mut nodes = vec![TemplateNode::Root];
    if query.is_empty() {
        append_template_groups(
            groups,
            snippets,
            &guard.collapsed_groups,
            selected,
            None,
            1,
            &mut rows,
            &mut nodes,
        );
    } else {
        for group in groups {
            if group.name.to_lowercase().contains(&query) {
                rows.push(TemplateRow {
                    name: group.name.clone().into(),
                    detail: "GROUP".into(),
                    depth: 1,
                    folder: true,
                    collapsed: false,
                    selected: selected == TemplateNode::Group(group.id),
                });
                nodes.push(TemplateNode::Group(group.id));
            }
        }
        for snippet in snippets {
            if snippet.name.to_lowercase().contains(&query)
                || snippet
                    .content
                    .chars()
                    .take(512)
                    .collect::<String>()
                    .to_lowercase()
                    .contains(&query)
            {
                rows.push(TemplateRow {
                    name: snippet.name.clone().into(),
                    detail: "TEXT TEMPLATE".into(),
                    depth: 1,
                    folder: false,
                    collapsed: false,
                    selected: selected == TemplateNode::Snippet(snippet.id),
                });
                nodes.push(TemplateNode::Snippet(snippet.id));
            }
        }
    }
    guard.visible_template_nodes = nodes;
    drop(guard);
    drop(data_guard);
    ui.set_template_rows(ModelRc::new(VecModel::from(rows)));
}

fn append_template_groups(
    groups: &[Group],
    snippets: &[Snippet],
    collapsed: &HashSet<u64>,
    selected: TemplateNode,
    parent: Option<u64>,
    depth: i32,
    rows: &mut Vec<TemplateRow>,
    nodes: &mut Vec<TemplateNode>,
) {
    for group in groups.iter().filter(|group| group.parent == parent) {
        rows.push(TemplateRow {
            name: group.name.clone().into(),
            detail: "GROUP".into(),
            depth,
            folder: true,
            collapsed: collapsed.contains(&group.id),
            selected: selected == TemplateNode::Group(group.id),
        });
        nodes.push(TemplateNode::Group(group.id));
        if collapsed.contains(&group.id) {
            continue;
        }
        append_template_groups(
            groups,
            snippets,
            collapsed,
            selected,
            Some(group.id),
            depth + 1,
            rows,
            nodes,
        );
        for snippet in snippets.iter().filter(|snippet| snippet.group == group.id) {
            rows.push(TemplateRow {
                name: snippet.name.clone().into(),
                detail: snippet
                    .content
                    .lines()
                    .next()
                    .unwrap_or("Empty template")
                    .chars()
                    .take(80)
                    .collect::<String>()
                    .into(),
                depth: depth + 1,
                folder: false,
                collapsed: false,
                selected: selected == TemplateNode::Snippet(snippet.id),
            });
            nodes.push(TemplateNode::Snippet(snippet.id));
        }
    }
}

fn render_template_editor(ui: &MainWindow, state: &Arc<Mutex<UiState>>, data: &Arc<Mutex<UiData>>) {
    let selected = lock(state).selected_template;
    let data = lock(data);
    match selected {
        TemplateNode::Root => {
            ui.set_template_kind(0);
            ui.set_template_name("".into());
            ui.set_template_content("".into());
        }
        TemplateNode::Group(id) => {
            if let Some(group) = data.groups.iter().find(|group| group.id == id) {
                ui.set_template_kind(1);
                ui.set_template_name(group.name.clone().into());
                ui.set_template_content("".into());
            }
        }
        TemplateNode::Snippet(id) => {
            if let Some(snippet) = data.snippets.iter().find(|snippet| snippet.id == id) {
                ui.set_template_kind(2);
                ui.set_template_name(snippet.name.clone().into());
                ui.set_template_content(snippet.content.clone().into());
            }
        }
    }
}

fn confirm_template_discard(
    ui: &MainWindow,
    state: &Arc<Mutex<UiState>>,
    data: &Arc<Mutex<UiData>>,
) -> bool {
    let selected = lock(state).selected_template;
    let data = lock(data);
    let dirty = match selected {
        TemplateNode::Root => false,
        TemplateNode::Group(id) => data
            .groups
            .iter()
            .find(|group| group.id == id)
            .is_some_and(|group| ui.get_template_name().to_string() != group.name),
        TemplateNode::Snippet(id) => data
            .snippets
            .iter()
            .find(|snippet| snippet.id == id)
            .is_some_and(|snippet| {
                ui.get_template_name().to_string() != snippet.name
                    || ui.get_template_content().to_string() != snippet.content
            }),
    };
    drop(data);
    if !dirty {
        return true;
    }
    unsafe {
        MessageBoxW(
            None,
            w!("Discard unsaved template changes?"),
            w!("rustclip"),
            MB_YESNO | MB_ICONQUESTION,
        ) == IDYES
    }
}

fn handle_key(
    ui: &MainWindow,
    state: &Arc<Mutex<UiState>>,
    data: &Arc<Mutex<UiData>>,
    worker: &SyncSender<WorkerCommand>,
    text: &str,
    ctrl: bool,
    alt: bool,
    shift: bool,
    win: bool,
) -> bool {
    let key = key_from_slint(text);
    let capture = lock(state).capture;
    if let Some(action) = capture {
        if key == Some(0x1b) {
            lock(state).capture = None;
            render_shortcuts(ui, state);
            set_status(ui, "Key selection cancelled.");
        } else if let Some(key) = key.filter(|key| valid_key(*key)) {
            let mut guard = lock(state);
            guard.draft.bindings[action.index()].key = key;
            guard.capture = None;
            drop(guard);
            render_shortcuts(ui, state);
            set_status(ui, "Key changed. Save to apply.");
        }
        return true;
    }
    if ui.get_settings_open() {
        if key == Some(0x1b) {
            ui.invoke_cancel_settings();
            return true;
        }
        return false;
    }
    if ui.get_template_editor_open() {
        if key == Some(0x1b) {
            ui.invoke_cancel_template_edit();
            return true;
        }
        return false;
    }
    if ui.get_menu_open() != 0 && key == Some(0x1b) {
        ui.set_menu_open(0);
        return true;
    }
    let Some(key) = key else { return false };
    let modifiers = (if alt { ALT } else { 0 })
        | (if ctrl { CTRL } else { 0 })
        | (if shift { SHIFT } else { 0 })
        | (if win { WIN } else { 0 });
    let saved = lock(state).saved.clone();
    if ui.get_page() != 0 {
        return false;
    }
    for action in [
        Action::NextEntry,
        Action::PreviousEntry,
        Action::CopySelected,
        Action::PasteSelected,
        Action::DeleteSelected,
        Action::FocusSearch,
    ] {
        let binding = saved.binding(action);
        if binding.key != key || binding.modifiers != modifiers {
            continue;
        }
        match action {
            Action::NextEntry | Action::PreviousEntry => {
                navigate_history(
                    ui,
                    state,
                    data,
                    if action == Action::NextEntry { 1 } else { -1 },
                );
            }
            Action::CopySelected => {
                if let Some(id) = lock(state).selected {
                    if let Err(error) = send_worker(worker, WorkerCommand::Restore(id)) {
                        set_status(ui, error);
                    } else {
                        set_status(ui, "Copying clip...");
                    }
                }
            }
            Action::PasteSelected => ui.invoke_paste_selected(),
            Action::DeleteSelected => ui.invoke_delete_selected(),
            Action::FocusSearch => {
                ui.invoke_focus_search();
            }
            _ => {}
        }
        return true;
    }
    false
}

fn key_from_slint(text: &str) -> Option<u32> {
    use slint::platform::Key;
    let mut chars = text.chars();
    let value = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    if value.is_ascii_alphabetic() {
        return Some(value.to_ascii_uppercase() as u32);
    }
    if value.is_ascii_digit() {
        return Some(value as u32);
    }
    for (named, vk) in [
        (Key::Backspace, 0x08),
        (Key::Tab, 0x09),
        (Key::Return, 0x0d),
        (Key::Escape, 0x1b),
        (Key::Space, 0x20),
        (Key::PageUp, 0x21),
        (Key::PageDown, 0x22),
        (Key::End, 0x23),
        (Key::Home, 0x24),
        (Key::LeftArrow, 0x25),
        (Key::UpArrow, 0x26),
        (Key::RightArrow, 0x27),
        (Key::DownArrow, 0x28),
        (Key::Insert, 0x2d),
        (Key::Delete, 0x2e),
        (Key::F1, 0x70),
        (Key::F2, 0x71),
        (Key::F3, 0x72),
        (Key::F4, 0x73),
        (Key::F5, 0x74),
        (Key::F6, 0x75),
        (Key::F7, 0x76),
        (Key::F8, 0x77),
        (Key::F9, 0x78),
        (Key::F10, 0x79),
        (Key::F11, 0x7a),
        (Key::F12, 0x7b),
        (Key::F13, 0x7c),
        (Key::F14, 0x7d),
        (Key::F15, 0x7e),
        (Key::F16, 0x7f),
        (Key::F17, 0x80),
        (Key::F18, 0x81),
        (Key::F19, 0x82),
        (Key::F20, 0x83),
        (Key::F21, 0x84),
        (Key::F22, 0x85),
        (Key::F23, 0x86),
        (Key::F24, 0x87),
    ] {
        if value == char::from(named) {
            return Some(vk);
        }
    }
    None
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn monitor_loop(
    worker: SyncSender<WorkerCommand>,
    own_sequence: Arc<AtomicU32>,
    ui: slint::Weak<MainWindow>,
    ui_state: Arc<Mutex<UiState>>,
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
                ui_state,
                commands,
                settings: settings.clone(),
                registered: [false; 3],
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
        WM_HOTKEY => {
            let page = match wparam.0 as i32 {
                HOTKEY_HISTORY => 0,
                HOTKEY_SETTINGS => 2,
                HOTKEY_TEMPLATES => 1,
                _ => return LRESULT(0),
            };
            let target = unsafe { GetForegroundWindow() };
            let weak = MONITOR.with(|cell| {
                let state_ref = cell.borrow();
                let state = state_ref.as_ref()?;
                let mut guard = lock(&state.ui_state);
                if windows_paste::valid_target(target) {
                    guard.original_window = target.0 as usize;
                    guard.last_external_window = target.0 as usize;
                } else {
                    guard.original_window = guard.last_external_window;
                }
                Some(state.ui.clone())
            });
            if let Some(weak) = weak {
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        show_page(&ui, page);
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
    registered: &mut [bool; 3],
) -> Result<(), String> {
    for (index, action) in [
        Action::OpenHistory,
        Action::OpenSettings,
        Action::OpenTemplates,
    ]
    .into_iter()
    .enumerate()
    {
        let binding = settings.binding(action);
        let modifiers = HOT_KEY_MODIFIERS(binding.modifiers as u32) | MOD_NOREPEAT;
        if let Err(error) =
            unsafe { RegisterHotKey(Some(window), index as i32 + 1, modifiers, binding.key) }
        {
            return Err(format!(
                "{} ({}) could not be registered: {error}",
                action.name(),
                binding.label()
            ));
        }
        registered[index] = true;
    }
    Ok(())
}

unsafe fn unregister_hotkeys(window: HWND, registered: &mut [bool; 3]) {
    for (index, flag) in registered.iter_mut().enumerate() {
        if *flag {
            let _ = unsafe { UnregisterHotKey(Some(window), index as i32 + 1) };
            *flag = false;
        }
    }
}

fn worker_loop(
    monitor_window: usize,
    commands: Receiver<WorkerCommand>,
    own_sequence: Arc<AtomicU32>,
    ui: slint::Weak<MainWindow>,
    ui_state: Arc<Mutex<UiState>>,
    data: Arc<Mutex<UiData>>,
    initial_limit: u64,
) {
    let directory = match store::default_directory() {
        Ok(directory) => directory,
        Err(_) => {
            report_error(
                &ui,
                &ui_state,
                &data,
                "Local history directory is unavailable.",
            );
            return;
        }
    };
    let template_path = directory
        .parent()
        .map(|parent| parent.join("templates.json"));
    let mut store = match Store::open(directory, initial_limit) {
        Ok(store) => store,
        Err(_) => {
            report_error(&ui, &ui_state, &data, "Could not open local history.");
            return;
        }
    };
    let mut thumbnails = HashMap::new();
    publish(&ui, &ui_state, &data, &store, &mut thumbnails);
    let mut templates = template_path.and_then(|path| TemplateStore::open(path).ok());
    if let Some(templates) = templates.as_ref() {
        publish_templates(&ui, &ui_state, &data, templates);
    } else {
        report_error(&ui, &ui_state, &data, "Could not open template database.");
    }
    let mut last_sequence = 0;
    let paste_busy = Arc::new(AtomicBool::new(false));
    while let Ok(command) = commands.recv() {
        match command {
            WorkerCommand::ClipboardChanged(sequence) => {
                if sequence == 0 || sequence == last_sequence {
                    continue;
                }
                last_sequence = sequence;
                if sequence == own_sequence.load(Ordering::Acquire) {
                    continue;
                }
                if unsafe { GetClipboardSequenceNumber() } != sequence {
                    continue;
                }
                if unsafe { GetClipboardOwner() }
                    .is_ok_and(|owner| owner.0 as usize == monitor_window)
                {
                    continue;
                }
                if let Some(clip) = unsafe { windows_clipboard::read() } {
                    if unsafe { GetClipboardSequenceNumber() } != sequence {
                        continue;
                    }
                    match store.insert_data(clip) {
                        Ok(true) => publish(&ui, &ui_state, &data, &store, &mut thumbnails),
                        Ok(false) => {}
                        Err(_) => {
                            publish(&ui, &ui_state, &data, &store, &mut thumbnails);
                            report_error(
                                &ui,
                                &ui_state,
                                &data,
                                "Could not save clipboard history.",
                            );
                        }
                    }
                }
            }
            WorkerCommand::Restore(id) => {
                if let Some(entry) = store.get(id) {
                    let owner = HWND(monitor_window as *mut c_void);
                    match unsafe { windows_clipboard::write(&entry.data, owner) } {
                        Ok(result) => {
                            own_sequence.store(result.sequence, Ordering::Release);
                            post_status(
                                &ui,
                                if result.complete {
                                    "Copied to clipboard."
                                } else {
                                    "Some clipboard formats could not be restored."
                                },
                            );
                        }
                        Err(_) => report_error(
                            &ui,
                            &ui_state,
                            &data,
                            "Could not restore the clip to the clipboard.",
                        ),
                    }
                }
            }
            WorkerCommand::Paste(id, target) => {
                if paste_busy.swap(true, Ordering::AcqRel) {
                    post_paste_error(&ui, "A paste is already in progress.".into());
                    continue;
                }
                let Some(entry) = store.get(id) else {
                    paste_busy.store(false, Ordering::Release);
                    post_paste_error(&ui, "The selected clip is unavailable.".into());
                    continue;
                };
                let clip = entry.data.clone();
                let settings = lock(&ui_state).saved.clone();
                let busy = Arc::clone(&paste_busy);
                let sequence = Arc::clone(&own_sequence);
                let weak = ui.clone();
                thread::spawn(move || {
                    let result =
                        windows_paste::paste(&clip, target, monitor_window, &settings, &sequence);
                    busy.store(false, Ordering::Release);
                    match result {
                        Ok(message) => post_status(&weak, message),
                        Err(error) => post_paste_error(&weak, error),
                    }
                });
            }
            WorkerCommand::PasteText(text, target) => {
                if paste_busy.swap(true, Ordering::AcqRel) {
                    post_paste_error(&ui, "A paste is already in progress.".into());
                    continue;
                }
                let Some(clip) = store::ClipData::text(text) else {
                    paste_busy.store(false, Ordering::Release);
                    post_paste_error(&ui, "The template is empty or too large.".into());
                    continue;
                };
                let settings = lock(&ui_state).saved.clone();
                let busy = Arc::clone(&paste_busy);
                let sequence = Arc::clone(&own_sequence);
                let weak = ui.clone();
                thread::spawn(move || {
                    let result =
                        windows_paste::paste(&clip, target, monitor_window, &settings, &sequence);
                    busy.store(false, Ordering::Release);
                    match result {
                        Ok(message) => post_status(&weak, message),
                        Err(error) => post_paste_error(&weak, error),
                    }
                });
            }
            WorkerCommand::RestoreText(text) => {
                if let Some(clip) = store::ClipData::text(text) {
                    let owner = HWND(monitor_window as *mut c_void);
                    match unsafe { windows_clipboard::write(&clip, owner) } {
                        Ok(result) => {
                            own_sequence.store(result.sequence, Ordering::Release);
                            post_status(&ui, "Template copied to clipboard.");
                        }
                        Err(_) => report_error(
                            &ui,
                            &ui_state,
                            &data,
                            "Could not copy template to clipboard.",
                        ),
                    }
                }
            }
            WorkerCommand::Delete(id) => match store.remove(id) {
                Ok(true) => {
                    publish(&ui, &ui_state, &data, &store, &mut thumbnails);
                    post_status(&ui, "Clip deleted.");
                }
                Ok(false) => post_status(&ui, "Clip is no longer available."),
                Err(_) => report_error(&ui, &ui_state, &data, "Could not delete the clip."),
            },
            WorkerCommand::Clear => match store.clear() {
                Ok(()) => {
                    publish(&ui, &ui_state, &data, &store, &mut thumbnails);
                    post_status(&ui, "History cleared.");
                }
                Err(_) => {
                    publish(&ui, &ui_state, &data, &store, &mut thumbnails);
                    report_error(&ui, &ui_state, &data, "Could not clear clipboard history.");
                }
            },
            WorkerCommand::TemplateAddGroup(parent) => {
                apply_template_change(
                    &mut templates,
                    &ui,
                    &ui_state,
                    &data,
                    "Group created.",
                    |templates| {
                        templates
                            .add_group(parent, "New group".into())
                            .map(|id| Some(TemplateNode::Group(id)))
                    },
                );
            }
            WorkerCommand::TemplateRenameGroup(id, name) => {
                apply_template_change(
                    &mut templates,
                    &ui,
                    &ui_state,
                    &data,
                    "Group saved.",
                    |templates| templates.rename_group(id, name).map(|()| None),
                );
            }
            WorkerCommand::TemplateRemoveGroup(id) => {
                apply_template_change(
                    &mut templates,
                    &ui,
                    &ui_state,
                    &data,
                    "Group deleted.",
                    |templates| {
                        templates
                            .remove_group(id)
                            .map(|()| Some(TemplateNode::Root))
                    },
                );
            }
            WorkerCommand::TemplateAddSnippet(group) => {
                apply_template_change(
                    &mut templates,
                    &ui,
                    &ui_state,
                    &data,
                    "Template created.",
                    |templates| {
                        templates
                            .add_snippet(group, "New template".into())
                            .map(|id| Some(TemplateNode::Snippet(id)))
                    },
                );
            }
            WorkerCommand::TemplateSaveSnippet(id, name, content) => {
                apply_template_change(
                    &mut templates,
                    &ui,
                    &ui_state,
                    &data,
                    "Template saved.",
                    |templates| templates.save_snippet(id, name, content).map(|()| None),
                );
            }
            WorkerCommand::TemplateRemoveSnippet(id) => {
                apply_template_change(
                    &mut templates,
                    &ui,
                    &ui_state,
                    &data,
                    "Template deleted.",
                    |templates| {
                        templates
                            .remove_snippet(id)
                            .map(|()| Some(TemplateNode::Root))
                    },
                );
            }
            WorkerCommand::SetLimit(limit) => match store.set_limit(limit) {
                Ok(()) => publish(&ui, &ui_state, &data, &store, &mut thumbnails),
                Err(_) => report_error(&ui, &ui_state, &data, "Could not apply history limit."),
            },
        }
    }
}

fn publish(
    ui: &slint::Weak<MainWindow>,
    ui_state: &Arc<Mutex<UiState>>,
    data: &Arc<Mutex<UiData>>,
    store: &Store,
    thumbnails: &mut HashMap<u128, Option<SharedPixelBuffer<Rgba8Pixel>>>,
) {
    let visible_ids: HashSet<u128> = store
        .entries()
        .iter()
        .take(1000)
        .map(|entry| entry.id)
        .collect();
    thumbnails.retain(|id, _| visible_ids.contains(id));
    let entries = store
        .entries()
        .iter()
        .take(1000)
        .map(|entry| {
            let image = entry.data.image.as_ref();
            let preview = entry
                .data
                .text
                .as_ref()
                .map(|text| {
                    text.lines()
                        .next()
                        .unwrap_or("")
                        .chars()
                        .take(240)
                        .collect()
                })
                .unwrap_or_else(|| {
                    image
                        .map(|image| format!("Image {} × {}", image.width, image.height))
                        .unwrap_or_default()
                });
            let search = entry
                .data
                .text
                .as_ref()
                .map(|text| text.chars().take(32_000).collect::<String>().to_lowercase())
                .unwrap_or_else(|| preview.to_lowercase());
            Summary {
                id: entry.id,
                preview,
                search,
                kind: match (entry.data.text.is_some(), image.is_some()) {
                    (true, true) => "TEXT + IMAGE",
                    (true, false) => "TEXT CLIP",
                    (false, true) => "IMAGE CLIP",
                    _ => "CLIP",
                },
                thumbnail: image.and_then(|image| {
                    thumbnails
                        .entry(entry.id)
                        .or_insert_with(|| image_preview::thumbnail(image))
                        .clone()
                }),
            }
        })
        .collect();
    let mut data_guard = lock(data);
    data_guard.entries = entries;
    queue_update(ui, ui_state, data, &mut data_guard);
}

fn report_error(
    ui: &slint::Weak<MainWindow>,
    ui_state: &Arc<Mutex<UiState>>,
    data: &Arc<Mutex<UiData>>,
    message: &str,
) {
    let mut data_guard = lock(data);
    data_guard.error = Some(message.into());
    queue_update(ui, ui_state, data, &mut data_guard);
}

fn publish_templates(
    ui: &slint::Weak<MainWindow>,
    ui_state: &Arc<Mutex<UiState>>,
    data: &Arc<Mutex<UiData>>,
    templates: &TemplateStore,
) {
    let mut guard = lock(data);
    guard.groups = templates.groups().to_vec();
    guard.snippets = templates.snippets().to_vec();
    guard.template_changed = true;
    queue_update(ui, ui_state, data, &mut guard);
}

fn apply_template_change(
    templates: &mut Option<TemplateStore>,
    ui: &slint::Weak<MainWindow>,
    ui_state: &Arc<Mutex<UiState>>,
    data: &Arc<Mutex<UiData>>,
    success: &'static str,
    change: impl FnOnce(&mut TemplateStore) -> std::io::Result<Option<TemplateNode>>,
) {
    let Some(templates) = templates.as_mut() else {
        report_error(ui, ui_state, data, "Template database is unavailable.");
        return;
    };
    match change(templates) {
        Ok(selected) => {
            if let Some(selected) = selected {
                let mut guard = lock(ui_state);
                guard.selected_template = selected;
                guard.collapsed_groups.clear();
            }
            publish_templates(ui, ui_state, data, templates);
            post_status(ui, success);
        }
        Err(error) => report_error(ui, ui_state, data, &error.to_string()),
    }
}

fn queue_update(
    ui: &slint::Weak<MainWindow>,
    ui_state: &Arc<Mutex<UiState>>,
    data: &Arc<Mutex<UiData>>,
    guard: &mut UiData,
) {
    if guard.pending {
        return;
    }
    guard.pending = true;
    let weak = ui.clone();
    let ui_state = Arc::clone(ui_state);
    let data = Arc::clone(data);
    if slint::invoke_from_event_loop(move || {
        let (error, template_changed) = {
            let mut data_guard = lock(&data);
            data_guard.pending = false;
            let changed = data_guard.template_changed;
            data_guard.template_changed = false;
            (data_guard.error.take(), changed)
        };
        if let Some(ui) = weak.upgrade() {
            render_history(&ui, &ui_state, &data);
            if template_changed {
                render_templates(&ui, &ui_state, &data);
                render_template_editor(&ui, &ui_state, &data);
            }
            if let Some(error) = error {
                set_status(&ui, &error);
            }
        }
    })
    .is_err()
    {
        guard.pending = false;
    }
}

fn post_status(ui: &slint::Weak<MainWindow>, message: &'static str) {
    let weak = ui.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            set_status(&ui, message);
        }
    });
}

fn post_paste_error(ui: &slint::Weak<MainWindow>, message: String) {
    let weak = ui.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            show_page(&ui, ui.get_page());
            set_status(&ui, &message);
        }
    });
}
