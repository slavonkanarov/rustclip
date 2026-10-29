use super::{
    App,
    widgets::{SURFACE, secret_app_picker, secret_button},
};
use crate::{
    clipboard::{self, Command},
    model, secrets,
};
use eframe::egui;
use zeroize::Zeroize;

#[cfg(test)]
mod tests;

fn has_secret_for_app(vault: &secrets::VaultData, app: &str) -> bool {
    !app.is_empty()
        && vault
            .iter()
            .any(|secret| clipboard::app_matches(app, vault.app_for(secret)))
}

fn navigate_secrets(
    indices: &[usize],
    selected: Option<usize>,
    key: Option<egui::Key>,
) -> Option<usize> {
    let Some(row) = indices.iter().position(|&i| Some(i) == selected) else {
        return indices.first().copied();
    };
    indices
        .get(match key {
            Some(egui::Key::ArrowUp) => row.saturating_sub(1),
            Some(egui::Key::ArrowDown) => (row + 1).min(indices.len() - 1),
            _ => row,
        })
        .copied()
}

impl App {
    pub(super) fn select_first_folder(&mut self) {
        if let Some(vault) = &self.vault {
            if !self
                .selected_folder
                .as_ref()
                .is_some_and(|name| vault.folders.iter().any(|folder| folder.name == *name))
            {
                self.selected_folder = vault.folders.first().map(|folder| folder.name.clone());
            }
        }
    }
    fn save_secrets(&mut self) {
        if let (Some(vault), Some(key)) = (&self.vault, &self.credential) {
            if let Err(e) = secrets::save_session(key, self.pin_mode, vault) {
                self.status = e;
            }
        }
    }
    fn migrate_templates(&mut self) {
        let templates: Vec<_> = self
            .clips
            .iter()
            .filter(|clip| clip.snippet)
            .cloned()
            .collect();
        if templates.is_empty() {
            return;
        }
        let vault = self.vault.as_mut().unwrap();
        let folder = vault.import_folder();
        vault.extend(templates.iter().map(|clip| secrets::Secret {
            name: if clip.label.is_empty() {
                clip.title()
            } else {
                clip.label.clone()
            },
            value: clip.text.clone(),
            app: String::new(),
            folder: folder.clone(),
            hidden: true,
        }));
        if let Err(e) =
            secrets::save_session(self.credential.as_ref().unwrap(), self.pin_mode, vault)
        {
            let original_len = vault.len() - templates.len();
            vault.truncate(original_len);
            self.status = e;
            return;
        }
        for clip in templates {
            let path = model::directory()
                .join("clips")
                .join(format!("{}.json", clip.id));
            if let Err(e) = std::fs::remove_file(path) {
                self.status = e.to_string();
            }
        }
        self.clips.retain(|clip| !clip.snippet);
        self.selected = self.clips.len().checked_sub(1);
    }
    pub(super) fn lock_secrets(&mut self) {
        self.clear_secret_view();
        self.snippets = false;
        self.credential_input.zeroize();
        self.credential_confirm.zeroize();
        self.unlock_input.zeroize();
        self.secret_selected = None;
        self.secret_copied = None;
        self.selected_folder = None;
        self.folder_edit_original = None;
    }
    pub(super) fn ask_unlock(&mut self) {
        self.snippets = false;
        self.unlock_open = true;
        self.focus_unlock = true;
        self.unlock_input.zeroize();
        self.status.clear();
        if let Ok(pin) = secrets::mode() {
            self.pin_mode = pin;
        }
    }
    pub(super) fn route_app(&mut self) {
        self.snippets = self
            .vault
            .as_ref()
            .is_some_and(|vault| has_secret_for_app(vault, &self.pending_app));
        if self.snippets {
            self.selected_folder = None;
            self.secret_app_filter = self
                .pending_app
                .rsplit('\\')
                .next()
                .unwrap_or(&self.pending_app)
                .into();
            self.filter_active_app = true;
        }
    }
    pub(super) fn unlock_dialog(&mut self, ctx: &egui::Context) {
        if !self.unlock_open {
            return;
        }
        let mut submit = false;
        let mut cancel = false;
        egui::Modal::new(egui::Id::new("rustclip-unlock")).show(ctx, |ui| {
            ui.set_min_width(340.);
            ui.heading("Разблокировать секреты");
            ui.label(if self.pin_mode {
                "Введите PIN"
            } else {
                "Введите пароль"
            });
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.unlock_input)
                    .password(true)
                    .desired_width(f32::INFINITY),
            );
            if self.focus_unlock {
                response.request_focus();
                self.focus_unlock = false;
            }
            submit = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if !self.status.is_empty() {
                ui.colored_label(egui::Color32::RED, &self.status);
            }
            ui.horizontal(|ui| {
                submit |= ui.button("Разблокировать").clicked();
                cancel = ui.button("Продолжить без PIN").clicked();
            });
        });
        if cancel {
            self.cancel_unlock();
        } else if submit {
            match secrets::unlock_session(&self.unlock_input) {
                Ok((vault, key)) => {
                    self.vault = Some(vault);
                    self.credential = Some(key);
                    self.unlock_input.zeroize();
                    self.status.clear();
                    self.unlock_open = false;
                    self.migrate_templates();
                    self.snippets = true;
                    self.select_first_folder();
                }
                Err(e) => {
                    self.status = e;
                    self.unlock_input.zeroize();
                    self.focus_unlock = true;
                }
            }
        }
    }
    pub(super) fn cancel_unlock(&mut self) {
        self.unlock_open = false;
        self.snippets = false;
        self.unlock_input.zeroize();
        self.status.clear();
    }
    fn clear_secret_view(&mut self) {
        self.credential = None;
        if let Some(mut vault) = self.vault.take() {
            for secret in &mut vault.secrets {
                secret.clear();
            }
            for folder in &mut vault.folders {
                folder.name.zeroize();
                folder.app.zeroize();
            }
        }
        if let Some(mut secret) = self.secret_editor.take() {
            secret.clear();
        }
        if let Some(mut folder) = self.folder_editor.take() {
            folder.name.zeroize();
            folder.app.zeroize();
        }
    }
    fn secret_action(&mut self, ctx: &egui::Context, paste: bool) {
        if let Some((i, secret)) = self.vault.as_ref().and_then(|v| {
            self.secret_selected
                .and_then(|i| v.get(i).map(|secret| (i, secret)))
        }) {
            self.send(Command::Secret(
                clipboard::text_clip(secret.value.clone()),
                paste,
                i,
            ));
            if paste && self.settings.hide_after {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }
    }
    pub(super) fn setup_secrets_ui(&mut self, ui: &mut egui::Ui) {
        if secrets::exists() {
            ui.label("Шифрование секретов настроено.");
            ui.label(if self.pin_mode {
                "Способ защиты: PIN"
            } else {
                "Способ защиты: пароль"
            });
            if ui.button("Сбросить разблокировку").clicked() {
                self.lock_secrets();
            }
            return;
        }
        ui.label("Создать зашифрованное хранилище секретов");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.pin_mode, true, "PIN");
            ui.radio_value(&mut self.pin_mode, false, "Пароль");
        });
        ui.label(if self.pin_mode {
            "PIN (минимум 4 цифры)"
        } else {
            "Пароль"
        });
        ui.add(egui::TextEdit::singleline(&mut self.credential_input).password(true));
        ui.label("Повторите PIN или пароль");
        ui.add(egui::TextEdit::singleline(&mut self.credential_confirm).password(true));
        if ui.button("Создать хранилище").clicked() {
            if self.credential_input != self.credential_confirm {
                self.status = "PIN или пароль не совпадают".into();
            } else if self.credential_input.is_empty()
                || self.pin_mode
                    && (!self.credential_input.bytes().all(|b| b.is_ascii_digit())
                        || self.credential_input.len() < 4)
            {
                self.status = "PIN: минимум 4 цифры; пароль не должен быть пустым".into();
            } else {
                match secrets::save(
                    &self.credential_input,
                    self.pin_mode,
                    &secrets::VaultData::default(),
                ) {
                    Ok(()) => {
                        let session = secrets::unlock_session(&self.credential_input);
                        self.credential_input.zeroize();
                        self.credential_confirm.zeroize();
                        match session {
                            Ok((vault, key)) => {
                                self.credential = Some(key);
                                self.vault = Some(vault);
                                self.status.clear();
                                self.migrate_templates();
                                self.clear_secret_view();
                            }
                            Err(e) => self.status = e,
                        }
                    }
                    Err(e) => self.status = e,
                }
            }
        }
    }
    pub(super) fn secrets_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if !secrets::exists() {
            ui.colored_label(egui::Color32::YELLOW, "Защита секретов не настроена. Добавьте PIN или пароль в Настройках → Секреты. До настройки секреты не сохраняются.");
            if ui.button("Открыть настройки секретов").clicked() {
                self.settings_page = 6;
                self.open_preferences();
            }
            return;
        }
        if self.vault.is_none() {
            return;
        }
        ui.horizontal(|ui| {
            ui.heading("Секреты");
            if ui
                .add_enabled(
                    self.selected_folder.is_some()
                        && !self.filter_active_app
                        && self.secret_app_filter.is_empty(),
                    egui::Button::new("+ Новый секрет").fill(egui::Color32::from_rgb(38, 120, 155)),
                )
                .clicked()
            {
                self.status.clear();
                self.secret_selected = None;
                self.secret_editor = Some(secrets::Secret {
                    name: String::new(),
                    value: String::new(),
                    app: String::new(),
                    folder: self.selected_folder.clone().unwrap_or_default(),
                    hidden: true,
                });
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Заблокировать").clicked() {
                    self.lock_secrets();
                }
            });
        });
        if self.vault.is_none() {
            return;
        }
        let mut paste = false;
        let mut copy = false;
        let mut edit = false;
        let mut delete = false;
        let mut toggle_visibility = None;
        egui::Panel::left("secret_folders")
            .exact_size(190.)
            .show(ui, |ui| {
                ui.heading("Папки");
                if ui.button("+ Новая папка").clicked() {
                    self.status.clear();
                    self.folder_edit_original = None;
                    self.folder_editor = Some(secrets::Folder {
                        name: String::new(),
                        app: String::new(),
                    });
                }
                ui.separator();
                let mut delete_folder = None;
                ui.vertical(|ui| {
                    for folder in &self.vault.as_ref().unwrap().folders {
                        ui.push_id(&folder.name, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.;
                                if ui
                                    .add_sized(
                                        [(ui.available_width() - 68.).max(1.), 34.],
                                        egui::Button::new(format!("📁 {}", folder.name))
                                            .selected(
                                                self.selected_folder.as_ref() == Some(&folder.name),
                                            )
                                            .truncate(),
                                    )
                                    .on_hover_text(&folder.name)
                                    .clicked()
                                {
                                    self.selected_folder = Some(folder.name.clone());
                                    self.filter_active_app = false;
                                    self.secret_app_filter.clear();
                                }
                                if secret_button(ui, "edit", "Изменить папку").clicked()
                                {
                                    self.folder_edit_original = Some(folder.name.clone());
                                    self.folder_editor = Some(folder.clone());
                                }
                                if secret_button(ui, "delete", "Удалить папку").clicked()
                                {
                                    delete_folder = Some(folder.name.clone());
                                }
                            });
                        });
                    }
                });
                if let Some(name) = delete_folder {
                    if self
                        .vault
                        .as_ref()
                        .unwrap()
                        .iter()
                        .any(|secret| secret.folder == name)
                    {
                        self.status = "Сначала переместите или удалите секреты из папки".into();
                    } else {
                        self.vault
                            .as_mut()
                            .unwrap()
                            .folders
                            .retain(|folder| folder.name != name);
                        if self.selected_folder.as_ref() == Some(&name) {
                            self.selected_folder = self
                                .vault
                                .as_ref()
                                .unwrap()
                                .folders
                                .first()
                                .map(|folder| folder.name.clone());
                        }
                        self.save_secrets();
                    }
                }
            });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.separator();
            if self
                .secret_copied
                .is_some_and(|(_, at)| at.elapsed().as_secs() >= 3)
            {
                self.secret_copied = None;
            }
            ui.heading(
                if self.filter_active_app || !self.secret_app_filter.is_empty() {
                    "Поиск по приложению"
                } else {
                    self.selected_folder.as_deref().unwrap_or("Создайте папку")
                },
            );
            ui.horizontal_wrapped(|ui| {
                let search = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("Поиск по названию · Ctrl+F"),
                );
                if self.secret_editor.is_none()
                    && self.folder_editor.is_none()
                    && !egui::Popup::is_any_open(ctx)
                    && ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::F))
                {
                    search.request_focus();
                }
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.secret_app_filter)
                            .hint_text("Поиск по приложению"),
                    )
                    .changed()
                {
                    self.filter_active_app = false;
                }
                if ui.button("Очистить").clicked() {
                    self.query.clear();
                    self.secret_app_filter.clear();
                    self.filter_active_app = false;
                    self.select_first_folder();
                }
            });
            let query = self.query.to_lowercase();
            let app_filter = self.secret_app_filter.to_lowercase();
            let vault = self.vault.as_ref().unwrap();
            let indices: Vec<_> = vault
                .iter()
                .enumerate()
                .filter(|(_, secret)| {
                    (self.filter_active_app
                        || !app_filter.is_empty()
                        || self.selected_folder.as_ref() == Some(&secret.folder))
                        && secret.name.to_lowercase().contains(&query)
                        && if self.filter_active_app {
                            clipboard::app_matches(&self.pending_app, vault.app_for(secret))
                        } else {
                            vault.app_for(secret).to_lowercase().contains(&app_filter)
                        }
                })
                .map(|(i, _)| i)
                .collect();
            let key = if !self.settings_open
                && !self.unlock_open
                && self.editor.is_none()
                && self.secret_editor.is_none()
                && self.folder_editor.is_none()
                && !egui::Popup::is_any_open(ctx)
                && !ctx.text_edit_focused()
            {
                ctx.input_mut(|input| {
                    [egui::Key::ArrowUp, egui::Key::ArrowDown]
                        .into_iter()
                        .find(|&key| input.consume_key(egui::Modifiers::NONE, key))
                })
            } else {
                None
            };
            if key.is_some() {
                ctx.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
            }
            if self.secret_editor.is_none() {
                self.secret_selected = navigate_secrets(&indices, self.secret_selected, key);
            }
            egui::ScrollArea::vertical()
                .max_height(ui.available_height().max(250.))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        for &i in &indices {
                            let secret = &vault[i];
                            let card = egui::Frame::group(ui.style())
                                .corner_radius(egui::CornerRadius::same(8))
                                .inner_margin(egui::Margin::symmetric(10, 6))
                                .fill(if self.secret_selected == Some(i) {
                                    egui::Color32::from_rgb(32, 66, 88)
                                } else {
                                    SURFACE
                                })
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width().min(700.));
                                    ui.horizontal(|ui| {
                                        let text_width = (ui.available_width() - 155.).max(100.);
                                        let text = ui.vertical(|ui| {
                                            ui.set_width(text_width);
                                            ui.add(
                                                egui::Label::new(
                                                    egui::RichText::new(&secret.name).strong(),
                                                )
                                                .truncate(),
                                            );
                                            ui.add(
                                                egui::Label::new(if secret.hidden {
                                                    "••••••••".into()
                                                } else {
                                                    secret.value.replace(['\r', '\n'], " ")
                                                })
                                                .truncate(),
                                            );
                                        });
                                        let response = text.response.interact(egui::Sense::click());
                                        if response.clicked() {
                                            self.secret_selected = Some(i);
                                        }
                                        if response.double_clicked() {
                                            self.secret_selected = Some(i);
                                            paste = true;
                                        }
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.spacing_mut().item_spacing.x = 4.;
                                                let copied = self.secret_copied.is_some_and(
                                                    |(index, at)| {
                                                        index == i && at.elapsed().as_secs() < 3
                                                    },
                                                );
                                                if secret_button(ui, "delete", "Удалить секрет")
                                                    .clicked()
                                                {
                                                    self.secret_selected = Some(i);
                                                    delete = true;
                                                }
                                                if secret_button(ui, "edit", "Изменить секрет")
                                                    .clicked()
                                                {
                                                    self.secret_selected = Some(i);
                                                    edit = true;
                                                }
                                                if secret_button(
                                                    ui,
                                                    if secret.hidden { "show" } else { "hide" },
                                                    if secret.hidden {
                                                        "Показать значение"
                                                    } else {
                                                        "Скрыть значение"
                                                    },
                                                )
                                                .clicked()
                                                {
                                                    toggle_visibility = Some(i);
                                                }
                                                if secret_button(
                                                    ui,
                                                    if copied { "check" } else { "copy" },
                                                    if copied {
                                                        "Скопировано в буфер обмена"
                                                    } else {
                                                        "Скопировать значение"
                                                    },
                                                )
                                                .clicked()
                                                {
                                                    self.secret_selected = Some(i);
                                                    copy = true;
                                                }
                                            },
                                        );
                                    });
                                });
                            if key.is_some() && self.secret_selected == Some(i) {
                                card.response.scroll_to_me(None);
                            }
                        }
                    });
                });
            if indices.is_empty() {
                ui.label(if self.vault.as_ref().unwrap().folders.is_empty() {
                    "Создайте первую папку слева, затем добавьте секрет."
                } else if self.vault.as_ref().unwrap().is_empty() {
                    "В папке пока нет секретов. Нажмите «+ Новый секрет»."
                } else {
                    "В этой папке или по этому запросу секретов нет."
                });
            }
        });
        if paste {
            self.secret_action(ctx, true);
        }
        if copy {
            self.secret_action(ctx, false);
        }
        if let Some(i) = toggle_visibility {
            if let Some(secret) = self.vault.as_mut().and_then(|vault| vault.get_mut(i)) {
                secret.hidden = !secret.hidden;
                self.save_secrets();
            }
        }
        if edit {
            self.secret_editor = self
                .vault
                .as_ref()
                .and_then(|v| self.secret_selected.and_then(|i| v.get(i)))
                .cloned();
        }
        if delete {
            if let Some(i) = self.secret_selected.take() {
                if i < self.vault.as_ref().unwrap().len() {
                    self.vault.as_mut().unwrap().remove(i).clear();
                    self.secret_copied = None;
                    self.save_secrets();
                }
            }
        }
        if self.folder_editor.is_some() {
            egui::Modal::new(egui::Id::new("folder_editor")).show(ctx, |ui| {
                ui.set_min_width(400.);
                if let Some(folder) = &mut self.folder_editor {
                    ui.heading(if self.folder_edit_original.is_some() {
                        "Изменить папку"
                    } else {
                        "Новая папка"
                    });
                    ui.label("Название папки");
                    ui.add(
                        egui::TextEdit::singleline(&mut folder.name).desired_width(f32::INFINITY),
                    );
                    secret_app_picker(
                        ui,
                        &mut folder.app,
                        &self.settings.recent_apps,
                        &mut self.app_icons,
                    );
                    ui.separator();
                    let mut save = false;
                    ui.horizontal(|ui| {
                        save = ui
                            .add(
                                egui::Button::new("Сохранить папку")
                                    .fill(egui::Color32::from_rgb(38, 120, 155)),
                            )
                            .on_hover_text("Ctrl+Enter")
                            .clicked()
                            || super::widgets::save_requested(ui.ctx());
                        if ui.button("Отмена").clicked() {
                            self.folder_editor = None;
                            self.folder_edit_original = None;
                        }
                    });
                    if save {
                        let folder = self.folder_editor.take().unwrap();
                        if folder.name.trim().is_empty() {
                            self.status = "Введите название папки".into();
                            self.folder_editor = Some(folder);
                        } else if self.vault.as_ref().unwrap().folders.iter().any(|existing| {
                            existing.name.to_lowercase() == folder.name.to_lowercase()
                                && self.folder_edit_original.as_deref() != Some(&existing.name)
                        }) {
                            self.status = "Папка с таким названием уже существует".into();
                            self.folder_editor = Some(folder);
                        } else {
                            let vault = self.vault.as_mut().unwrap();
                            if let Some(old) = self.folder_edit_original.take() {
                                if let Some(existing) = vault
                                    .folders
                                    .iter_mut()
                                    .find(|existing| existing.name == old)
                                {
                                    *existing = folder.clone();
                                }
                                for secret in &mut vault.secrets {
                                    if secret.folder == old {
                                        secret.folder = folder.name.clone();
                                    }
                                }
                            } else {
                                vault.folders.push(folder.clone());
                            }
                            self.selected_folder = Some(folder.name);
                            self.status.clear();
                            self.save_secrets();
                        }
                    }
                    if !self.status.is_empty() {
                        ui.colored_label(egui::Color32::RED, &self.status);
                    }
                }
            });
        }
        if self.secret_editor.is_some() {
            egui::Modal::new(egui::Id::new("secret_editor")).show(ctx, |ui| {
                ui.set_min_width(420.);
                ui.heading("Секрет");
                if let Some(secret) = &mut self.secret_editor {
                    ui.label("Название");
                    ui.add(
                        egui::TextEdit::singleline(&mut secret.name).desired_width(f32::INFINITY),
                    );
                    ui.label("Значение");
                    ui.checkbox(&mut secret.hidden, "Скрывать значение на карточке");
                    ui.add(
                        egui::TextEdit::multiline(&mut secret.value)
                            .password(secret.hidden)
                            .desired_rows(5)
                            .desired_width(f32::INFINITY),
                    );
                    egui::ComboBox::from_label("Папка")
                        .selected_text(&secret.folder)
                        .show_ui(ui, |ui| {
                            for folder in &self.vault.as_ref().unwrap().folders {
                                ui.selectable_value(
                                    &mut secret.folder,
                                    folder.name.clone(),
                                    &folder.name,
                                );
                            }
                        });
                    secret_app_picker(
                        ui,
                        &mut secret.app,
                        &self.settings.recent_apps,
                        &mut self.app_icons,
                    );
                    if secret.app.is_empty() {
                        if let Some(folder) =
                            self.vault.as_ref().unwrap().folders.iter().find(|folder| {
                                folder.name == secret.folder && !folder.app.is_empty()
                            })
                        {
                            ui.small(format!("Используется приложение папки: {}", folder.app));
                        }
                    }
                    ui.separator();
                    let mut save = false;
                    ui.horizontal(|ui| {
                        save = ui
                            .add(
                                egui::Button::new("Сохранить секрет")
                                    .fill(egui::Color32::from_rgb(38, 120, 155)),
                            )
                            .on_hover_text("Ctrl+Enter")
                            .clicked()
                            || super::widgets::save_requested(ui.ctx());
                        if ui.button("Отмена").clicked() {
                            if let Some(mut secret) = self.secret_editor.take() {
                                secret.clear();
                            }
                        }
                    });
                    if save {
                        let secret = self.secret_editor.take().unwrap();
                        if secret.name.trim().is_empty() {
                            self.status = "Введите название секрета".into();
                            self.secret_editor = Some(secret);
                        } else if !self
                            .vault
                            .as_ref()
                            .unwrap()
                            .folders
                            .iter()
                            .any(|folder| folder.name == secret.folder)
                        {
                            self.status = "Выберите папку для секрета".into();
                            self.secret_editor = Some(secret);
                        } else if self.vault.as_ref().unwrap().name_in_folder(
                            &secret.name,
                            &secret.folder,
                            self.secret_selected,
                        ) {
                            self.status = "В этой папке уже есть секрет с таким названием".into();
                            self.secret_editor = Some(secret);
                        } else {
                            let vault = self.vault.as_mut().unwrap();
                            if let Some(i) = self.secret_selected.filter(|&i| i < vault.len()) {
                                std::mem::replace(&mut vault[i], secret).clear();
                            } else {
                                vault.push(secret);
                                self.secret_selected = Some(vault.len() - 1);
                            }
                            self.status.clear();
                            self.save_secrets();
                        }
                    }
                    if !self.status.is_empty() {
                        ui.colored_label(egui::Color32::RED, &self.status);
                    }
                }
            });
        }
    }
}
