#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod clipboard;
mod hotkeys;
mod model;
mod secrets;
use clipboard::{Command, Event};
use eframe::egui;
use model::{Clip, Settings};
use std::{collections::HashMap, sync::mpsc};
use zeroize::Zeroize;

struct App {
    clips: Vec<Clip>,
    settings: Settings,
    tx: mpsc::Sender<Command>,
    rx: mpsc::Receiver<Event>,
    selected: Option<usize>,
    query: String,
    snippets: bool,
    vault: Option<secrets::VaultData>,
    credential: String,
    credential_input: String,
    credential_confirm: String,
    pin_mode: bool,
    secret_editor: Option<secrets::Secret>,
    secret_selected: Option<usize>,
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
    draft: Settings,
    adding_capture: Option<String>,
    adding_hotkeys: Option<String>,
    app_icons: HashMap<String, egui::TextureHandle>,
    editor: Option<(Option<usize>, String)>,
    status: String,
    texture: Option<(u128, u32, egui::TextureHandle)>,
    quitting: bool,
}
fn exe_icon(path: &str) -> Option<egui::ColorImage> {
    use windows_sys::Win32::{
        Graphics::Gdi::*,
        UI::{Shell::*, WindowsAndMessaging::*},
    };
    unsafe {
        let mut info: SHFILEINFOW = std::mem::zeroed();
        if SHGetFileInfoW(
            clipboard::wide(path).as_ptr(),
            0,
            &mut info,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_SMALLICON,
        ) == 0
        {
            return None;
        }
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() {
            DestroyIcon(info.hIcon);
            return None;
        }
        let mut bitmap_info: BITMAPINFO = std::mem::zeroed();
        bitmap_info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bitmap_info.bmiHeader.biWidth = 16;
        bitmap_info.bmiHeader.biHeight = -16;
        bitmap_info.bmiHeader.biPlanes = 1;
        bitmap_info.bmiHeader.biBitCount = 32;
        bitmap_info.bmiHeader.biCompression = BI_RGB;
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(
            dc,
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        if bitmap.is_null() {
            DeleteDC(dc);
            DestroyIcon(info.hIcon);
            return None;
        }
        let old = SelectObject(dc, bitmap);
        let drawn = DrawIconEx(
            dc,
            0,
            0,
            info.hIcon,
            16,
            16,
            0,
            std::ptr::null_mut(),
            DI_NORMAL,
        ) != 0;
        let mut rgba = vec![0; 16 * 16 * 4];
        if drawn {
            let pixels = std::slice::from_raw_parts(bits as *const u8, rgba.len());
            for (source, dest) in pixels.chunks_exact(4).zip(rgba.chunks_exact_mut(4)) {
                dest.copy_from_slice(&[source[2], source[1], source[0], source[3]]);
            }
            if rgba.chunks_exact(4).all(|pixel| pixel[3] == 0) {
                for pixel in rgba.chunks_exact_mut(4) {
                    pixel[3] = 255;
                }
            }
        }
        SelectObject(dc, old);
        DeleteObject(bitmap);
        DeleteDC(dc);
        DestroyIcon(info.hIcon);
        drawn.then(|| egui::ColorImage::from_rgba_unmultiplied([16, 16], &rgba))
    }
}

fn app_icon(ui: &mut egui::Ui, path: &str, icons: &mut HashMap<String, egui::TextureHandle>) {
    if !icons.contains_key(path) {
        if let Some(image) = exe_icon(path) {
            icons.insert(
                path.into(),
                ui.ctx().load_texture(path, image, Default::default()),
            );
        }
    }
    if let Some(icon) = icons.get(path) {
        ui.add(egui::Image::new(icon).fit_to_exact_size(egui::vec2(16., 16.)));
    }
}

fn app_list(
    ui: &mut egui::Ui,
    title: &str,
    entries: &mut String,
    adding: &mut Option<String>,
    recent: &[String],
    icons: &mut HashMap<String, egui::TextureHandle>,
) {
    ui.label(title);
    let mut rows: Vec<String> = entries.lines().map(str::to_owned).collect();
    let mut changed = false;
    let mut remove = None;
    for (index, row) in rows.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            if let Some(path) = recent.iter().find(|path| {
                path.rsplit('\\')
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case(row))
                    || path.eq_ignore_ascii_case(row)
            }) {
                app_icon(ui, path, icons);
            }
            changed |= ui
                .add(egui::TextEdit::singleline(row).desired_width(230.))
                .changed();
            if ui.small_button("Удалить").clicked() {
                remove = Some(index);
            }
        });
    }
    if let Some(index) = remove {
        rows.remove(index);
        changed = true;
    }
    if changed {
        *entries = rows.join("\n");
    }
    if adding.is_none() && ui.button("+ Добавить приложение").clicked() {
        *adding = Some(String::new());
    }
    let mut close = false;
    if let Some(value) = adding.as_mut() {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(value)
                    .hint_text("Имя.exe или полный путь")
                    .desired_width(230.),
            );
            if ui.button("Добавить").clicked() && !value.trim().is_empty() {
                let name = value.trim();
                if !entries
                    .lines()
                    .any(|entry| entry.trim().eq_ignore_ascii_case(name))
                {
                    if !entries.is_empty() {
                        entries.push('\n');
                    }
                    entries.push_str(name);
                }
                close = true;
            } else if ui.button("Отмена").clicked() {
                close = true;
            }
        });
        if !close {
            ui.label("Последние активные приложения:");
            for path in recent.iter().take(5) {
                ui.horizontal(|ui| {
                    app_icon(ui, path, icons);
                    let name = path.rsplit('\\').next().unwrap_or(path);
                    if ui.button(name).on_hover_text(path).clicked() {
                        *value = name.into();
                    }
                });
            }
        }
    }
    if close {
        *adding = None;
    }
}
fn secret_app_picker(
    ui: &mut egui::Ui,
    app: &mut String,
    recent: &[String],
    icons: &mut HashMap<String, egui::TextureHandle>,
) {
    ui.horizontal(|ui| {
        ui.label(if app.is_empty() {
            "Приложение: не выбрано"
        } else {
            app.as_str()
        });
        ui.menu_button("Выбрать приложение…", |ui| {
            ui.label("Имя.exe или полный путь");
            ui.text_edit_singleline(app);
            ui.separator();
            ui.label("Последние 5 приложений");
            for path in recent.iter().take(5) {
                ui.horizontal(|ui| {
                    app_icon(ui, path, icons);
                    let name = path.rsplit('\\').next().unwrap_or(path);
                    if ui.button(name).on_hover_text(path).clicked() {
                        *app = name.into();
                        ui.close();
                    }
                });
            }
        });
        if !app.is_empty()
            && ui
                .small_button("×")
                .on_hover_text("Убрать привязку")
                .clicked()
        {
            app.clear();
        }
    });
}
fn has_secret_for_app(vault: &secrets::VaultData, app: &str) -> bool {
    !app.is_empty()
        && vault
            .iter()
            .any(|secret| clipboard::app_matches(app, vault.app_for(secret)))
}
#[cfg(test)]
mod secret_routing_tests {
    use super::*;

    #[test]
    fn hotkey_routes_only_for_matching_app() {
        let vault = secrets::VaultData {
            secrets: vec![secrets::Secret {
                name: "Вход".into(),
                value: "secret".into(),
                app: String::new(),
                folder: "Работа".into(),
                hidden: true,
            }],
            folders: vec![secrets::Folder {
                name: "Работа".into(),
                app: "Code.exe".into(),
            }],
        };
        assert!(has_secret_for_app(&vault, "C:\\Apps\\Code.exe"));
        assert!(!has_secret_for_app(&vault, "C:\\Apps\\Explorer.exe"));
        assert!(!has_secret_for_app(&vault, ""));
    }
}
impl App {
    fn select_first_folder(&mut self) {
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
        if let Some(vault) = &self.vault {
            if let Err(e) = secrets::save(&self.credential, self.pin_mode, vault) {
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
        if let Err(e) = secrets::save(&self.credential, self.pin_mode, vault) {
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
    fn lock_secrets(&mut self) {
        self.clear_secret_view();
        self.credential.zeroize();
        self.credential_input.zeroize();
        self.credential_confirm.zeroize();
        self.secret_selected = None;
        self.selected_folder = None;
        self.folder_edit_original = None;
    }
    fn clear_secret_view(&mut self) {
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
        if let Some(secret) = self
            .vault
            .as_ref()
            .and_then(|v| self.secret_selected.and_then(|i| v.get(i)))
        {
            self.send(Command::Secret(
                clipboard::text_clip(secret.value.clone()),
                paste,
            ));
            if paste && self.settings.hide_after {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }
    }
    fn setup_secrets_ui(&mut self, ui: &mut egui::Ui) {
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
                        self.credential = std::mem::take(&mut self.credential_input);
                        self.credential_confirm.clear();
                        self.vault = Some(secrets::VaultData::default());
                        self.status.clear();
                        self.migrate_templates();
                        self.clear_secret_view();
                    }
                    Err(e) => self.status = e,
                }
            }
        }
    }
    fn secrets_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if !secrets::exists() {
            ui.colored_label(egui::Color32::YELLOW, "Защита секретов не настроена. Добавьте PIN или пароль в Настройках → Секреты. До настройки секреты не сохраняются.");
            if ui.button("Открыть настройки секретов").clicked() {
                self.draft = self.settings.clone();
                self.settings_page = 6;
                self.settings_open = true;
            }
            return;
        }
        if self.vault.is_none() && !self.credential.is_empty() {
            match secrets::unlock(&self.credential) {
                Ok(vault) => {
                    self.vault = Some(vault);
                    self.select_first_folder();
                }
                Err(e) => {
                    self.lock_secrets();
                    self.status = e;
                }
            }
        }
        if self.vault.is_none() {
            ui.heading("Разблокировать секреты");
            if let Ok(pin) = secrets::mode() {
                ui.label(if pin {
                    "Введите PIN"
                } else {
                    "Введите пароль"
                });
            }
            let response =
                ui.add(egui::TextEdit::singleline(&mut self.credential_input).password(true));
            if ui.button("Разблокировать").clicked()
                || response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
            {
                if self.credential_input.is_empty() {
                    self.status = "Введите PIN или пароль".into();
                } else {
                    let result = secrets::unlock(&self.credential_input);
                    match result {
                        Ok(vault) => {
                            self.status.clear();
                            self.vault = Some(vault);
                            self.credential = std::mem::take(&mut self.credential_input);
                            if let Ok(pin) = secrets::mode() {
                                self.pin_mode = pin;
                            }
                            self.migrate_templates();
                            self.select_first_folder();
                        }
                        Err(e) => self.status = e,
                    }
                }
            }
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
                ui.vertical(|ui| {
                    for folder in &self.vault.as_ref().unwrap().folders {
                        if ui
                            .selectable_value(
                                &mut self.selected_folder,
                                Some(folder.name.clone()),
                                format!("📁 {}", folder.name),
                            )
                            .clicked()
                        {
                            self.filter_active_app = false;
                            self.secret_app_filter.clear();
                        }
                    }
                });
                if let Some(name) = self.selected_folder.as_ref().cloned() {
                    ui.vertical(|ui| {
                        if ui.button("Изменить папку").clicked() {
                            self.folder_edit_original = Some(name.clone());
                            self.folder_editor = self
                                .vault
                                .as_ref()
                                .unwrap()
                                .folders
                                .iter()
                                .find(|folder| folder.name == name)
                                .cloned();
                        }
                        if ui.button("Удалить папку").clicked() {
                            if self
                                .vault
                                .as_ref()
                                .unwrap()
                                .iter()
                                .any(|secret| secret.folder == name)
                            {
                                self.status =
                                    "Сначала переместите или удалите секреты из папки".into();
                            } else {
                                self.vault
                                    .as_mut()
                                    .unwrap()
                                    .folders
                                    .retain(|folder| folder.name != name);
                                self.selected_folder = self
                                    .vault
                                    .as_ref()
                                    .unwrap()
                                    .folders
                                    .first()
                                    .map(|folder| folder.name.clone());
                                self.save_secrets();
                            }
                        }
                    });
                }
            });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.separator();
            ui.heading(
                if self.filter_active_app || !self.secret_app_filter.is_empty() {
                    "Поиск по приложению"
                } else {
                    self.selected_folder.as_deref().unwrap_or("Создайте папку")
                },
            );
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("Поиск по названию"));
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
            let mut visible = 0;
            egui::ScrollArea::vertical()
                .max_height(ui.available_height().max(250.))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        for (i, secret) in self.vault.as_ref().unwrap().iter().enumerate() {
                            if !self.filter_active_app
                                && self.secret_app_filter.is_empty()
                                && self.selected_folder.as_ref() != Some(&secret.folder)
                            {
                                continue;
                            }
                            if !secret
                                .name
                                .to_lowercase()
                                .contains(&self.query.to_lowercase())
                                || if self.filter_active_app {
                                    !clipboard::app_matches(
                                        &self.pending_app,
                                        self.vault.as_ref().unwrap().app_for(secret),
                                    )
                                } else {
                                    !self
                                        .vault
                                        .as_ref()
                                        .unwrap()
                                        .app_for(secret)
                                        .to_lowercase()
                                        .contains(&self.secret_app_filter.to_lowercase())
                                }
                            {
                                continue;
                            }
                            visible += 1;
                            egui::Frame::group(ui.style())
                                .corner_radius(egui::CornerRadius::same(8))
                                .inner_margin(egui::Margin::same(12))
                                .fill(if self.secret_selected == Some(i) {
                                    egui::Color32::from_rgb(32, 58, 70)
                                } else {
                                    egui::Color32::from_rgb(37, 40, 43)
                                })
                                .show(ui, |ui| {
                                    ui.set_width((ui.available_width() - 24.).max(120.).min(700.));
                                    ui.set_min_height(110.);
                                    let response = ui.add(
                                        egui::Label::new(&secret.name)
                                            .truncate()
                                            .sense(egui::Sense::click()),
                                    );
                                    if response.clicked() {
                                        self.secret_selected = Some(i);
                                    }
                                    if response.double_clicked() {
                                        self.secret_selected = Some(i);
                                        paste = true;
                                    }
                                    if secret.hidden {
                                        ui.label("Значение: ••••••••");
                                    } else {
                                        ui.add(
                                            egui::Label::new(format!(
                                                "Значение: {}",
                                                secret.value.replace(['\r', '\n'], " ")
                                            ))
                                            .truncate(),
                                        );
                                    }
                                    let app = self.vault.as_ref().unwrap().app_for(secret);
                                    if !app.is_empty() {
                                        ui.add(
                                            egui::Label::new(format!(
                                                "{}: {app}",
                                                if secret.app.is_empty() {
                                                    "Приложение папки"
                                                } else {
                                                    "Приложение"
                                                }
                                            ))
                                            .truncate(),
                                        );
                                    }
                                    if !secret.folder.is_empty() {
                                        ui.add(
                                            egui::Label::new(format!("📁 {}", secret.folder))
                                                .truncate(),
                                        );
                                    }
                                    ui.horizontal(|ui| {
                                        if ui.small_button("Вставить").clicked() {
                                            self.secret_selected = Some(i);
                                            paste = true;
                                        }
                                        if ui.small_button("Копировать").clicked() {
                                            self.secret_selected = Some(i);
                                            copy = true;
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        if ui
                                            .small_button(if secret.hidden {
                                                "Показать"
                                            } else {
                                                "Скрыть"
                                            })
                                            .clicked()
                                        {
                                            toggle_visibility = Some(i);
                                        }
                                        if ui.small_button("Изменить").clicked() {
                                            self.secret_selected = Some(i);
                                            edit = true;
                                        }
                                        if ui.small_button("Удалить").clicked() {
                                            self.secret_selected = Some(i);
                                            delete = true;
                                        }
                                    });
                                });
                        }
                    });
                });
            if visible == 0 {
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
                            .clicked();
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
                            .clicked();
                        if ui.button("Отмена").clicked() {
                            self.secret_editor = None;
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
    fn new(cc: &eframe::CreationContext<'_>, initial_target: usize) -> Self {
        cc.egui_ctx.set_theme(egui::ThemePreference::Dark);
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::from_rgb(24, 24, 24);
        visuals.window_fill = egui::Color32::from_rgb(24, 24, 24);
        visuals.extreme_bg_color = egui::Color32::from_rgb(17, 17, 17);
        visuals.selection.bg_fill = egui::Color32::from_rgb(38, 120, 155);
        for widget in [
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
        ] {
            widget.corner_radius = egui::CornerRadius::same(5);
        }
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(43, 45, 47);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(55, 59, 62);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(38, 120, 155);
        cc.egui_ctx.set_visuals(visuals);
        cc.egui_ctx.style_mut_of(egui::Theme::Dark, |style| {
            style.spacing.button_padding = egui::vec2(10.0, 5.0);
            style.spacing.item_spacing = egui::vec2(8.0, 7.0);
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
            credential: String::new(),
            credential_input: String::new(),
            credential_confirm: String::new(),
            pin_mode: secrets::mode().unwrap_or(true),
            secret_editor: None,
            secret_selected: None,
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
    fn close_preferences(&mut self) {
        if self.draft == self.settings {
            self.settings_open = false;
        } else {
            self.confirm_discard = true;
        }
    }
    fn preferences(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            self.focus_settings = 0;
            return;
        }
        let focus_settings = self.focus_settings == 1;
        self.focus_settings = self.focus_settings.saturating_sub(1);
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustclip-settings"),
            egui::ViewportBuilder::default()
                .with_title("Настройки — RustClip")
                .with_inner_size([820., 610.])
                .with_min_inner_size([600., 420.]),
            |ui, _| {
                if ui.input(|i| i.viewport().focused == Some(true)) {
                    self.settings_last_active = true;
                }
                if focus_settings {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
                }
            ui.painter().rect_filled(ui.max_rect(), 0.0, egui::Color32::from_rgb(24, 24, 24));
            ui.heading(["Общее","Горячие клавиши","История буфера обмена","Настройки вставки","Дополнительно","Фильтрация","Секреты"][self.settings_page]);
            if !self.status.is_empty() { ui.colored_label(egui::Color32::RED, &self.status); }
            ui.separator();
            let mut cancel = false;
            egui::Panel::bottom("settings_actions").exact_size(52.).frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(8, 8))).show(ui, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Сохранить").clicked(){if let Err(e)=hotkeys::validate(&self.draft){self.status=e;}else{self.draft.recent_apps=self.settings.recent_apps.clone();self.settings=self.draft.clone();self.send(Command::Settings(self.settings.clone()));if let Err(e)=model::write_json(&model::directory().join("settings.json"),&self.settings){self.status=e;}else{self.settings_open=false;}}}
                    if ui.button("Отмена").clicked(){cancel=true;}
                });
            });
            egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal_top(|ui|{
                ui.vertical(|ui|{ui.set_width(190.);for (i,name) in ["Общее","Горячие клавиши","История буфера обмена","Настройки вставки","Дополнительно","Фильтрация","Секреты"].iter().enumerate(){ui.selectable_value(&mut self.settings_page,i,*name);}});
                ui.separator();ui.vertical(|ui|{match self.settings_page {
                    0=>{ui.checkbox(&mut self.draft.monitor,"Следить за буфером обмена");ui.checkbox(&mut self.draft.hide_after,"Спрятаться после копирования/вставки");ui.label("Закрытие окна сворачивает RustClip в трей.");ui.label(format!("Данные: {}",model::directory().display()));},
                    1=>{
                        ui.label("Глобальные сочетания (Ctrl, Shift, Alt, Win + клавиша):");
                        egui::Grid::new("hotkey_grid").num_columns(2).show(ui, |ui| {
                            for (name, value) in [
                                ("Открыть историю", &mut self.draft.hotkey_open),
                                ("Предыдущий клип", &mut self.draft.hotkey_previous),
                                ("Следующий клип", &mut self.draft.hotkey_next),
                                ("Вставить как текст", &mut self.draft.hotkey_plain_paste),
                            ] {
                                ui.label(name);
                                ui.text_edit_singleline(value);
                                ui.end_row();
                            }
                        });
                        ui.label("Пример: Ctrl+Shift+Up. Изменения применяются после сохранения.");
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
                }});
            });
            });
            if cancel {
                self.close_preferences();
            } else if ui.input(|i| i.viewport().close_requested()) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
                if !self.confirm_discard {
                    self.close_preferences();
                }
            } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                if self.confirm_discard {
                    self.confirm_discard = false;
                } else {
                    self.close_preferences();
                }
            }
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
    }
}
impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        if ctx.input(|i| i.viewport().focused == Some(true)) {
            self.settings_last_active = false;
        }
        if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::X)) {
            self.quitting = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Captured(clip) => self.add(clip),
                Event::ActiveApp(path) => {
                    self.settings
                        .recent_apps
                        .retain(|app| !app.eq_ignore_ascii_case(&path));
                    self.settings.recent_apps.insert(0, path);
                    self.settings.recent_apps.truncate(5);
                    self.draft.recent_apps = self.settings.recent_apps.clone();
                }
                Event::Open(app) => {
                    self.pending_app = app.clone();
                    self.query.clear();
                    self.secret_app_filter.clear();
                    self.filter_active_app = false;
                    if self.vault.is_none() && !self.credential.is_empty() {
                        match secrets::unlock(&self.credential) {
                            Ok(vault) => self.vault = Some(vault),
                            Err(e) => {
                                self.lock_secrets();
                                self.status = e;
                            }
                        }
                    }
                    self.snippets = self
                        .vault
                        .as_ref()
                        .is_some_and(|vault| has_secret_for_app(vault, &app));
                    if self.snippets {
                        self.selected_folder = None;
                        self.secret_app_filter = app.rsplit('\\').next().unwrap_or(&app).into();
                        self.filter_active_app = true;
                    } else {
                        self.clear_secret_view();
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    self.focus_settings = if self.settings_open && self.settings_last_active {
                        2
                    } else {
                        0
                    };
                    self.send(Command::HidePreview);
                }
                Event::Previous(destination) => self.cycle(&ctx, true, Some(destination)),
                Event::Next(destination) => self.cycle(&ctx, false, Some(destination)),
                Event::PlainPaste(destination) => {
                    if let Ok(mut cb) = arboard::Clipboard::new() {
                        if let Ok(text) = cb.get_text() {
                            self.send(Command::Paste(
                                clipboard::text_clip(text),
                                true,
                                Some(destination),
                                None,
                            ));
                        }
                    }
                }
                Event::Error(e) => self.status = e,
                Event::Quit => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        if self.quitting {
            self.send(Command::Quit);
        }
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let _ = model::write_json(&model::directory().join("settings.json"), &self.settings);
        self.lock_secrets();
        self.send(Command::Quit);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, egui::Color32::from_rgb(24, 24, 24));
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Файл", |ui| {
                if ui
                    .checkbox(&mut self.settings.monitor, "Следить за буфером обмена")
                    .changed()
                {
                    self.send(Command::Settings(self.settings.clone()));
                }
                if ui.button("Настройки…     F4").clicked() {
                    self.draft = self.settings.clone();
                    self.settings_open = true;
                    ui.close();
                }
                if ui.button("Создать резервную копию").clicked() {
                    self.status = match model::backup(&self.clips, &self.settings) {
                        Ok(path) => format!("Резервная копия: {}", path.display()),
                        Err(e) => e,
                    };
                    ui.close();
                }
                if ui.button("Свернуть в трей     Esc").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    ui.close();
                }
                if ui.button("Выход     Alt+X").clicked() {
                    self.quitting = true;
                    self.send(Command::Quit);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Клип", |ui| {
                for (name, paste, plain) in [
                    ("Вставить в активное окно", true, false),
                    ("Копировать в буфер обмена", false, false),
                    ("Вставить как простой текст", true, true),
                    ("Копировать как простой текст", false, true),
                ] {
                    if ui.button(name).clicked() {
                        self.action(&ctx, paste, plain);
                        ui.close();
                    }
                }
                if ui.button("Изменить     F2").clicked() {
                    if let Some(i) = self.selected {
                        self.editor = Some((Some(i), self.clips[i].text.clone()));
                    }
                    ui.close();
                }
                if ui.button("Удалить").clicked() {
                    if let Some(i) = self.selected {
                        self.remove(i);
                    }
                    ui.close();
                }
                if ui.button("Сохранить в файл…").clicked() {
                    if let Some(i) = self.selected {
                        self.status = match model::export_clip(&self.clips[i]) {
                            Ok(p) => format!("Сохранено: {}", p.display()),
                            Err(e) => e,
                        };
                    }
                    ui.close();
                }
            });
            ui.menu_button("Вид", |ui| {
                ui.checkbox(&mut self.stars, "Только со звёздочкой");
                ui.checkbox(&mut self.settings.newest_first, "Новые сверху");
            });
            ui.menu_button("Ярлыки", |ui| {
                if let Some(i) = self.selected {
                    if ui
                        .checkbox(&mut self.clips[i].starred, "Звёздочка")
                        .changed()
                    {
                        self.persist(i);
                    }
                    if ui
                        .text_edit_singleline(&mut self.clips[i].label)
                        .lost_focus()
                    {
                        self.persist(i);
                    }
                }
            });
            ui.menu_button("Помощь", |ui| {
                ui.label("RustClip 0.1 — рабочая начальная версия");
                ui.label("GPL-3.0-or-later");
                ui.label("Полный список ограничений: docs/IMPLEMENTATION.md");
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .selectable_value(&mut self.snippets, false, "История буфера обмена")
                .clicked()
            {
                self.clear_secret_view();
            }
            if ui
                .selectable_value(&mut self.snippets, true, "Секреты")
                .clicked()
            {
                self.filter_active_app = false;
                self.secret_app_filter.clear();
                self.select_first_folder();
            }
        });
        if self.snippets {
            self.secrets_ui(ui, &ctx);
            if !self.status.is_empty() {
                ui.colored_label(egui::Color32::RED, &self.status);
                if ui.small_button("Скрыть сообщение").clicked() {
                    self.status.clear();
                }
            }
            self.preferences(&ctx);
            return;
        }
        ui.horizontal(|ui| {
            if ui
                .add(egui::Button::new("Вставить").fill(egui::Color32::from_rgb(38, 120, 155)))
                .clicked()
            {
                self.action(&ctx, true, false);
            }
            if ui.button("Копировать").clicked() {
                self.action(&ctx, false, false);
            }
            if ui.button("Текст").clicked() {
                self.action(&ctx, true, true);
            }
            if ui.button("↑").on_hover_text("Предыдущий клип").clicked() {
                self.cycle(&ctx, true, None);
            }
            if ui.button("↓").on_hover_text("Следующий клип").clicked() {
                self.cycle(&ctx, false, None);
            }
            if ui.button("Удалить").clicked() {
                if let Some(i) = self.selected {
                    self.remove(i);
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Поиск:");
            ui.text_edit_singleline(&mut self.query);
            if ui.button("Очистить").clicked() {
                self.query.clear();
            }
        });
        let q = self.query.to_lowercase();
        let mut indices: Vec<_> = self
            .clips
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                c.snippet == self.snippets
                    && (!self.stars || c.starred)
                    && (q.is_empty()
                        || format!("{} {} {}", c.text, c.source, c.label)
                            .to_lowercase()
                            .contains(&q))
            })
            .map(|(i, _)| i)
            .collect();
        if self.settings.newest_first {
            indices.reverse();
        }
        ui.columns(2, |cols| {
            let height = (cols[0].available_height() - 75.).max(120.);
            cols[0].set_min_height(height);
            cols[1].set_min_height(height);
            egui::ScrollArea::vertical()
                .id_salt("history")
                .max_height(cols[0].available_height() - 45.)
                .show_rows(&mut cols[0], 24., indices.len(), |ui, range| {
                    for row in range {
                        let i = indices[row];
                        let c = &self.clips[i];
                        let icon = if c.starred {
                            "★"
                        } else if !c.png.is_empty() {
                            "▣"
                        } else {
                            "≡"
                        };
                        let response = ui.selectable_label(
                            self.selected == Some(i),
                            format!("{icon} {}", c.title()),
                        );
                        if response.clicked() {
                            self.selected = Some(i);
                        }
                        if response.double_clicked() {
                            self.selected = Some(i);
                            self.action(&ctx, true, false);
                        }
                    }
                });
            if let Some(c) = self.selected.and_then(|i| self.clips.get(i)) {
                cols[1].heading("Предпросмотр");
                cols[1].label(format!("Источник: {}", c.source));
                cols[1].label(format!(
                    "Форматы: {}",
                    c.formats
                        .iter()
                        .map(|f| f.0.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
                if !c.png.is_empty() {
                    let width =
                        (cols[1].available_width() * ctx.pixels_per_point()).max(1.0) as u32;
                    if self.texture.as_ref().map(|t| (t.0, t.1)) != Some((c.id, width)) {
                        if let Ok(img) = image::load_from_memory(&c.png) {
                            let img = if img.width() > width {
                                img.resize(
                                    width,
                                    ((img.height() as f64 * width as f64 / img.width() as f64)
                                        .round() as u32)
                                        .max(1),
                                    image::imageops::FilterType::Lanczos3,
                                )
                            } else {
                                img
                            };
                            let img = img.to_rgba8();
                            let size = [img.width() as usize, img.height() as usize];
                            self.texture = Some((
                                c.id,
                                width,
                                ctx.load_texture(
                                    "preview",
                                    egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()),
                                    Default::default(),
                                ),
                            ));
                        }
                    }
                    if let Some((_, _, tex)) = &self.texture {
                        cols[1].add(egui::Image::new(tex).fit_to_exact_size(
                            tex.size_vec2()
                                * (cols[1].available_width() / tex.size_vec2().x).min(1.),
                        ));
                    }
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("text")
                        .show(&mut cols[1], |ui| {
                            ui.label(&c.text);
                        });
                }
            }
        });
        ui.separator();
        ui.label(format!(
            "Клипов: {}  •  {}",
            self.clips.len(),
            if self.settings.monitor {
                "Мониторинг включён"
            } else {
                "Пауза"
            }
        ));
        if !self.status.is_empty() {
            ui.colored_label(egui::Color32::DARK_RED, &self.status);
            if ui.small_button("Скрыть сообщение").clicked() {
                self.status.clear();
            }
        }
        if !self.settings_open && self.editor.is_none() && !ctx.egui_wants_keyboard_input() {
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                self.select_adjacent(true);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                self.select_adjacent(false);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.action(&ctx, true, ctx.input(|i| i.modifiers.shift));
            }
            if ctx.input(|i| i.key_pressed(egui::Key::F2)) {
                if let Some(i) = self.selected {
                    self.editor = Some((Some(i), self.clips[i].text.clone()));
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::F4)) {
                self.draft = self.settings.clone();
                self.settings_open = true;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.send(Command::HidePreview);
            }
        }
        self.preferences(&ctx);
        let mut save = false;
        let mut cancel = false;
        if self.editor.is_some() {
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("rustclip-editor"),
                egui::ViewportBuilder::default()
                    .with_title("Редактор клипа — RustClip")
                    .with_inner_size([600., 600.])
                    .with_min_inner_size([400., 300.]),
                |ui, _| {
                    if ui.input(|i| i.viewport().close_requested()) {
                        cancel = true;
                        return;
                    }
                    if let Some((_, text)) = &mut self.editor {
                        ui.label("Сохранение заменяет исходные форматы простым текстом.");
                        ui.add(
                            egui::TextEdit::multiline(text)
                                .desired_rows(22)
                                .desired_width(f32::INFINITY),
                        );
                        ui.horizontal(|ui| {
                            save = ui.button("Сохранить").clicked();
                            cancel = ui.button("Отмена").clicked();
                        });
                    }
                },
            );
        }
        if save {
            if let Some((index, text)) = self.editor.take() {
                let mut c = clipboard::text_clip(text);
                if let Some(i) = index {
                    c.id = self.clips[i].id;
                    c.starred = self.clips[i].starred;
                    c.label = self.clips[i].label.clone();
                    c.snippet = self.clips[i].snippet;
                    self.clips[i] = c;
                    self.persist(i);
                } else {
                    c.snippet = true;
                    self.add(c);
                }
            }
        }
        if cancel {
            self.editor = None;
        }
    }
}
fn main() -> eframe::Result {
    use windows_sys::Win32::{Foundation::*, System::Threading::*, UI::WindowsAndMessaging::*};
    let initial_target = unsafe { GetForegroundWindow() } as usize;
    let demo = std::env::args().any(|arg| arg == "--demo");
    let mutex_name = if demo {
        "Local\\RustClip.DemoSingleInstance"
    } else {
        "Local\\RustClip.SingleInstance"
    };
    let mutex = unsafe { CreateMutexW(std::ptr::null(), 0, clipboard::wide(mutex_name).as_ptr()) };
    if !mutex.is_null() && unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            let hwnd = FindWindowW(
                clipboard::wide("RustClip.Listener").as_ptr(),
                std::ptr::null(),
            );
            PostMessageW(hwnd, WM_APP + 1, 0, WM_LBUTTONUP as isize);
            CloseHandle(mutex);
        }
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RustClip (История буфера обмена)")
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
                    .expect("valid app icon"),
            )
            .with_inner_size([1080., 800.]),
        ..Default::default()
    };
    let result = eframe::run_native(
        "RustClip",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, initial_target)))),
    );
    if !mutex.is_null() {
        unsafe {
            CloseHandle(mutex);
        }
    }
    result
}
