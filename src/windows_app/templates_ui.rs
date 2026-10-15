use super::*;
use slint::{ModelRc, VecModel};
use windows::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONQUESTION, MB_YESNO, MessageBoxW};
use windows::core::w;

pub(super) fn wire_templates(
    ui: &MainWindow,
    state: &Arc<Mutex<UiState>>,
    data: &Arc<Mutex<UiData>>,
    worker: &SyncSender<WorkerCommand>,
) {
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
}

pub(super) fn selected_group(
    state: &Arc<Mutex<UiState>>,
    data: &Arc<Mutex<UiData>>,
) -> Option<u64> {
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

pub(super) fn render_templates(
    ui: &MainWindow,
    state: &Arc<Mutex<UiState>>,
    data: &Arc<Mutex<UiData>>,
) {
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

pub(super) fn append_template_groups(
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

pub(super) fn render_template_editor(
    ui: &MainWindow,
    state: &Arc<Mutex<UiState>>,
    data: &Arc<Mutex<UiData>>,
) {
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

pub(super) fn confirm_template_discard(
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
