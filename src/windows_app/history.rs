use super::*;
use slint::{Image as SlintImage, ModelRc, VecModel};
use windows::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONQUESTION, MB_YESNO, MessageBoxW};
use windows::core::w;

pub(super) fn wire_history(
    ui: &MainWindow,
    state: &Arc<Mutex<UiState>>,
    data: &Arc<Mutex<UiData>>,
    worker: &SyncSender<WorkerCommand>,
) {
    let weak = ui.as_weak();
    let state_for_select = Arc::clone(state);
    ui.on_select_clip(move |index| {
        if let Some(ui) = weak.upgrade() {
            select_index(&ui, &state_for_select, index);
        }
    });
    let weak = ui.as_weak();
    let state_for_activate = Arc::clone(state);
    ui.on_activate_clip(move |index| {
        if let Some(ui) = weak.upgrade() {
            let mut guard = lock(&state_for_activate);
            let Some(id) = guard.visible_ids.get(index as usize).copied() else {
                return;
            };
            guard.selected = Some(id);
            drop(guard);
            ui.invoke_paste_selected();
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
            let (Some(id), target) = (
                guard.selected,
                if guard.last_external_window != 0 {
                    guard.last_external_window
                } else {
                    guard.original_window
                },
            ) else {
                return;
            };
            let enabled = guard.saved.paste_enabled;
            drop(guard);
            if !enabled {
                set_status(
                    &ui,
                    "Enable paste in Settings > Paste settings to paste this clip.",
                );
                return;
            }
            if target == 0 {
                set_status(&ui, "Open history from the target app before pasting.");
                return;
            }
            if let Err(error) = send_worker(
                &worker_for_paste,
                WorkerCommand::Paste {
                    id,
                    target,
                    expected_sequence: None,
                },
            ) {
                set_status(&ui, error);
            } else {
                let _ = ui.hide();
                ui.invoke_refresh_preview();
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
}

pub(super) fn render_history(
    ui: &MainWindow,
    state: &Arc<Mutex<UiState>>,
    data: &Arc<Mutex<UiData>>,
) {
    let query = ui.get_search_text().to_lowercase();
    let entries = Arc::clone(&lock(data).entries);
    let revision = {
        let mut guard = lock(state);
        guard.search_revision = guard.search_revision.wrapping_add(1);
        guard.search_revision
    };
    if query.is_empty() {
        let matches = (0..entries.len()).collect::<Vec<_>>();
        show_matches(ui, state, &entries, &matches);
        return;
    }
    let weak = ui.as_weak();
    let state = Arc::clone(state);
    // Debounce typing, and scan the shared snapshot without holding the UI data lock.
    slint::Timer::single_shot(Duration::from_millis(80), move || {
        if lock(&state).search_revision != revision {
            return;
        }
        thread::spawn(move || {
            let matches: Vec<_> = entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| entry.search.contains(&query).then_some(index))
                .collect();
            let _ = slint::invoke_from_event_loop(move || {
                if lock(&state).search_revision != revision {
                    return;
                }
                if let Some(ui) = weak.upgrade() {
                    show_matches(&ui, &state, &entries, &matches);
                }
            });
        });
    });
}

fn show_matches(
    ui: &MainWindow,
    state: &Arc<Mutex<UiState>>,
    entries: &[Summary],
    matches: &[usize],
) {
    let visible: Vec<_> = matches
        .iter()
        .filter_map(|index| entries.get(*index))
        .collect();
    let mut state = lock(state);
    state.visible_ids = visible.iter().map(|entry| entry.id).collect();
    if !state.visible_ids.contains(&state.selected.unwrap_or(0)) {
        state.selected = state.visible_ids.first().copied();
    }
    let selected = state.selected;
    let selected_index = state
        .visible_ids
        .iter()
        .position(|id| Some(*id) == selected)
        .map(|index| index as i32)
        .unwrap_or(-1);
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
    ui.set_has_selection(selected.is_some());
    ui.set_clips(ModelRc::new(VecModel::from(rows)));
    ui.set_selected_index(selected_index);
    ui.invoke_refresh_preview();
}

pub(super) fn navigate_history(
    ui: &MainWindow,
    state: &Arc<Mutex<UiState>>,
    _data: &Arc<Mutex<UiData>>,
    step: i32,
) {
    let guard = lock(state);
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
    drop(guard);
    select_index(ui, state, next as i32);
}

fn select_index(ui: &MainWindow, state: &Arc<Mutex<UiState>>, index: i32) {
    let mut guard = lock(state);
    let Some(id) = guard.visible_ids.get(index as usize).copied() else {
        return;
    };
    guard.selected = Some(id);
    drop(guard);
    // Keep row instances alive so the second click reaches the same TouchArea.
    use slint::Model;
    let rows = ui.get_clips();
    if let Some(rows) = rows.as_any().downcast_ref::<VecModel<ClipRow>>() {
        for row_index in 0..rows.row_count() {
            if let Some(mut row) = rows.row_data(row_index) {
                let selected = row_index == index as usize;
                if row.selected != selected {
                    row.selected = selected;
                    rows.set_row_data(row_index, row);
                }
            }
        }
    }
    ui.set_selected_index(index);
    ui.invoke_refresh_preview();
}
