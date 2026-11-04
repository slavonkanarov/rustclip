use super::{
    App,
    widgets::{ACCENT, BACKGROUND, MUTED, SURFACE, app_list},
};
use crate::{autostart, clipboard::Command, hotkeys, model};
use eframe::egui;
use zeroize::Zeroize;

#[cfg(test)]
mod tests;

const SETTINGS_PAGES: [&str; 7] = [
    "General",
    "Hotkeys",
    "History",
    "Pasting",
    "Advanced",
    "Filtering",
    "Secrets",
];
const SETTINGS_HINTS: [&str; 7] = [
    "Clip capture and window behavior.",
    "Quick access to history and pasting from any application.",
    "Order, storage size, and duplicate clips.",
    "Clipboard behavior when pasting into other applications.",
    "Central preview and text size limits.",
    "Choose applications and formats to skip.",
    "Protection and access to the encrypted vault.",
];

fn settings_navigation(
    ui: &mut egui::Ui,
    page: &mut usize,
    focus: bool,
    language: crate::i18n::Language,
) {
    let mut enter_content = None;
    for (i, name) in SETTINGS_PAGES.iter().enumerate() {
        let response = ui.add_sized(
            [ui.available_width(), 38.],
            egui::Button::new(language.text(name)).selected(*page == i),
        );
        if focus && *page == i {
            response.request_focus();
        }
        if response.clicked() || response.has_focus() {
            *page = i;
        }
        if response.has_focus()
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight))
        {
            enter_content = Some(response.id);
        }
    }
    if let Some(id) = enter_content {
        ui.memory_mut(|memory| {
            memory.surrender_focus(id);
            memory.move_focus(egui::FocusDirection::Next);
        });
    }
}

impl App {
    fn save_preferences(&mut self, ctx: &egui::Context) -> Result<(), String> {
        hotkeys::validate(&self.draft)?;
        self.draft.recent_apps = self.settings.recent_apps.clone();
        if self.draft.autostart != self.settings.autostart {
            autostart::set_enabled(self.draft.autostart)?;
        }
        if let Err(error) =
            model::write_json(&model::directory().join("settings.json"), &self.draft)
        {
            if self.draft.autostart != self.settings.autostart {
                autostart::set_enabled(self.settings.autostart)
                    .map_err(|rollback| format!("{error}\n{rollback}"))?;
            }
            return Err(error);
        }
        self.settings = self.draft.clone();
        ctx.send_viewport_cmd_to(
            egui::ViewportId::ROOT,
            egui::ViewportCommand::Title(
                self.settings
                    .language
                    .text("RustClip (Clipboard history)")
                    .into(),
            ),
        );
        self.send(Command::Settings(self.settings.clone()));
        self.settings_open = false;
        Ok(())
    }

    pub(super) fn open_preferences(&mut self) {
        if !self.settings_open {
            self.draft = self.settings.clone();
        }
        self.settings_open = true;
        self.focus_settings = 2;
    }

    fn close_preferences(&mut self) {
        if self.draft == self.settings {
            self.settings_open = false;
        } else {
            self.confirm_discard = true;
        }
    }
    pub(super) fn preferences(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            self.hotkey_recorder = None;
            self.focus_settings = 0;
            self.credential_input.zeroize();
            self.credential_confirm.zeroize();
            return;
        }
        let language = self.draft.language;
        let focus_settings = self.focus_settings == 1;
        self.focus_settings = self.focus_settings.saturating_sub(1);
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustclip-settings"),
            egui::ViewportBuilder::default()
                .with_title(language.text("Settings — RustClip"))
                .with_inner_size([920., 660.])
                .with_min_inner_size([760., 500.]),
            |ui, _| {
                ui.visuals_mut().widgets.active.bg_stroke = egui::Stroke::new(2., ACCENT);
                if super::widgets::back_requested(ui.ctx())
                    && self.hotkey_recorder.take().is_none()
                {
                    if self.confirm_discard {
                        self.confirm_discard = false;
                    } else if self.adding_capture.is_some() || self.adding_hotkeys.is_some() {
                        self.adding_capture = None;
                        self.adding_hotkeys = None;
                    } else {
                        self.close_preferences();
                    }
                }
                if ui.input(|i| i.viewport().focused == Some(true)) {
                    self.settings_last_active = true;
                }
                if focus_settings {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
                }
            ui.painter().rect_filled(ui.max_rect(), 0.0, BACKGROUND);
            let mut cancel = false;
            egui::Panel::bottom("settings_actions").exact_size(68.).frame(egui::Frame::NONE.fill(SURFACE).inner_margin(egui::Margin::symmetric(20, 16))).show(ui, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(language.text("Save")).fill(ACCENT)).clicked() {
                        if let Err(error) = self.save_preferences(ui.ctx()) {
                            self.status = error;
                        }
                    }
                    if ui.button(language.text("Cancel")).clicked(){cancel=true;}
                    if self.draft != self.settings { ui.colored_label(MUTED, language.text("Unsaved changes")); }
                });
            });
            egui::Panel::left("settings_navigation").exact_size(210.).resizable(false)
                .frame(egui::Frame::NONE.fill(SURFACE).inner_margin(18)).show(ui, |ui| {
                    ui.heading(language.text("Settings"));
                    ui.colored_label(MUTED, "RustClip");
                    ui.add_space(22.);
                    settings_navigation(ui, &mut self.settings_page,
                        focus_settings && self.hotkey_recorder.is_none() && !self.confirm_discard, language);
                });
            egui::CentralPanel::default().frame(egui::Frame::NONE.inner_margin(26)).show(ui, |ui| {
                ui.heading(language.text(SETTINGS_PAGES[self.settings_page]));
                ui.colored_label(MUTED, language.text(SETTINGS_HINTS[self.settings_page]));
                ui.add_space(16.);
                if !self.status.is_empty() { ui.colored_label(egui::Color32::LIGHT_RED, language.message(&self.status)); }
                egui::ScrollArea::vertical().id_salt(("settings_content", self.settings_page)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::Frame::NONE.fill(SURFACE).corner_radius(10).inner_margin(18).show(ui, |ui| {
                ui.set_width(ui.available_width());
                match self.settings_page {
                    0=>{
                        ui.label(language.text("Interface language"));
                        egui::ComboBox::from_id_salt("interface_language")
                            .selected_text(self.draft.language.name())
                            .show_ui(ui, |ui| {
                                for option in crate::i18n::Language::ALL {
                                    ui.selectable_value(&mut self.draft.language, option, option.name());
                                }
                            });
                        ui.checkbox(&mut self.draft.autostart,language.text("Start when Windows starts"));ui.checkbox(&mut self.draft.monitor,language.text("Monitor clipboard"));ui.checkbox(&mut self.draft.hide_after,language.text("Hide after copying or pasting"));ui.label(language.text("Closing the window minimizes RustClip to the tray."));ui.label(format!("{} {}", language.text("Data:"), model::directory().display()));},
                    1=>{
                        ui.label(language.text("Global shortcuts (Ctrl, Shift, Alt, Win + key):"));
                        egui::Grid::new("hotkey_grid").num_columns(3).show(ui, |ui| {
                            for (index, (name, value)) in [
                                (language.text("Open history"), &mut self.draft.hotkey_open),
                                (language.text("Previous clip"), &mut self.draft.hotkey_previous),
                                (language.text("Next clip"), &mut self.draft.hotkey_next),
                                (language.text("Paste as text"), &mut self.draft.hotkey_plain_paste),
                            ].into_iter().enumerate() {
                                ui.label(name);
                                ui.label(value.as_str());
                                if ui.button("…").on_hover_text(language.text("Record shortcut")).clicked() {
                                    match hotkeys::Recorder::new(ui.ctx()) {
                                        Ok(recorder) => self.hotkey_recorder = Some((index, recorder)),
                                        Err(error) => self.status = error,
                                    }
                                }
                                ui.end_row();
                            }
                        });
                        ui.label(language.text("Click “…” to record a shortcut. Changes take effect after saving settings."));
                        if let Err(error) = hotkeys::validate(&self.draft) {
                            ui.colored_label(egui::Color32::RED, language.message(&error));
                        }
                        ui.checkbox(&mut self.draft.auto_paste,language.text("Paste automatically after Quick Copy"));
                    },
                    2=>{ui.label(language.text("Maximum number of unstarred clips"));ui.add(egui::DragValue::new(&mut self.draft.limit).speed(1.).range(1..=usize::MAX));ui.label(language.text("Check recent clips for duplicates"));ui.add(egui::DragValue::new(&mut self.draft.duplicates).speed(1.).range(0..=100));ui.checkbox(&mut self.draft.newest_first,language.text("Newest clips first"));},
                    3=>{ui.checkbox(&mut self.draft.ctrl_v,language.text("Emulate Ctrl+V (otherwise Shift+Insert)"));ui.checkbox(&mut self.draft.restore,language.text("Restore clipboard after pasting"));ui.label(language.text("Compatibility delay, ms"));ui.add(egui::DragValue::new(&mut self.draft.paste_delay).speed(1.).range(20..=2000));},
                    4=>{ui.label(language.text("Central preview timeout, ms (0 = no timeout)"));ui.add(egui::DragValue::new(&mut self.draft.preview_ms).speed(1.).range(0..=60000));ui.label(language.text("Maximum text size, KiB"));ui.add(egui::DragValue::new(&mut self.draft.max_text_kb).speed(1.).range(1..=32768));ui.label(language.text("Esc hides the central preview."));},
                    5=>{
                        ui.label(language.text("Application name (with or without .exe) or full path."));
                        app_list(ui, language, language.text("Do not capture from applications:"), &mut self.draft.excluded_capture, &mut self.adding_capture, &self.settings.recent_apps, &mut self.app_icons);
                        ui.separator();
                        app_list(ui, language, language.text("Ignore hotkeys in applications:"), &mut self.draft.excluded_hotkeys, &mut self.adding_hotkeys, &self.settings.recent_apps, &mut self.app_icons);
                        ui.separator();
                        ui.label(language.text("Additional excluded format names/IDs:"));
                        ui.text_edit_multiline(&mut self.draft.excluded_formats);
                        ui.label(language.text("Common problematic formats are excluded by default; only supported formats are read."));
                    }
                    _=>self.setup_secrets_ui(ui),
                } });
            });
            });
            if cancel {
                self.close_preferences();
            } else if ui.input(|i| i.viewport().close_requested()) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
                if self.hotkey_recorder.take().is_none() && !self.confirm_discard {
                    self.close_preferences();
                }
            }
            self.record_hotkey(ui.ctx());
            if self.confirm_discard {
                egui::Modal::new(egui::Id::new("discard_settings")).show(ui.ctx(), |ui| {
                    ui.heading(language.text("Close settings?"));
                    ui.label(language.text("Unsaved changes will be discarded."));
                    ui.horizontal(|ui| {
                        if ui.button(language.text("Discard and close")).clicked() {
                            self.draft = self.settings.clone();
                            self.settings_open = false;
                            self.confirm_discard = false;
                        }
                        if ui.button(language.text("Keep editing")).clicked() {
                            self.confirm_discard = false;
                        }
                    });
                });
            }
            },
        );
        if !self.settings_open {
            self.hotkey_recorder = None;
        }
    }

    fn record_hotkey(&mut self, ctx: &egui::Context) {
        let language = self.draft.language;
        let Some((index, recorder)) = &mut self.hotkey_recorder else {
            return;
        };
        let value = recorder.value().to_owned();
        if value == "Esc" {
            self.hotkey_recorder = None;
            return;
        }
        let mut draft = self.draft.clone();
        *[
            &mut draft.hotkey_open,
            &mut draft.hotkey_previous,
            &mut draft.hotkey_next,
            &mut draft.hotkey_plain_paste,
        ][*index] = value.clone();
        let validation = hotkeys::validate(&draft);
        let mut close = false;
        egui::Modal::new(egui::Id::new("record_hotkey")).show(ctx, |ui| {
            ui.set_min_width(380.);
            ui.heading(language.text("Record hotkey"));
            ui.label(language.text("Press a shortcut with Ctrl, Shift, Alt, or Win."));
            ui.add_space(12.);
            ui.horizontal_wrapped(|ui| {
                if value.is_empty() {
                    ui.label(language.text("Waiting for keys…"));
                } else {
                    for (index, key) in value.split('+').enumerate() {
                        if index > 0 {
                            ui.label("+");
                        }
                        ui.add(
                            egui::Button::new(egui::RichText::new(key).size(22.))
                                .sense(egui::Sense::hover()),
                        );
                    }
                }
            });
            if !value.is_empty()
                && let Err(error) = &validation
            {
                ui.colored_label(egui::Color32::LIGHT_RED, language.message(error));
            }
            ui.add_space(12.);
            ui.horizontal(|ui| {
                if ui.button(language.text("Cancel")).clicked() {
                    close = true;
                }
                if ui
                    .add_enabled(
                        validation.is_ok(),
                        egui::Button::new(language.text("Save")).fill(ACCENT),
                    )
                    .clicked()
                {
                    self.draft = draft;
                    close = true;
                }
            });
        });
        if close {
            self.hotkey_recorder = None;
        }
    }
}
