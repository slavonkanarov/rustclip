use super::*;
use crate::settings::{ALT, Action, Binding, CTRL, SHIFT, WIN, key_name};
use slint::{ModelRc, VecModel};
use windows::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONQUESTION, MB_YESNO, MessageBoxW};
use windows::core::w;

pub(super) fn wire_settings(
    ui: &MainWindow,
    state: &Arc<Mutex<UiState>>,
    worker: &SyncSender<WorkerCommand>,
    monitor: &MonitorHandle,
    settings_path: PathBuf,
) {
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
            ui.invoke_refresh_preview();
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
    let state_for_clear = Arc::clone(state);
    ui.on_clear_shortcut(move |index| {
        if let Some(ui) = weak.upgrade() {
            if let Some(action) = Action::ALL
                .get(index as usize)
                .filter(|action| action.optional())
            {
                let mut guard = lock(&state_for_clear);
                guard.draft.bindings[action.index()] = Binding::new(0, 0);
                guard.capture = None;
                drop(guard);
                render_shortcuts(&ui, &state_for_clear);
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
                    if result.is_ok() {
                        ui.invoke_refresh_preview();
                    }
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
                    if let Err(error) = &result {
                        show_startup_error(error);
                    }
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
}

pub(super) fn render_shortcuts(ui: &MainWindow, state: &Arc<Mutex<UiState>>) {
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
                optional: action.optional(),
            }
        })
        .collect();
    drop(state);
    ui.set_shortcuts(ModelRc::new(VecModel::from(rows)));
}

pub(super) fn render_general_settings(ui: &MainWindow, state: &Arc<Mutex<UiState>>) {
    let draft = lock(state).draft.clone();
    let saved = lock(state).saved.clone();
    ui.set_quick_hint(
        format!(
            "{}: older  ·  {}: newer  ·  Double-click to paste",
            saved.binding(Action::BrowseOlder).label(),
            saved.binding(Action::BrowseNewer).label()
        )
        .into(),
    );
    ui.set_start_hidden(draft.start_hidden);
    ui.set_start_with_windows(draft.start_with_windows);
    ui.set_paste_enabled(draft.paste_enabled);
    ui.set_restore_clipboard(draft.restore_clipboard);
    ui.set_paste_delay_text(draft.paste_delay_ms.to_string().into());
    ui.set_history_limit_text(draft.history_limit_mb.to_string().into());
}

pub(super) fn confirm_clear(ui: &MainWindow, worker: &SyncSender<WorkerCommand>) {
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
