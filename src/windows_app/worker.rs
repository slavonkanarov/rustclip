use super::*;
use crate::store::Store;
use crate::templates::TemplateStore;
use crate::{image_preview, windows_clipboard, windows_paste};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::DataExchange::{GetClipboardOwner, GetClipboardSequenceNumber};

pub(super) fn worker_loop(
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
            WorkerCommand::Paste {
                id,
                target,
                expected_sequence,
            } => {
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
                    let result = windows_paste::paste(
                        &clip,
                        target,
                        monitor_window,
                        &settings,
                        &sequence,
                        expected_sequence,
                    );
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
                    let result = windows_paste::paste(
                        &clip,
                        target,
                        monitor_window,
                        &settings,
                        &sequence,
                        None,
                    );
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

pub(super) fn publish(
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
                detail: entry
                    .data
                    .text
                    .as_ref()
                    .map(|text| text.chars().take(2_000).collect())
                    .unwrap_or_else(|| {
                        image
                            .map(|image| format!("Image {} × {}", image.width, image.height))
                            .unwrap_or_default()
                    }),
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
    data_guard.entries = Arc::new(entries);
    queue_update(ui, ui_state, data, &mut data_guard);
}

pub(super) fn report_error(
    ui: &slint::Weak<MainWindow>,
    ui_state: &Arc<Mutex<UiState>>,
    data: &Arc<Mutex<UiData>>,
    message: &str,
) {
    let mut data_guard = lock(data);
    data_guard.error = Some(message.into());
    queue_update(ui, ui_state, data, &mut data_guard);
}

pub(super) fn publish_templates(
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

pub(super) fn apply_template_change(
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

pub(super) fn queue_update(
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

pub(super) fn post_status(ui: &slint::Weak<MainWindow>, message: &'static str) {
    let weak = ui.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            set_status(&ui, message);
        }
    });
}

pub(super) fn post_paste_error(ui: &slint::Weak<MainWindow>, message: String) {
    let weak = ui.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            show_page(&ui, ui.get_page());
            set_status(&ui, &message);
        }
    });
}
