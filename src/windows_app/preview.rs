use super::*;
use crate::history_selection::{Direction, SelectionOutcome};
use crate::settings::{ALT, Action, CTRL, SHIFT, WIN};
use slint::{Image as SlintImage, TimerMode};
use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

pub(super) fn wire_preview(
    ui: &MainWindow,
    preview: &PreviewWindow,
    state: &Arc<Mutex<UiState>>,
    data: &Arc<Mutex<UiData>>,
    worker: &SyncSender<WorkerCommand>,
) -> slint::Timer {
    let weak = ui.as_weak();
    let preview_weak = preview.as_weak();
    let state_for_refresh = Arc::clone(state);
    let data_for_refresh = Arc::clone(data);
    ui.on_refresh_preview(move || {
        if let (Some(ui), Some(preview)) = (weak.upgrade(), preview_weak.upgrade()) {
            refresh_main(&ui, &preview, &state_for_refresh, &data_for_refresh);
        }
    });
    let timer = slint::Timer::default();
    let weak = ui.as_weak();
    let preview_weak = preview.as_weak();
    let state = Arc::clone(state);
    let data = Arc::clone(data);
    let worker = worker.clone();
    timer.start(TimerMode::Repeated, Duration::from_millis(30), move || {
        let (Some(ui), Some(preview)) = (weak.upgrade(), preview_weak.upgrade()) else {
            return;
        };
        let mut guard = lock(&state);
        if let Some(pending) = &guard.quick {
            let foreground = unsafe { GetForegroundWindow() };
            let mut process = 0;
            unsafe { GetWindowThreadProcessId(foreground, Some(&mut process)) };
            let foreground = if process == std::process::id() {
                pending.target
            } else {
                foreground.0 as usize
            };
            match pending.outcome(
                held_modifiers(),
                unsafe { GetClipboardSequenceNumber() },
                foreground,
            ) {
                SelectionOutcome::Cancel => {
                    drop(guard);
                    cancel(&ui, &preview, &state);
                    return;
                }
                SelectionOutcome::Waiting => return,
                SelectionOutcome::Paste => {}
            }
            let pending = guard.quick.take().unwrap();
            let paste_enabled = guard.saved.paste_enabled;
            drop(guard);
            let _ = preview.hide();
            if paste_enabled {
                let _ = ui.hide();
                let command = WorkerCommand::Paste {
                    id: pending.id,
                    target: pending.target,
                    expected_sequence: Some(pending.sequence),
                };
                if let Err(error) = send_worker(&worker, command) {
                    show_page(&ui, 0);
                    set_status(&ui, error);
                }
            } else {
                set_status(
                    &ui,
                    "Enable paste in Settings > Paste settings to use quick paste.",
                );
                show_page(&ui, 2);
            }
            return;
        }
        if guard
            .preview_until
            .is_some_and(|deadline| Instant::now() < deadline)
        {
            return;
        }
        guard.preview_until = None;
        drop(guard);
        refresh_main(&ui, &preview, &state, &data);
    });
    timer
}

pub(super) fn cancel(ui: &MainWindow, preview: &PreviewWindow, state: &Arc<Mutex<UiState>>) {
    let mut guard = lock(state);
    if let Some(pending) = guard.quick.take() {
        guard.browse = pending.previous;
    }
    guard.preview_until = None;
    drop(guard);
    let _ = preview.hide();
    ui.invoke_refresh_preview();
}

fn refresh_main(
    ui: &MainWindow,
    preview: &PreviewWindow,
    state: &Arc<Mutex<UiState>>,
    data: &Arc<Mutex<UiData>>,
) {
    let selected = {
        let guard = lock(state);
        if guard.quick.is_some() || guard.preview_until.is_some() {
            return;
        }
        guard.selected
    };
    let mut process = 0;
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut process)) };
    if !ui.window().is_visible()
        || process != std::process::id()
        || ui.get_page() != 0
        || ui.get_settings_open()
        || ui.get_template_editor_open()
    {
        let _ = preview.hide();
        return;
    }
    let data = lock(data);
    let Some(index) = data
        .entries
        .iter()
        .position(|entry| Some(entry.id) == selected)
    else {
        let _ = preview.hide();
        return;
    };
    set_entry(preview, &data.entries[index], index, data.entries.len());
    drop(data);
    preview.set_quick_mode(false);
    preview.set_hint_text("Double-click a clip to paste into the previous application.".into());
    window::position_preview(ui, preview, 0, false);
    window::show_preview(preview);
}

pub(super) fn browse(
    ui: &MainWindow,
    preview: &PreviewWindow,
    state: &Arc<Mutex<UiState>>,
    data: &Arc<Mutex<UiData>>,
    direction: Direction,
) {
    let data_guard = lock(data);
    let ids: Vec<_> = data_guard.entries.iter().map(|entry| entry.id).collect();
    let mut guard = lock(state);
    let previous = guard
        .quick
        .as_ref()
        .map_or_else(|| guard.browse.clone(), |pending| pending.previous.clone());
    let selected = guard.browse.step(&ids, direction);
    let target = guard
        .quick
        .as_ref()
        .map_or(guard.original_window, |pending| pending.target);
    preview.set_quick_mode(true);
    if let Some((id, index)) = selected {
        set_entry(preview, &data_guard.entries[index], index, ids.len());
        let action = match direction {
            Direction::Older => Action::BrowseOlder,
            Direction::Newer => Action::BrowseNewer,
        };
        let sequence = guard.quick.as_ref().map_or_else(
            || unsafe { GetClipboardSequenceNumber() },
            |pending| pending.sequence,
        );
        guard.quick = Some(PendingSelection {
            id,
            target,
            sequence,
            modifiers: guard.saved.binding(action).modifiers,
            previous,
        });
        guard.preview_until = None;
        guard.selected = Some(id);
        preview.set_hint_text(
            format!(
                "Release modifiers to paste · {} to cancel",
                guard.saved.binding(Action::CancelQuick).label()
            )
            .into(),
        );
    } else {
        preview.set_entry_id("".into());
        preview.set_preview_kind("Clipboard history".into());
        preview.set_preview_count("0 clips".into());
        preview.set_preview_text("Copy text or an image to start your history.".into());
        preview.set_preview_image(SlintImage::default());
        preview.set_hint_text("No saved clips are available.".into());
        guard.preview_until = Some(Instant::now() + Duration::from_secs(2));
    }
    drop(guard);
    drop(data_guard);
    window::position_preview(ui, preview, target, !ui.window().is_visible());
    window::show_preview(preview);
    if ui.window().is_visible() {
        render_history(ui, state, data);
    }
}

fn set_entry(preview: &PreviewWindow, entry: &Summary, index: usize, count: usize) {
    let entry_id = entry.id.to_string();
    let count_text = format!("{} / {count}", index + 1);
    if preview.get_entry_id() == entry_id && preview.get_preview_count() == count_text {
        return;
    }
    preview.set_entry_id(entry_id.into());
    preview.set_preview_kind(entry.kind.into());
    preview.set_preview_count(format!("{} / {count}", index + 1).into());
    preview.set_preview_text(entry.detail.clone().into());
    preview.set_preview_image(
        entry
            .thumbnail
            .clone()
            .map(SlintImage::from_rgba8)
            .unwrap_or_default(),
    );
}

fn held_modifiers() -> u8 {
    let held = |key: u16| unsafe { GetAsyncKeyState(key as i32) < 0 };
    (if held(VK_CONTROL.0) { CTRL } else { 0 })
        | (if held(VK_MENU.0) { ALT } else { 0 })
        | (if held(VK_SHIFT.0) { SHIFT } else { 0 })
        | (if held(VK_LWIN.0) || held(VK_RWIN.0) {
            WIN
        } else {
            0
        })
}
