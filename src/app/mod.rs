mod preferences;
mod ui;
mod vault;
mod widgets;

use crate::{
    clipboard::{self, Command, Event},
    model::{self, Clip, Settings},
    secrets,
};
use eframe::egui;
use std::{collections::HashMap, sync::mpsc, time::Instant};
use widgets::{ACCENT, BACKGROUND, SURFACE};

pub(crate) struct App {
    clips: Vec<Clip>,
    settings: Settings,
    tx: mpsc::Sender<Command>,
    rx: mpsc::Receiver<Event>,
    selected: Option<usize>,
    scroll_to_newest: bool,
    query: String,
    snippets: bool,
    vault: Option<secrets::VaultData>,
    credential: Option<secrets::SessionKey>,
    credential_input: String,
    credential_confirm: String,
    unlock_input: String,
    unlock_open: bool,
    focus_unlock: bool,
    pin_mode: bool,
    secret_editor: Option<secrets::Secret>,
    secret_selected: Option<usize>,
    secret_copied: Option<(usize, Instant)>,
    folder_editor: Option<secrets::Folder>,
    folder_edit_original: Option<String>,
    selected_folder: Option<String>,
    secret_app_filter: String,
    filter_active_app: bool,
    pending_app: String,
    stars: bool,
    settings_open: bool,
    confirm_discard: bool,
    focus_settings: u8,
    settings_last_active: bool,
    settings_page: usize,
    hotkey_recorder: Option<(usize, crate::hotkeys::Recorder)>,
    draft: Settings,
    adding_capture: Option<String>,
    adding_hotkeys: Option<String>,
    app_icons: HashMap<String, egui::TextureHandle>,
    editor: Option<(Option<usize>, String)>,
    status: String,
    texture: Option<(u128, u32, egui::TextureHandle)>,
    quitting: bool,
}

impl App {
    pub(crate) fn new(cc: &eframe::CreationContext<'_>, initial_target: usize) -> Self {
        cc.egui_ctx.set_theme(egui::ThemePreference::Dark);
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BACKGROUND;
        visuals.window_fill = SURFACE;
        visuals.extreme_bg_color = egui::Color32::from_rgb(16, 20, 28);
        visuals.selection.bg_fill = ACCENT;
        visuals.override_text_color = Some(egui::Color32::from_rgb(226, 232, 241));
        for widget in [
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
        ] {
            widget.corner_radius = egui::CornerRadius::same(7);
        }
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(37, 45, 59);
        visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(37, 45, 59);
        visuals.widgets.inactive.bg_stroke =
            egui::Stroke::new(1., egui::Color32::from_rgb(69, 82, 101));
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(44, 57, 74);
        visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(44, 57, 74);
        visuals.widgets.active.bg_fill = ACCENT;
        cc.egui_ctx.set_visuals(visuals);
        cc.egui_ctx.style_mut_of(egui::Theme::Dark, |style| {
            style.interaction.tooltip_delay = 0.1;
            style.interaction.show_tooltips_only_when_still = false;
            style.spacing.button_padding = egui::vec2(14.0, 8.0);
            style.spacing.item_spacing = egui::vec2(10.0, 10.0);
            style.spacing.interact_size.y = 34.;
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(15.));
            style
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(15.));
            style
                .text_styles
                .insert(egui::TextStyle::Heading, egui::FontId::proportional(24.));
        });
        let (mut clips, mut settings, errors) = model::load();
        if std::env::args().any(|s| s == "--demo") {
            settings.monitor = false;
            settings.auto_paste = false;
            if clips.is_empty() {
                clips.push(clipboard::text_clip(
                    "Добро пожаловать в RustClip!\nИстория, поиск, секреты и центральное превью."
                        .into(),
                ));
            }
        }
        let (tx, commands) = mpsc::channel();
        let (events, rx) = mpsc::channel();
        clipboard::start(
            events,
            commands,
            settings.clone(),
            cc.egui_ctx.clone(),
            initial_target,
        );
        let selected = clips.len().checked_sub(1);
        Self {
            clips,
            selected,
            scroll_to_newest: true,
            draft: settings.clone(),
            adding_capture: None,
            adding_hotkeys: None,
            app_icons: HashMap::new(),
            settings,
            tx,
            rx,
            query: String::new(),
            snippets: false,
            vault: None,
            credential: None,
            credential_input: String::new(),
            credential_confirm: String::new(),
            unlock_input: String::new(),
            unlock_open: false,
            focus_unlock: false,
            pin_mode: secrets::mode().unwrap_or(true),
            secret_editor: None,
            secret_selected: None,
            secret_copied: None,
            folder_editor: None,
            folder_edit_original: None,
            selected_folder: None,
            secret_app_filter: String::new(),
            filter_active_app: false,
            pending_app: String::new(),
            stars: false,
            settings_open: false,
            confirm_discard: false,
            focus_settings: 0,
            settings_last_active: false,
            settings_page: 0,
            hotkey_recorder: None,
            editor: None,
            status: errors.join("\n"),
            texture: None,
            quitting: false,
        }
    }
    fn send(&mut self, command: Command) {
        if self.tx.send(command).is_err() {
            self.status = "Clipboard worker остановлен".into();
        }
    }
    fn persist(&mut self, index: usize) {
        if let Err(e) = model::save_clip(&self.clips[index]) {
            self.status = e;
        }
    }
    fn add(&mut self, mut clip: Clip) {
        if !clip.snippet
            && self
                .clips
                .iter()
                .rev()
                .filter(|c| !c.snippet)
                .take(self.settings.duplicates)
                .any(|c| c.same_content(&clip))
        {
            return;
        }
        clip.id = clip.id.max(self.clips.last().map_or(0, |c| c.id + 1));
        self.clips.push(clip);
        let i = self.clips.len() - 1;
        self.selected = Some(i);
        self.persist(i);
        if self.settings.limit > 0 {
            while self
                .clips
                .iter()
                .filter(|c| !c.snippet && !c.starred)
                .count()
                > self.settings.limit
            {
                if let Some(i) = self.clips.iter().position(|c| !c.snippet && !c.starred) {
                    if !self.remove(i) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
    }
    fn remove(&mut self, index: usize) -> bool {
        if self.clips[index].starred {
            self.status = "Сначала снимите звёздочку".into();
            return false;
        }
        let path = model::directory()
            .join("clips")
            .join(format!("{}.json", self.clips[index].id));
        if let Err(e) = std::fs::remove_file(path) {
            self.status = e.to_string();
            return false;
        }
        self.clips.remove(index);
        self.selected = self.clips.len().checked_sub(1);
        true
    }
    fn action(&mut self, ctx: &egui::Context, paste: bool, plain: bool) {
        if let Some(i) = self.selected.filter(|&i| i < self.clips.len()) {
            let clip = self.clips[i].clone();
            let number =
                (!clip.snippet).then(|| self.clips[i..].iter().filter(|c| !c.snippet).count());
            if paste {
                self.send(Command::Paste(clip, plain, None, number));
            } else {
                self.send(Command::Copy(clip, plain, number));
            }
            if self.settings.hide_after {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }
    }
    fn select_adjacent(&mut self, older: bool) -> bool {
        let history: Vec<_> = self
            .clips
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.snippet)
            .map(|(i, _)| i)
            .collect();
        if history.is_empty() {
            return false;
        }
        let pos = self
            .selected
            .and_then(|i| history.iter().position(|n| *n == i))
            .unwrap_or(history.len() - 1);
        self.selected = Some(
            history[if older {
                pos.saturating_sub(1)
            } else {
                (pos + 1).min(history.len() - 1)
            }],
        );
        true
    }
    fn cycle(&mut self, ctx: &egui::Context, older: bool, destination: Option<usize>) {
        if !self.select_adjacent(older) {
            return;
        }
        if let Some(i) = self.selected.filter(|&i| i < self.clips.len()) {
            let clip = self.clips[i].clone();
            let number = Some(self.clips[i..].iter().filter(|c| !c.snippet).count());
            if self.settings.auto_paste {
                self.send(Command::Paste(clip, false, destination, number));
            } else {
                self.send(Command::Copy(clip, false, number));
            }
            if self.settings.hide_after {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }
    }
}
