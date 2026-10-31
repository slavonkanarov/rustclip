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
                    self.pending_app = app;
                    self.query.clear();
                    self.secret_app_filter.clear();
                    self.filter_active_app = false;
                    self.unlock_open = false;
                    self.unlock_input.zeroize();
                    self.route_app();
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
        let language = self.settings.language;
        let ctx = ui.ctx().clone();
        if super::widgets::back_requested(&ctx) {
            if self.unlock_open {
                self.cancel_unlock();
            } else if let Some(mut secret) = self.secret_editor.take() {
                secret.clear();
            } else if self.folder_editor.take().is_some() {
                self.folder_edit_original = None;
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.send(Command::HidePreview);
            }
        }
        if self.editor.is_none()
            && self.secret_editor.is_none()
            && self.folder_editor.is_none()
            && !self.unlock_open
            && ctx.input_mut(|i| {
                i.modifiers.is_none() && i.consume_key(egui::Modifiers::NONE, egui::Key::F4)
            })
        {
            egui::Popup::close_all(&ctx);
            self.open_preferences();
        }
        ui.painter().rect_filled(ui.max_rect(), 0.0, BACKGROUND);
        egui::Frame::NONE.inner_margin(20).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(egui::RichText::new("RustClip").strong().size(27.));
                ui.colored_label(MUTED, language.text("Everything you copy, close at hand"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(language.text("⚙ Settings"))
                        .on_hover_text(language.text("Application settings · F4"))
                        .clicked()
                    {
                        self.open_preferences();
                    }
                });
            });
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button(language.text("File"), |ui| {
                    if ui
                        .checkbox(
                            &mut self.settings.monitor,
                            language.text("Monitor clipboard"),
                        )
                        .changed()
                    {
                        self.send(Command::Settings(self.settings.clone()));
                    }
                    if ui.button(language.text("Settings…     F4")).clicked() {
                        self.open_preferences();
                        ui.close();
                    }
                    if ui.button(language.text("Create backup")).clicked() {
                        self.status = match model::backup(&self.clips, &self.settings) {
                            Ok(path) => format!("{} {}", language.text("Backup:"), path.display()),
                            Err(e) => e,
                        };
                        ui.close();
                    }
                    if ui
                        .button(language.text("Minimize to tray     Esc"))
                        .clicked()
                    {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                        ui.close();
                    }
                    if ui.button(language.text("Exit     Alt+X")).clicked() {
                        self.quitting = true;
                        self.send(Command::Quit);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button(language.text("Clip"), |ui| {
                    for (name, paste, plain) in [
                        (language.text("Paste into active window"), true, false),
                        (language.text("Copy to clipboard"), false, false),
                        (language.text("Paste as plain text"), true, true),
                        (language.text("Copy as plain text"), false, true),
                    ] {
                        if ui.button(name).clicked() {
                            self.action(&ctx, paste, plain);
                            ui.close();
                        }
                    }
                    if ui.button(language.text("Edit     F2")).clicked() {
                        if let Some(i) = self.selected {
                            self.editor = Some((Some(i), self.clips[i].text.clone()));
                        }
                        ui.close();
                    }
                    if ui.button(language.text("Delete")).clicked() {
                        if let Some(i) = self.selected {
                            self.remove(i);
                        }
                        ui.close();
                    }
                    if ui.button(language.text("Save to file…")).clicked() {
                        if let Some(i) = self.selected {
                            self.status = match model::export_clip(&self.clips[i]) {
                                Ok(p) => format!("{} {}", language.text("Saved:"), p.display()),
                                Err(e) => e,
                            };
                        }
                        ui.close();
                    }
                });
                ui.menu_button(language.text("View"), |ui| {
                    ui.checkbox(&mut self.stars, language.text("Starred only"));
                    ui.checkbox(
                        &mut self.settings.newest_first,
                        language.text("Newest first"),
                    );
                });
                ui.menu_button(language.text("Labels"), |ui| {
                    if let Some(i) = self.selected {
                        if ui
                            .checkbox(&mut self.clips[i].starred, language.text("Star"))
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
                ui.menu_button(language.text("Help"), |ui| {
                    ui.label(format!(
                        "RustClip {}",
                        option_env!("RUSTCLIP_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
                    ));
                    ui.label("GPL-3.0-or-later");
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .selectable_value(&mut self.snippets, false, language.text("History"))
                    .clicked()
                {
                    self.unlock_open = false;
                    self.unlock_input.zeroize();
                }
                if ui
                    .selectable_value(&mut self.snippets, true, language.text("Secrets"))
                    .clicked()
                {
                    self.filter_active_app = false;
                    self.secret_app_filter.clear();
                    if self.vault.is_none() && secrets::exists() {
                        self.ask_unlock();
                    } else {
                        self.select_first_folder();
                    }
                }
            });
            if self.snippets {
                self.secrets_ui(ui, &ctx);
                if !self.status.is_empty() {
                    ui.colored_label(egui::Color32::RED, language.message(&self.status));
                    if ui.small_button(language.text("Dismiss message")).clicked() {
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
                        egui::Button::new(language.text("Paste")).fill(ACCENT),
                    )
                    .on_hover_text(language.text("Paste into active application · Enter"))
                    .clicked()
                {
                    self.action(&ctx, true, false);
                }
                if ui
                    .add_enabled(
                        self.selected.is_some(),
                        egui::Button::new(language.text("As text")),
                    )
                    .on_hover_text(language.text("Paste without formatting · Shift+Enter"))
                    .clicked()
                {
                    self.action(&ctx, true, true);
                }
                if ui
                    .add_enabled(
                        self.selected.is_some(),
                        egui::Button::new(language.text("Edit")),
                    )
                    .on_hover_text(language.text("Edit clip text · F2"))
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
                                language.text("Remove star")
                            } else {
                                language.text("Add to favorites")
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
                    .add_enabled(
                        self.selected.is_some(),
                        egui::Button::new(language.text("Delete")),
                    )
                    .clicked()
                {
                    if let Some(i) = self.selected {
                        self.remove(i);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            self.selected.is_some(),
                            egui::Button::new(language.text("📋 Copy")),
                        )
                        .on_hover_text(language.text("Copy clip to clipboard"))
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
                        .hint_text(language.text("Search text, app, or label…  Ctrl+F"))
                        .margin(10.),
                );
                if !self.settings_open
                    && !self.unlock_open
                    && self.editor.is_none()
                    && ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::F))
                {
                    search.request_focus();
                }
                if ui
                    .add_enabled(
                        !self.query.is_empty(),
                        egui::Button::new(language.text("Reset")),
                    )
                    .clicked()
                {
                    self.query.clear();
                }
                if ui
                    .selectable_label(self.stars, language.text("Favorites"))
                    .on_hover_text(language.text("Toggle favorites filter"))
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
                && !self.unlock_open
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
                    ui.strong(language.text("Clipboard history"));
                    ui.colored_label(
                        MUTED,
                        format!(
                            "{} {} {}",
                            indices.len(),
                            language.text("of"),
                            self.clips.len()
                        ),
                    );
                });
                cols[0].separator();
                if indices.is_empty() {
                    cols[0].add_space(30.);
                    cols[0].heading(if self.query.is_empty() && !self.stars {
                        language.text("Your clips will appear here")
                    } else {
                        language.text("Nothing found")
                    });
                    cols[0].colored_label(
                        MUTED,
                        if self.query.is_empty() && !self.stars {
                            language.text("Copy text or an image while monitoring is enabled.")
                        } else {
                            language.text("Change the search or turn off the favorites filter.")
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
                                                format!("★ {}", c.title(language))
                                            } else {
                                                c.title(language)
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
                    cols[1].heading(language.text("Preview"));
                    cols[1].colored_label(
                        MUTED,
                        format!(
                            "{} {}",
                            language.text("Source:"),
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
                            "{} {}",
                            language.text("Formats:"),
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
                "{} {}  •  {}",
                language.text("Clips:"),
                self.clips.len(),
                if self.settings.monitor {
                    language.text("Monitoring on")
                } else {
                    language.text("Paused")
                }
            ));
            if !self.status.is_empty() {
                ui.colored_label(egui::Color32::LIGHT_RED, language.message(&self.status));
                if ui.small_button(language.text("Dismiss message")).clicked() {
                    self.status.clear();
                }
            }
            if !self.settings_open
                && !self.unlock_open
                && self.editor.is_none()
                && !egui::Popup::is_any_open(&ctx)
                && !ctx.egui_wants_keyboard_input()
            {
                if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.action(&ctx, true, ctx.input(|i| i.modifiers.shift));
                }
                if ctx.input(|i| i.key_pressed(egui::Key::F2)) {
                    if let Some(i) = self.selected {
                        self.editor = Some((Some(i), self.clips[i].text.clone()));
                    }
                }
            }
            self.preferences(&ctx);
            let mut save = false;
            let mut cancel = false;
            if self.editor.is_some() {
                ctx.show_viewport_immediate(
                    egui::ViewportId::from_hash_of("rustclip-editor"),
                    egui::ViewportBuilder::default()
                        .with_title(language.text("Clip editor — RustClip"))
                        .with_inner_size([600., 600.])
                        .with_min_inner_size([400., 300.]),
                    |ui, _| {
                        if ui.input(|i| i.viewport().close_requested())
                            || super::widgets::back_requested(ui.ctx())
                        {
                            cancel = true;
                            return;
                        }
                        if let Some((_, text)) = &mut self.editor {
                            ui.label(
                                language
                                    .text("Saving replaces the original formats with plain text."),
                            );
                            ui.add(
                                egui::TextEdit::multiline(text)
                                    .desired_rows(22)
                                    .desired_width(f32::INFINITY),
                            );
                            ui.horizontal(|ui| {
                                save = ui
                                    .button(language.text("Save"))
                                    .on_hover_text("Ctrl+Enter")
                                    .clicked()
                                    || super::widgets::save_requested(ui.ctx());
                                cancel = ui.button(language.text("Cancel")).clicked();
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
        self.unlock_dialog(&ctx);
    }
}
