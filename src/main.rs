#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod clipboard;
mod hotkeys;
mod model;
use clipboard::{Command, Event};
use eframe::egui;
use model::{Clip, Settings};
use std::sync::mpsc;

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
    settings_page: usize,
    draft: Settings,
    editor: Option<(Option<usize>, String)>,
    status: String,
    texture: Option<(u128, egui::TextureHandle)>,
    quitting: bool,
}
impl App {
    fn new(cc: &eframe::CreationContext<'_>, initial_target: usize) -> Self {
        cc.egui_ctx.set_theme(egui::ThemePreference::Light);
        cc.egui_ctx.set_visuals(egui::Visuals::light());
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
            settings,
            tx,
            rx,
            query: String::new(),
            snippets: false,
            stars: false,
            settings_open: false,
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
        if let Some(clip) = self.selected.and_then(|i| self.clips.get(i)).cloned() {
            if paste {
                self.send(Command::Paste(clip, plain, None));
            } else {
                self.send(Command::Copy(clip, plain));
            }
            if self.settings.hide_after {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }
    }
    fn cycle(&mut self, ctx: &egui::Context, older: bool, destination: Option<usize>) {
        let history: Vec<_> = self
            .clips
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.snippet)
            .map(|(i, _)| i)
            .collect();
        if history.is_empty() {
            return;
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
        if let Some(clip) = self.selected.and_then(|i| self.clips.get(i)).cloned() {
            if self.settings.auto_paste {
                self.send(Command::Paste(clip, false, destination));
            } else {
                self.send(Command::Copy(clip, false));
            }
            if self.settings.hide_after {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }
    }
    fn preferences(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustclip-settings"),
            egui::ViewportBuilder::default()
                .with_title("Настройки — RustClip")
                .with_inner_size([820., 610.])
                .with_min_inner_size([600., 420.]),
            |ui, _| {
                if ui.input(|i| i.viewport().close_requested()) {
                    self.settings_open = false;
                    return;
                }
            egui::Frame::new().fill(egui::Color32::from_rgb(255,195,35)).show(ui,|ui|{ui.set_width(ui.available_width());ui.vertical_centered(|ui|{ui.heading(["Общее","Горячие клавиши","История буфера обмена","Настройки вставки","Дополнительно","Фильтрация"][self.settings_page]);});});
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
                        ui.label("Пример: Ctrl+Shift+Up. Изменения применяются после OK.");
                        if let Err(error) = hotkeys::validate(&self.draft) {
                            ui.colored_label(egui::Color32::RED, error);
                        }
                        ui.checkbox(&mut self.draft.auto_paste,"Автоматически вставлять после Quick Copy");
                    },
                    2=>{ui.label("Максимальное количество незакреплённых клипов");ui.add(egui::DragValue::new(&mut self.draft.limit).range(1..=50000));ui.label("Проверять последние клипы на дубликаты");ui.add(egui::DragValue::new(&mut self.draft.duplicates).range(0..=100));ui.checkbox(&mut self.draft.newest_first,"Новые клипы сверху");},
                    3=>{ui.checkbox(&mut self.draft.ctrl_v,"Эмулировать Ctrl+V (иначе Shift+Insert)");ui.checkbox(&mut self.draft.restore,"Восстанавливать Clipboard после вставки");ui.label("Задержка совместимости, мс");ui.add(egui::DragValue::new(&mut self.draft.paste_delay).range(20..=2000));},
                    4=>{ui.label("Таймер центрального превью, мс (0 — без таймера)");ui.add(egui::DragValue::new(&mut self.draft.preview_ms).range(0..=60000));ui.label("Максимальный текст, KiB");ui.add(egui::DragValue::new(&mut self.draft.max_text_kb).range(1..=32768));ui.label("Esc скрывает центральное превью.");},
                    _=>{ui.label("Один exe или полный путь на строку. Без wildcard.");ui.label("Не захватывать из приложений:");ui.text_edit_multiline(&mut self.draft.excluded_capture);ui.label("Игнорировать hotkeys в приложениях:");ui.text_edit_multiline(&mut self.draft.excluded_hotkeys);ui.label("Дополнительные исключённые имена/ID форматов:");ui.text_edit_multiline(&mut self.draft.excluded_formats);ui.label("Базовый список проблемных форматов встроен; читаются только поддерживаемые форматы.");}
                }});
            });
            ui.separator();ui.horizontal(|ui|{if ui.button("OK").clicked(){if let Err(e)=hotkeys::validate(&self.draft){self.status=e;}else{self.settings=self.draft.clone();self.send(Command::Settings(self.settings.clone()));if let Err(e)=model::write_json(&model::directory().join("settings.json"),&self.settings){self.status=e;}else{self.settings_open=false;}}}if ui.button("Отмена").clicked(){self.settings_open=false;}});
            },
        );
    }
}
impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::X)) {
            self.quitting = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Captured(clip) => self.add(clip),
                Event::Open => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
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
            .rect_filled(ui.max_rect(), 0.0, egui::Color32::from_rgb(245, 245, 245));
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
            if ui.button("Вставить").clicked() {
                self.action(&ctx, true, false);
            }
            if ui.button("Копировать").clicked() {
                self.action(&ctx, false, false);
            }
            if ui.button("Текст").clicked() {
                self.action(&ctx, true, true);
            }
            if ui.button("↑").clicked() {
                self.cycle(&ctx, true, None);
            }
            if ui.button("↓").clicked() {
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
            if ui.button("×").clicked() {
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
                    if self.texture.as_ref().map(|t| t.0) != Some(c.id) {
                        if let Ok(img) = image::load_from_memory(&c.png) {
                            let img = img.to_rgba8();
                            let size = [img.width() as usize, img.height() as usize];
                            self.texture = Some((
                                c.id,
                                ctx.load_texture(
                                    "preview",
                                    egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()),
                                    Default::default(),
                                ),
                            ));
                        }
                    }
                    if let Some((_, tex)) = &self.texture {
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
