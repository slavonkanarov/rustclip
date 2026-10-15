use crate::history_selection::{HistorySelection, PendingSelection};
use crate::settings::{Action, Settings};
use crate::templates::{Group, Snippet};
use crate::{store, windows_autostart};
use slint::{CloseRequestResponse, ComponentHandle, Rgba8Pixel, SharedPixelBuffer};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::AtomicU32;
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

slint::include_modules!();

mod history;
mod keyboard;
mod monitor;
mod preview;
mod settings_ui;
mod templates_ui;
mod window;
mod worker;

use history::{navigate_history, render_history};
use keyboard::handle_key;
use monitor::{MonitorHandle, monitor_loop};
use settings_ui::{confirm_clear, render_general_settings, render_shortcuts};
use templates_ui::{render_template_editor, render_templates};
pub use window::show_startup_error;
use window::{InstanceGuard, capture_target, show_page, show_startup_warning};
use worker::worker_loop;

#[derive(Clone)]
struct Summary {
    id: u128,
    preview: String,
    detail: String,
    search: String,
    kind: &'static str,
    thumbnail: Option<SharedPixelBuffer<Rgba8Pixel>>,
}

#[derive(Default)]
struct UiData {
    entries: Arc<Vec<Summary>>,
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
    search_revision: u64,
    original_window: usize,
    last_external_window: usize,
    saving: bool,
    browse: HistorySelection,
    quick: Option<PendingSelection>,
    preview_until: Option<Instant>,
    visible_template_nodes: Vec<TemplateNode>,
    selected_template: TemplateNode,
    collapsed_groups: HashSet<u64>,
}

impl UiState {
    fn new(settings: Settings) -> Self {
        Self {
            saved: settings.clone(),
            draft: settings.clone(),
            capture: None,
            visible_ids: Vec::new(),
            selected: None,
            search_revision: 0,
            original_window: 0,
            last_external_window: 0,
            saving: false,
            browse: HistorySelection::default(),
            quick: None,
            preview_until: None,
            visible_template_nodes: Vec::new(),
            selected_template: TemplateNode::Root,
            collapsed_groups: HashSet::new(),
        }
    }
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
    Paste {
        id: u128,
        target: usize,
        expected_sequence: Option<u32>,
    },
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

pub fn run() -> Result<(), String> {
    let background = std::env::args_os().any(|argument| argument == "--background");
    let Some(_instance) = InstanceGuard::acquire(background)? else {
        return Ok(());
    };
    let settings_path = store::default_directory()
        .map_err(|error| error.to_string())?
        .parent()
        .ok_or("Could not locate local settings directory.")?
        .join("settings.conf");
    let (settings, mut load_warning) = match Settings::load(&settings_path) {
        Ok(settings) => (settings, None),
        Err(_) => (
            Settings::default(),
            Some("Settings could not be loaded; defaults are active.".to_string()),
        ),
    };
    if settings.start_with_windows {
        if let Err(error) = windows_autostart::set_enabled(true) {
            let warning = format!("Could not update Windows startup entry: {error}");
            load_warning = Some(match load_warning {
                Some(existing) => format!("{existing} {warning}"),
                None => warning,
            });
        }
    }
    let show_on_start = !background || !settings.start_hidden;
    let worker_limit = u64::from(settings.history_limit_mb) * 1024 * 1024;
    window::configure_backend()?;
    let ui = MainWindow::new().map_err(|error| error.to_string())?;
    let preview = PreviewWindow::new().map_err(|error| error.to_string())?;
    let tray = TrayIcon::new().map_err(|error| error.to_string())?;
    let ui_state = Arc::new(Mutex::new(UiState::new(settings.clone())));
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
        let preview_weak = preview.as_weak();
        let ui_state = Arc::clone(&ui_state);
        let ui_data = Arc::clone(&data);
        thread::spawn(move || {
            monitor_loop(
                worker_sender,
                own_sequence,
                ui_weak,
                preview_weak,
                ui_state,
                ui_data,
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
        &ui_state,
        &data,
        &worker_sender,
        &monitor,
        settings_path,
    );
    wire_tray(&ui, &tray, &ui_state);
    if let Some(warning) = registration_warning.as_ref().or(load_warning.as_ref()) {
        set_status(&ui, warning);
    }
    let weak_for_close = ui.as_weak();
    let preview_for_close = preview.as_weak();
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
        if let Some(preview) = preview_for_close.upgrade() {
            if !preview.get_quick_mode() {
                let _ = preview.hide();
            }
        }
        CloseRequestResponse::HideWindow
    });
    let preview_timer = preview::wire_preview(&ui, &preview, &ui_state, &data, &worker_sender);
    let result = match tray.show() {
        Ok(()) => {
            if show_on_start {
                capture_target(&ui_state);
                show_page(&ui, 0);
                let weak = ui.as_weak();
                slint::Timer::single_shot(Duration::from_millis(75), move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.invoke_refresh_preview();
                    }
                });
            }
            if let Some(warning) = registration_warning.as_deref() {
                show_startup_warning(warning);
            }
            slint::run_event_loop().map_err(|error| error.to_string())
        }
        Err(error) => Err(error.to_string()),
    };
    let _ = tray.hide();
    let _ = preview.hide();
    drop(preview_timer);
    monitor.stop();
    let _ = monitor_thread.join();
    drop(ui);
    drop(preview);
    drop(tray);
    drop(monitor);
    drop(worker_sender);
    let _ = worker_thread.join();
    result
}

fn wire_ui(
    ui: &MainWindow,
    state: &Arc<Mutex<UiState>>,
    data: &Arc<Mutex<UiData>>,
    worker: &SyncSender<WorkerCommand>,
    monitor: &MonitorHandle,
    settings_path: PathBuf,
) {
    history::wire_history(ui, state, data, worker);
    settings_ui::wire_settings(ui, state, worker, monitor, settings_path);
    templates_ui::wire_templates(ui, state, data, worker);
    let weak = ui.as_weak();
    ui.on_open_history(move || {
        if let Some(ui) = weak.upgrade() {
            show_page(&ui, 0);
        }
    });
    let weak = ui.as_weak();
    ui.on_open_templates(move || {
        if let Some(ui) = weak.upgrade() {
            show_page(&ui, 1);
        }
    });
    let weak = ui.as_weak();
    ui.on_open_settings(move || {
        if let Some(ui) = weak.upgrade() {
            show_page(&ui, 2);
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
}

fn wire_tray(ui: &MainWindow, tray: &TrayIcon, state: &Arc<Mutex<UiState>>) {
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
    tray.on_exit_app(|| {
        let _ = slint::quit_event_loop();
    });
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

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod ui_tests;
