use super::{
    App,
    widgets::{ACCENT, BACKGROUND, MUTED, SURFACE, app_list},
};
use crate::{clipboard::Command, hotkeys, model};
use eframe::egui;
use zeroize::Zeroize;

const SETTINGS_PAGES: [&str; 7] = [
    "Общее",
    "Горячие клавиши",
    "История",
    "Вставка",
    "Дополнительно",
    "Фильтрация",
    "Секреты",
];
const SETTINGS_HINTS: [&str; 7] = [
    "Сбор клипов и поведение окна приложения.",
    "Быстрый доступ к истории и вставке из любого приложения.",
    "Порядок, объём хранения и повторяющиеся клипы.",
    "Поведение буфера обмена при вставке в другие приложения.",
    "Центральное превью и ограничения размера текста.",
    "Выберите приложения и форматы, которые нужно пропускать.",
    "Защита и доступ к зашифрованному хранилищу.",
];

impl App {
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
        let focus_settings = self.focus_settings == 1;
        self.focus_settings = self.focus_settings.saturating_sub(1);
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustclip-settings"),
            egui::ViewportBuilder::default()
                .with_title("Настройки — RustClip")
                .with_inner_size([920., 660.])
                .with_min_inner_size([760., 500.]),
            |ui, _| {
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
                    if ui.add(egui::Button::new("Сохранить").fill(ACCENT)).clicked(){if let Err(e)=hotkeys::validate(&self.draft){self.status=e;}else{self.draft.recent_apps=self.settings.recent_apps.clone();self.settings=self.draft.clone();self.send(Command::Settings(self.settings.clone()));if let Err(e)=model::write_json(&model::directory().join("settings.json"),&self.settings){self.status=e;}else{self.settings_open=false;}}}
                    if ui.button("Отмена").clicked(){cancel=true;}
                    if self.draft != self.settings { ui.colored_label(MUTED, "Есть несохранённые изменения"); }
                });
            });
            egui::Panel::left("settings_navigation").exact_size(210.).resizable(false)
                .frame(egui::Frame::NONE.fill(SURFACE).inner_margin(18)).show(ui, |ui| {
                    ui.heading("Настройки");
                    ui.colored_label(MUTED, "RustClip");
                    ui.add_space(22.);
                    for (i, name) in SETTINGS_PAGES.iter().enumerate() {
                        if ui.add_sized([ui.available_width(), 38.], egui::Button::new(*name).selected(self.settings_page == i)).clicked() {
                            self.settings_page = i;
                        }
                    }
                });
            egui::CentralPanel::default().frame(egui::Frame::NONE.inner_margin(26)).show(ui, |ui| {
                ui.heading(SETTINGS_PAGES[self.settings_page]);
                ui.colored_label(MUTED, SETTINGS_HINTS[self.settings_page]);
                ui.add_space(16.);
                if !self.status.is_empty() { ui.colored_label(egui::Color32::LIGHT_RED, &self.status); }
                egui::ScrollArea::vertical().id_salt(("settings_content", self.settings_page)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::Frame::NONE.fill(SURFACE).corner_radius(10).inner_margin(18).show(ui, |ui| {
                ui.set_width(ui.available_width());
                match self.settings_page {
                    0=>{ui.checkbox(&mut self.draft.monitor,"Следить за буфером обмена");ui.checkbox(&mut self.draft.hide_after,"Спрятаться после копирования/вставки");ui.label("Закрытие окна сворачивает RustClip в трей.");ui.label(format!("Данные: {}",model::directory().display()));},
                    1=>{
                        ui.label("Глобальные сочетания (Ctrl, Shift, Alt, Win + клавиша):");
                        egui::Grid::new("hotkey_grid").num_columns(3).show(ui, |ui| {
                            for (index, (name, value)) in [
                                ("Открыть историю", &mut self.draft.hotkey_open),
                                ("Предыдущий клип", &mut self.draft.hotkey_previous),
                                ("Следующий клип", &mut self.draft.hotkey_next),
                                ("Вставить как текст", &mut self.draft.hotkey_plain_paste),
                            ].into_iter().enumerate() {
                                ui.label(name);
                                ui.label(value.as_str());
                                if ui.button("…").on_hover_text("Записать сочетание клавиш").clicked() {
                                    match hotkeys::Recorder::new(ui.ctx()) {
                                        Ok(recorder) => self.hotkey_recorder = Some((index, recorder)),
                                        Err(error) => self.status = error,
                                    }
                                }
                                ui.end_row();
                            }
                        });
                        ui.label("Нажмите «…», чтобы записать сочетание. Изменения применяются после сохранения настроек.");
                        if let Err(error) = hotkeys::validate(&self.draft) {
                            ui.colored_label(egui::Color32::RED, error);
                        }
                        ui.checkbox(&mut self.draft.auto_paste,"Автоматически вставлять после Quick Copy");
                    },
                    2=>{ui.label("Максимальное количество незакреплённых клипов");ui.add(egui::DragValue::new(&mut self.draft.limit).range(1..=usize::MAX));ui.label("Проверять последние клипы на дубликаты");ui.add(egui::DragValue::new(&mut self.draft.duplicates).range(0..=100));ui.checkbox(&mut self.draft.newest_first,"Новые клипы сверху");},
                    3=>{ui.checkbox(&mut self.draft.ctrl_v,"Эмулировать Ctrl+V (иначе Shift+Insert)");ui.checkbox(&mut self.draft.restore,"Восстанавливать Clipboard после вставки");ui.label("Задержка совместимости, мс");ui.add(egui::DragValue::new(&mut self.draft.paste_delay).range(20..=2000));},
                    4=>{ui.label("Таймер центрального превью, мс (0 — без таймера)");ui.add(egui::DragValue::new(&mut self.draft.preview_ms).range(0..=60000));ui.label("Максимальный текст, KiB");ui.add(egui::DragValue::new(&mut self.draft.max_text_kb).range(1..=32768));ui.label("Esc скрывает центральное превью.");},
                    5=>{
                        ui.label("Имя приложения (с .exe или без) или полный путь.");
                        app_list(ui, "Не захватывать из приложений:", &mut self.draft.excluded_capture, &mut self.adding_capture, &self.settings.recent_apps, &mut self.app_icons);
                        ui.separator();
                        app_list(ui, "Игнорировать горячие клавиши в приложениях:", &mut self.draft.excluded_hotkeys, &mut self.adding_hotkeys, &self.settings.recent_apps, &mut self.app_icons);
                        ui.separator();
                        ui.label("Дополнительные исключённые имена/ID форматов:");
                        ui.text_edit_multiline(&mut self.draft.excluded_formats);
                        ui.label("Базовый список проблемных форматов встроен; читаются только поддерживаемые форматы.");
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
                    ui.heading("Закрыть настройки?");
                    ui.label("Несохранённые изменения будут сброшены.");
                    ui.horizontal(|ui| {
                        if ui.button("Сбросить и закрыть").clicked() {
                            self.draft = self.settings.clone();
                            self.settings_open = false;
                            self.confirm_discard = false;
                        }
                        if ui.button("Продолжить редактирование").clicked() {
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
            ui.heading("Запись горячей клавиши");
            ui.label("Нажмите сочетание с Ctrl, Shift, Alt или Win.");
            ui.add_space(12.);
            ui.horizontal_wrapped(|ui| {
                if value.is_empty() {
                    ui.label("Ожидание нажатия…");
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
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
            ui.add_space(12.);
            ui.horizontal(|ui| {
                if ui.button("Отмена").clicked() {
                    close = true;
                }
                if ui
                    .add_enabled(
                        validation.is_ok(),
                        egui::Button::new("Сохранить").fill(ACCENT),
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
