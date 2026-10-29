use super::{
    App,
    widgets::{ACCENT, BACKGROUND, CLIP_ROW_HEIGHT, MUTED, SURFACE, clip_icon, clip_kind},
};
use crate::{
    clipboard::{self, Command, Event},
    model, secrets,
};
use eframe::egui;
use std::time::Instant;
use zeroize::Zeroize;

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
