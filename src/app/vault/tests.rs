use super::*;

#[test]
fn secret_navigation_follows_visible_order_and_stops_at_edges() {
    let visible = [2, 5, 9];
    assert_eq!(
        navigate_secrets(&visible, Some(2), Some(egui::Key::ArrowDown)),
        Some(5)
    );
    assert_eq!(
        navigate_secrets(&visible, Some(9), Some(egui::Key::ArrowUp)),
        Some(5)
    );
    assert_eq!(
        navigate_secrets(&visible, Some(2), Some(egui::Key::ArrowUp)),
        Some(2)
    );
    assert_eq!(
        navigate_secrets(&visible, Some(9), Some(egui::Key::ArrowDown)),
        Some(9)
    );
    assert_eq!(navigate_secrets(&visible, Some(5), None), Some(5));
}

#[test]
fn secret_navigation_resets_selection_after_filtering() {
    assert_eq!(navigate_secrets(&[3, 7], Some(1), None), Some(3));
    assert_eq!(
        navigate_secrets(&[3, 7], None, Some(egui::Key::ArrowDown)),
        Some(3)
    );
    assert_eq!(
        navigate_secrets(&[], Some(1), Some(egui::Key::ArrowDown)),
        None
    );
    assert_eq!(
        navigate_secrets(&[7], Some(7), Some(egui::Key::ArrowUp)),
        Some(7)
    );
}

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
