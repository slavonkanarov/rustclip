use crate::{clipboard, model::Clip};
use eframe::egui;
use std::collections::HashMap;

pub(super) const BACKGROUND: egui::Color32 = egui::Color32::from_rgb(20, 24, 32);
pub(super) const SURFACE: egui::Color32 = egui::Color32::from_rgb(29, 35, 46);
pub(super) const ACCENT: egui::Color32 = egui::Color32::from_rgb(48, 126, 164);
pub(super) const MUTED: egui::Color32 = egui::Color32::from_rgb(153, 167, 187);
pub(super) const CLIP_ROW_HEIGHT: f32 = 34.;

pub(super) fn back_requested(ctx: &egui::Context) -> bool {
    let back = ctx.input_mut(|input| {
        let escape = input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
        let mut mouse = false;
        input.events.retain(|event| {
            if matches!(
                event,
                egui::Event::PointerButton {
                    button: egui::PointerButton::Extra1,
                    pressed: true,
                    ..
                }
            ) {
                mouse = true;
                false
            } else {
                true
            }
        });
        escape || mouse
    });
    if back && egui::Popup::is_any_open(ctx) {
        egui::Popup::close_all(ctx);
        return false;
    }
    back
}

pub(super) fn save_requested(ctx: &egui::Context) -> bool {
    !egui::Popup::is_any_open(ctx)
        && ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Enter))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn escape() -> egui::Event {
        egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn mouse(button: egui::PointerButton) -> egui::Event {
        egui::Event::PointerButton {
            pos: egui::Pos2::ZERO,
            button,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn back_is_consumed_once_even_with_text_focus() {
        for event in [escape(), mouse(egui::PointerButton::Extra1)] {
            let ctx = egui::Context::default();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events: vec![event],
                    ..Default::default()
                },
                |ui| {
                    ui.text_edit_singleline(&mut String::new()).request_focus();
                    assert!(back_requested(ui.ctx()));
                    assert!(!back_requested(ui.ctx()));
                },
            );
            output.textures_delta.clear();
        }
    }

    #[test]
    fn back_closes_popup_without_closing_parent() {
        for event in [escape(), mouse(egui::PointerButton::Extra1)] {
            let ctx = egui::Context::default();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events: vec![event],
                    ..Default::default()
                },
                |ui| {
                    egui::Popup::open_id(ui.ctx(), egui::Id::new("picker"));
                    assert!(!back_requested(ui.ctx()));
                    assert!(!egui::Popup::is_any_open(ui.ctx()));
                    assert!(!back_requested(ui.ctx()));
                },
            );
            output.textures_delta.clear();
        }
    }

    #[test]
    fn other_mouse_buttons_do_not_cancel() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![mouse(egui::PointerButton::Extra2)],
                ..Default::default()
            },
            |ui| {
                assert!(!back_requested(ui.ctx()));
                assert_eq!(ui.input(|i| i.events.len()), 1);
            },
        );
        output.textures_delta.clear();
    }
}

pub(super) fn clip_kind(clip: &Clip) -> &'static str {
    if clip.formats.iter().any(|(format, _)| format == "15") {
        "Files"
    } else if !clip.png.is_empty() {
        "Image"
    } else if !clip.text.is_empty() {
        "Text"
    } else {
        "Data"
    }
}

pub(super) fn clip_icon(ui: &mut egui::Ui, kind: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(24., 24.), egui::Sense::hover());
    let painter = ui.painter();
    let stroke = egui::Stroke::new(1.5, egui::Color32::from_rgb(134, 193, 221));
    let paper = egui::Rect::from_min_size(rect.min + egui::vec2(4., 2.), egui::vec2(16., 20.));
    match kind {
        "Image" => {
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
        "Files" => {
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
        "Text" => {
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
pub(super) fn secret_button(ui: &mut egui::Ui, icon: &str, hint: &str) -> egui::Response {
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

pub(super) fn app_list(
    ui: &mut egui::Ui,
    language: crate::i18n::Language,
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
            if ui.small_button(language.text("Delete")).clicked() {
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
    if adding.is_none() && ui.button(language.text("+ Add application")).clicked() {
        *adding = Some(String::new());
    }
    let mut close = false;
    if let Some(value) = adding.as_mut() {
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(value)
                    .hint_text(language.text("Name.exe or full path"))
                    .desired_width(230.),
            );
            if (ui.button(language.text("Add")).clicked()
                || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))))
                && !value.trim().is_empty()
            {
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
            } else if ui.button(language.text("Cancel")).clicked() {
                close = true;
            }
        });
        if !close {
            ui.label(language.text("Recently active applications:"));
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
pub(super) fn secret_app_picker(
    ui: &mut egui::Ui,
    language: crate::i18n::Language,
    app: &mut String,
    recent: &[String],
    icons: &mut HashMap<String, egui::TextureHandle>,
) {
    ui.horizontal(|ui| {
        ui.label(if app.is_empty() {
            language.text("Application: none selected")
        } else {
            app.as_str()
        });
        ui.menu_button(language.text("Choose application…"), |ui| {
            ui.label(language.text("Name.exe or full path"));
            ui.text_edit_singleline(app);
            ui.separator();
            ui.label(language.text("Last 5 applications"));
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
                .on_hover_text(language.text("Remove association"))
                .clicked()
        {
            app.clear();
        }
    });
}
