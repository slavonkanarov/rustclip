use super::*;
use crate::settings::{ALT, Action, CTRL, SHIFT, WIN, valid_key};

pub(super) fn handle_key(
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
    let Some(key) = key else { return false };
    let modifiers = (if alt { ALT } else { 0 })
        | (if ctrl { CTRL } else { 0 })
        | (if shift { SHIFT } else { 0 })
        | (if win { WIN } else { 0 });
    let saved = lock(state).saved.clone();
    let close = saved.binding(Action::CloseView);
    if close.key == key && close.modifiers == modifiers {
        if ui.get_settings_open() {
            ui.invoke_cancel_settings();
        } else if ui.get_template_editor_open() {
            ui.invoke_cancel_template_edit();
        } else {
            let _ = ui.hide();
            ui.invoke_refresh_preview();
        }
        return true;
    }
    if ui.get_settings_open() || ui.get_template_editor_open() {
        return false;
    }
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

pub(super) fn key_from_slint(text: &str) -> Option<u32> {
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
