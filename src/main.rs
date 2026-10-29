#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod clipboard;
mod hotkeys;
mod model;
mod secrets;
use clipboard::{Command, Event};
use eframe::egui;
use model::{Clip, Settings};
use std::{collections::HashMap, sync::mpsc, time::Instant};
use zeroize::Zeroize;

const BACKGROUND: egui::Color32 = egui::Color32::from_rgb(20, 24, 32);
const SURFACE: egui::Color32 = egui::Color32::from_rgb(29, 35, 46);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(48, 126, 164);
const MUTED: egui::Color32 = egui::Color32::from_rgb(153, 167, 187);
const CLIP_ROW_HEIGHT: f32 = 34.;

fn clip_kind(clip: &Clip) -> &'static str {
    if clip.formats.iter().any(|(format, _)| format == "15") {
        "Файлы"
    } else if !clip.png.is_empty() {
        "Изображение"
    } else if !clip.text.is_empty() {
        "Текст"
    } else {
        "Данные"
    }
}

fn clip_icon(ui: &mut egui::Ui, kind: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(24., 24.), egui::Sense::hover());
    let painter = ui.painter();
    let stroke = egui::Stroke::new(1.5, egui::Color32::from_rgb(134, 193, 221));
    let paper = egui::Rect::from_min_size(rect.min + egui::vec2(4., 2.), egui::vec2(16., 20.));
    match kind {
        "Изображение" => {
            painter.rect_stroke(paper, 3., stroke, egui::StrokeKind::Inside);
            painter.circle_filled(paper.min + egui::vec2(11., 6.), 1.5, stroke.color);
            painter.line_segment(
                [
                    paper.min + egui::vec2(3., 16.),
                    paper.min + egui::vec2(7., 11.),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    paper.min + egui::vec2(7., 11.),
                    paper.min + egui::vec2(13., 17.),
                ],
                stroke,
            );
        }
        "Файлы" => {
            painter.rect_stroke(
                paper.translate(egui::vec2(-3., -2.)),
                2.,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.rect_filled(paper, 2., SURFACE);
            painter.rect_stroke(paper, 2., stroke, egui::StrokeKind::Inside);
            painter.line_segment(
                [
                    paper.min + egui::vec2(4., 7.),
                    paper.min + egui::vec2(12., 7.),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    paper.min + egui::vec2(4., 12.),
                    paper.min + egui::vec2(12., 12.),
                ],
                stroke,
            );
        }
        "Текст" => {
            painter.rect_stroke(paper, 2., stroke, egui::StrokeKind::Inside);
            for y in [6., 10., 14.] {
                painter.line_segment(
                    [
                        paper.min + egui::vec2(4., y),
                        paper.min + egui::vec2(12., y),
                    ],
                    stroke,
                );
            }
        }
        _ => {
            painter.rect_stroke(paper, 3., stroke, egui::StrokeKind::Inside);
            for x in [5., 11.] {
                for y in [7., 13.] {
                    painter.circle_filled(paper.min + egui::vec2(x, y), 1.5, stroke.color);
                }
            }
        }
    }
}
fn secret_button(ui: &mut egui::Ui, icon: &str, hint: &str) -> egui::Response {
    let response = ui
        .add(
            egui::Button::new("")
                .min_size(egui::vec2(30., 30.))
                .corner_radius(4),
        )
        .on_hover_text_at_pointer(hint);
    let rect = response.rect;
    let pressed = response.is_pointer_button_down_on();
    let center = rect.center()
        + if pressed {
            egui::vec2(1., 1.)
        } else {
            egui::Vec2::ZERO
        };
    let painter = ui.painter();
    painter.rect_filled(
        rect.shrink(1.),
        4.,
        if pressed {
            egui::Color32::from_rgb(54, 111, 145)
        } else if response.hovered() {
            egui::Color32::from_rgb(44, 70, 89)
        } else {
            egui::Color32::from_rgb(36, 47, 62)
        },
    );
    painter.rect_stroke(
        rect.shrink(1.),
        4.,
        egui::Stroke::new(
            1.,
            if pressed {
                egui::Color32::WHITE
            } else {
                egui::Color32::from_rgb(75, 91, 110)
            },
        ),
        egui::StrokeKind::Inside,
    );
    let stroke = egui::Stroke::new(
        1.6,
        if icon == "check" {
            egui::Color32::LIGHT_GREEN
        } else {
            MUTED
        },
    );
    match icon {
        "copy" => {
            painter.rect_stroke(
                egui::Rect::from_min_size(center + egui::vec2(-6., -7.), egui::vec2(10., 11.)),
                1.,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.rect_stroke(
                egui::Rect::from_min_size(center + egui::vec2(-3., -4.), egui::vec2(10., 11.)),
                1.,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        "check" => {
            painter.line(
                vec![
                    center + egui::vec2(-6., 0.),
                    center + egui::vec2(-1., 5.),
                    center + egui::vec2(7., -5.),
                ],
                stroke,
            );
        }
        "delete" => {
            painter.line_segment(
                [center + egui::vec2(-7., -5.), center + egui::vec2(7., -5.)],
                stroke,
            );
            painter.rect_stroke(
                egui::Rect::from_min_size(center + egui::vec2(-5., -3.), egui::vec2(10., 10.)),
                1.,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.line_segment(
                [center + egui::vec2(-3., -7.), center + egui::vec2(3., -7.)],
                stroke,
            );
        }
        "edit" => {
            painter.line_segment(
                [center + egui::vec2(-5., 5.), center + egui::vec2(5., -5.)],
                stroke,
            );
            painter.line_segment(
                [center + egui::vec2(-7., 7.), center + egui::vec2(-3., 6.)],
                stroke,
            );
        }
        "show" | "hide" => {
            painter.circle_stroke(center, 6., stroke);
            painter.circle_filled(center, 2., stroke.color);
            if icon == "hide" {
                painter.line_segment(
                    [center + egui::vec2(-8., 8.), center + egui::vec2(8., -8.)],
                    stroke,
                );
            }
        }
        _ => {}
    }
    response
}
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

struct App {
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
    unlock_for_app: bool,
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
    fn lock_secrets(&mut self) {
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
    fn ask_unlock(&mut self, for_app: bool) {
        self.snippets = false;
        self.unlock_open = true;
        self.unlock_for_app = for_app;
        self.focus_unlock = true;
        self.unlock_input.zeroize();
        self.status.clear();
        if let Ok(pin) = secrets::mode() {
            self.pin_mode = pin;
        }
    }
    fn route_app(&mut self) {
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
    fn unlock_dialog(&mut self, ctx: &egui::Context) {
        if !self.unlock_open {
            return;
        }
        let mut submit = false;
        let mut cancel = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustclip-unlock"),
            egui::ViewportBuilder::default()
                .with_title("Разблокировать секреты — RustClip")
                .with_inner_size([360., 170.])
                .with_resizable(false),
            |ui, _| {
                if ui.input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape))
                {
                    cancel = true;
                    return;
                }
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
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
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
            },
        );
        if cancel {
            self.unlock_open = false;
            self.snippets = false;
            self.unlock_input.zeroize();
            self.status.clear();
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        } else if submit {
            match secrets::unlock_session(&self.unlock_input) {
                Ok((vault, key)) => {
                    self.vault = Some(vault);
                    self.credential = Some(key);
                    self.unlock_input.zeroize();
                    self.status.clear();
                    self.unlock_open = false;
                    self.migrate_templates();
                    if self.unlock_for_app {
                        self.route_app();
                    } else {
                        self.snippets = true;
                        self.select_first_folder();
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                Err(e) => {
                    self.status = e;
                    self.unlock_input.zeroize();
                    self.focus_unlock = true;
                }
            }
        }
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
            unlock_for_app: false,
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
                } });
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
                    self.scroll_to_newest = true;
                    self.pending_app = app.clone();
                    self.query.clear();
                    self.secret_app_filter.clear();
                    self.filter_active_app = false;
                    if self.vault.is_none() && secrets::exists() && !app.is_empty() {
                        self.ask_unlock(true);
                    } else {
                        self.unlock_open = false;
                        self.unlock_input.zeroize();
                        self.route_app();
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
                Event::Error(e) => {
                    self.secret_copied = None;
                    self.status = e;
                }
                Event::SecretCopied(i) => {
                    self.status.clear();
                    self.secret_copied = Some((i, Instant::now()));
                }
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
        ui.painter().rect_filled(ui.max_rect(), 0.0, BACKGROUND);
        egui::Frame::NONE.inner_margin(20).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(egui::RichText::new("RustClip").strong().size(27.));
                ui.colored_label(MUTED, "Всё скопированное — под рукой");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button("⚙ Настройки")
                        .on_hover_text("Настройки приложения · F4")
                        .clicked()
                    {
                        self.draft = self.settings.clone();
                        self.settings_open = true;
                        self.focus_settings = 2;
                    }
                });
            });
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
                    .selectable_value(&mut self.snippets, false, "История")
                    .clicked()
                {
                    self.unlock_open = false;
                    self.unlock_input.zeroize();
                }
                if ui
                    .selectable_value(&mut self.snippets, true, "Секреты")
                    .clicked()
                {
                    self.filter_active_app = false;
                    self.secret_app_filter.clear();
                    if self.vault.is_none() && secrets::exists() {
                        self.ask_unlock(false);
                    } else {
                        self.select_first_folder();
                    }
                }
            });
            self.unlock_dialog(&ctx);
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
                    .add_enabled(
                        self.selected.is_some(),
                        egui::Button::new("Вставить").fill(ACCENT),
                    )
                    .on_hover_text("Вставить в активное приложение · Enter")
                    .clicked()
                {
                    self.action(&ctx, true, false);
                }
                if ui
                    .add_enabled(self.selected.is_some(), egui::Button::new("Как текст"))
                    .on_hover_text("Вставить без форматирования · Shift+Enter")
                    .clicked()
                {
                    self.action(&ctx, true, true);
                }
                if ui
                    .add_enabled(self.selected.is_some(), egui::Button::new("Изменить"))
                    .on_hover_text("Редактировать текст клипа · F2")
                    .clicked()
                {
                    if let Some(i) = self.selected {
                        self.editor = Some((Some(i), self.clips[i].text.clone()));
                    }
                }
                if ui
                    .add_enabled(
                        self.selected.is_some(),
                        egui::Button::new(
                            if self.selected.is_some_and(|i| self.clips[i].starred) {
                                "Убрать звёздочку"
                            } else {
                                "В избранное"
                            },
                        ),
                    )
                    .clicked()
                {
                    if let Some(i) = self.selected {
                        self.clips[i].starred = !self.clips[i].starred;
                        self.persist(i);
                    }
                }
                if ui
                    .add_enabled(self.selected.is_some(), egui::Button::new("Удалить"))
                    .clicked()
                {
                    if let Some(i) = self.selected {
                        self.remove(i);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(self.selected.is_some(), egui::Button::new("📋 Копировать"))
                        .on_hover_text("Скопировать клип в буфер обмена")
                        .clicked()
                    {
                        self.action(&ctx, false, false);
                    }
                });
            });
            ui.horizontal(|ui| {
                let search = ui.add_sized(
                    [(ui.available_width() - 240.).max(160.), 38.],
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("Поиск по тексту, приложению или ярлыку…  Ctrl+F")
                        .margin(10.),
                );
                if !self.settings_open
                    && self.editor.is_none()
                    && ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::F))
                {
                    search.request_focus();
                }
                if ui
                    .add_enabled(!self.query.is_empty(), egui::Button::new("Сбросить"))
                    .clicked()
                {
                    self.query.clear();
                }
                if ui
                    .selectable_label(self.stars, "Избранное")
                    .on_hover_text("Включить или отключить фильтр избранного")
                    .clicked()
                {
                    self.stars = !self.stars;
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
            if self.selected.is_some_and(|i| !indices.contains(&i)) || self.selected.is_none() {
                self.selected = indices.first().copied();
            }
            let navigating = !self.settings_open
                && self.editor.is_none()
                && !ctx.egui_wants_keyboard_input()
                && ctx.input(|i| {
                    i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::ArrowDown)
                });
            if navigating {
                if let Some(row) = indices.iter().position(|i| Some(*i) == self.selected) {
                    self.selected = indices
                        .get(if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                            row.saturating_sub(1)
                        } else {
                            (row + 1).min(indices.len() - 1)
                        })
                        .copied();
                }
            }
            ui.add_space(6.);
            ui.columns(2, |cols| {
                let height = (cols[0].available_height() - 75.).max(120.);
                cols[0].set_min_height(height);
                cols[1].set_min_height(height);
                cols[0].horizontal(|ui| {
                    ui.strong("История буфера обмена");
                    ui.colored_label(MUTED, format!("{} из {}", indices.len(), self.clips.len()));
                });
                cols[0].separator();
                if indices.is_empty() {
                    cols[0].add_space(30.);
                    cols[0].heading(if self.query.is_empty() && !self.stars {
                        "Здесь появятся ваши клипы"
                    } else {
                        "Ничего не найдено"
                    });
                    cols[0].colored_label(
                        MUTED,
                        if self.query.is_empty() && !self.stars {
                            "Скопируйте текст или изображение при включённом мониторинге."
                        } else {
                            "Измените запрос или отключите фильтр избранного."
                        },
                    );
                }
                let viewport_height = cols[0].available_height() - 45.;
                cols[0].spacing_mut().item_spacing.y = 4.;
                let row_stride = CLIP_ROW_HEIGHT + cols[0].spacing().item_spacing.y;
                let mut scroll = egui::ScrollArea::vertical()
                    .id_salt("history")
                    .max_height(viewport_height);
                if navigating {
                    if let Some(row) = indices.iter().position(|i| Some(*i) == self.selected) {
                        scroll = scroll.vertical_scroll_offset(
                            (row as f32 * row_stride - viewport_height / 2.).max(0.),
                        );
                    }
                }
                if self.scroll_to_newest {
                    scroll = scroll.vertical_scroll_offset(if self.settings.newest_first {
                        0.
                    } else {
                        (indices.len() as f32 * row_stride
                            - cols[0].spacing().item_spacing.y
                            - viewport_height)
                            .max(0.)
                    });
                    self.scroll_to_newest = false;
                }
                scroll.show_rows(&mut cols[0], CLIP_ROW_HEIGHT, indices.len(), |ui, range| {
                    for row in range {
                        let i = indices[row];
                        let c = &self.clips[i];
                        let kind = clip_kind(c);
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), CLIP_ROW_HEIGHT),
                            egui::Sense::click(),
                        );
                        ui.painter().rect_filled(
                            rect,
                            6.,
                            if self.selected == Some(i) {
                                egui::Color32::from_rgb(32, 66, 88)
                            } else if response.hovered() {
                                egui::Color32::from_rgb(38, 46, 60)
                            } else {
                                SURFACE
                            },
                        );
                        if self.selected == Some(i) {
                            ui.painter().rect_filled(
                                egui::Rect::from_min_size(
                                    rect.min + egui::vec2(0., 6.),
                                    egui::vec2(3., CLIP_ROW_HEIGHT - 12.),
                                ),
                                2.,
                                ACCENT,
                            );
                        }
                        let text_clicks = ui.scope_builder(
                            egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(10., 4.))),
                            |ui| {
                                ui.horizontal(|ui| {
                                    clip_icon(ui, kind);
                                    let title = ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(if c.starred {
                                                format!("★ {}", c.title())
                                            } else {
                                                c.title()
                                            })
                                            .strong(),
                                        )
                                        .truncate(),
                                    );
                                    (title.clicked(), title.double_clicked())
                                })
                                .inner
                            },
                        );
                        ui.advance_cursor_after_rect(rect);
                        if response.clicked() || text_clicks.inner.0 {
                            self.selected = Some(i);
                        }
                        if response.double_clicked() || text_clicks.inner.1 {
                            self.selected = Some(i);
                            self.action(&ctx, true, false);
                        }
                    }
                });
                if let Some(c) = self.selected.and_then(|i| self.clips.get(i)) {
                    cols[1].heading("Предпросмотр");
                    cols[1].colored_label(
                        MUTED,
                        format!(
                            "Источник: {}",
                            if c.source.is_empty() {
                                "—"
                            } else {
                                &c.source
                            }
                        ),
                    );
                    cols[1].colored_label(
                        MUTED,
                        format!(
                            "Форматы: {}",
                            c.formats
                                .iter()
                                .map(|f| f.0.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    );
                    if !c.png.is_empty() {
                        let width =
                            (cols[1].available_width() * ctx.pixels_per_point()).max(1.0) as u32;
                        if self.texture.as_ref().map(|t| (t.0, t.1)) != Some((c.id, width)) {
                            if let Ok(img) = image::load_from_memory(&c.png) {
                                let img = if img.width() > width {
                                    img.resize(
                                        width,
                                        ((img.height() as f64 * width as f64 / img.width() as f64)
                                            .round()
                                            as u32)
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
                                        egui::ColorImage::from_rgba_unmultiplied(
                                            size,
                                            img.as_raw(),
                                        ),
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
                ui.colored_label(egui::Color32::LIGHT_RED, &self.status);
                if ui.small_button("Скрыть сообщение").clicked() {
                    self.status.clear();
                }
            }
            if !self.settings_open && self.editor.is_none() && !ctx.egui_wants_keyboard_input() {
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
        });
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
            .with_inner_size([1120., 800.])
            .with_min_inner_size([880., 560.]),
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
