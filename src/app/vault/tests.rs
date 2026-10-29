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
