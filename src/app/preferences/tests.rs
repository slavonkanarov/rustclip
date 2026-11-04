use super::*;

#[derive(Default)]
struct SettingsUi {
    page: usize,
    checked: bool,
    text: String,
    number: i32,
    controls: Vec<egui::Id>,
}

impl SettingsUi {
    fn frame(&mut self, ctx: &egui::Context, key: Option<egui::Key>, focus: bool) {
        self.controls.clear();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(920., 660.),
                )),
                events: key
                    .map(|key| egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    })
                    .into_iter()
                    .collect(),
                ..Default::default()
            },
            |ui| {
                egui::Panel::left("navigation")
                    .exact_size(210.)
                    .resizable(false)
                    .show(ui, |ui| {
                        settings_navigation(
                            ui,
                            &mut self.page,
                            focus,
                            crate::i18n::Language::English,
                        );
                    });
                egui::CentralPanel::default().show(ui, |ui| {
                    self.controls
                        .push(ui.checkbox(&mut self.checked, "Option").id);
                    self.controls
                        .push(ui.text_edit_singleline(&mut self.text).id);
                    self.controls
                        .push(ui.add(egui::DragValue::new(&mut self.number).speed(1.)).id);
                });
            },
        );
        output.textures_delta.clear();
    }
}

#[test]
fn arrows_select_settings_pages_and_enter_controls() {
    let ctx = egui::Context::default();
    let mut settings = SettingsUi::default();
    settings.frame(&ctx, None, true);
    settings.frame(&ctx, Some(egui::Key::ArrowDown), false);
    settings.frame(&ctx, None, false);
    assert_eq!(settings.page, 1);
    settings.frame(&ctx, Some(egui::Key::ArrowUp), false);
    settings.frame(&ctx, None, false);
    assert_eq!(settings.page, 0);
    settings.frame(&ctx, Some(egui::Key::ArrowRight), false);
    settings.frame(&ctx, None, false);
    assert_eq!(ctx.memory(|m| m.focused()), Some(settings.controls[0]));
    settings.frame(&ctx, Some(egui::Key::Space), false);
    assert!(settings.checked);
    settings.frame(&ctx, Some(egui::Key::Tab), false);
    settings.frame(&ctx, None, false);
    assert_eq!(ctx.memory(|m| m.focused()), Some(settings.controls[1]));
    settings.frame(&ctx, Some(egui::Key::ArrowLeft), false);
    assert_eq!(ctx.memory(|m| m.focused()), Some(settings.controls[1]));
    settings.frame(&ctx, Some(egui::Key::Tab), false);
    settings.frame(&ctx, None, false);
    assert_eq!(ctx.memory(|m| m.focused()), Some(settings.controls[2]));
    settings.frame(&ctx, Some(egui::Key::ArrowUp), false);
    settings.frame(&ctx, None, false);
    assert_eq!(settings.number, 1);
    assert_eq!(ctx.memory(|m| m.focused()), Some(settings.controls[2]));
}

#[test]
fn right_arrow_enters_controls_from_every_page() {
    for page in 0..SETTINGS_PAGES.len() {
        let ctx = egui::Context::default();
        let mut settings = SettingsUi {
            page,
            ..Default::default()
        };
        settings.frame(&ctx, None, true);
        settings.frame(&ctx, Some(egui::Key::ArrowRight), false);
        settings.frame(&ctx, None, false);
        assert_eq!(settings.page, page);
        assert_eq!(ctx.memory(|m| m.focused()), Some(settings.controls[0]));
    }
}
