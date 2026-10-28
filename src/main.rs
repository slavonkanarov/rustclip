#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod clipboard;
mod hotkeys;
mod model;
use clipboard::{Command, Event};
use eframe::egui;
use model::{Clip, Settings};
use std::{collections::HashMap, sync::mpsc};

struct App {
    clips: Vec<Clip>,
    settings: Settings,
    tx: mpsc::Sender<Command>,
    rx: mpsc::Receiver<Event>,
    selected: Option<usize>,
    query: String,
    snippets: bool,
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
impl App {
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
                clips.push(clipboard::text_clip("Добро пожаловать в RustClip!\nИстория, поиск, текстовые шаблоны и центральное превью.".into()));
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
            ui.heading(["Общее","Горячие клавиши","История буфера обмена","Настройки вставки","Дополнительно","Фильтрация"][self.settings_page]);
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
                ui.vertical(|ui|{ui.set_width(190.);for (i,name) in ["Общее","Горячие клавиши","История буфера обмена","Настройки вставки","Дополнительно","Фильтрация"].iter().enumerate(){ui.selectable_value(&mut self.settings_page,i,*name);}});
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
                    _=>{
                        ui.label("Имя приложения (с .exe или без) или полный путь.");
                        app_list(ui, "Не захватывать из приложений:", &mut self.draft.excluded_capture, &mut self.adding_capture, &self.settings.recent_apps, &mut self.app_icons);
                        ui.separator();
                        app_list(ui, "Игнорировать горячие клавиши в приложениях:", &mut self.draft.excluded_hotkeys, &mut self.adding_hotkeys, &self.settings.recent_apps, &mut self.app_icons);
                        ui.separator();
                        ui.label("Дополнительные исключённые имена/ID форматов:");
                        ui.text_edit_multiline(&mut self.draft.excluded_formats);
                        ui.label("Базовый список проблемных форматов встроен; читаются только поддерживаемые форматы.");
                    }
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
                Event::Open => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    self.focus_settings = if self.settings_open && self.settings_last_active { 2 } else { 0 };
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
                if ui.button("Создать шаблон").clicked() {
                    if let Some(i) = self.selected {
                        let mut c = self.clips[i].clone();
                        c.id = model::now();
                        c.snippet = true;
                        self.add(c);
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
            if ui.button("+ Шаблон").clicked() {
                self.editor = Some((None, String::new()));
            }
        });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.snippets, false, "История буфера обмена");
            ui.selectable_value(&mut self.snippets, true, "Шаблоны");
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
