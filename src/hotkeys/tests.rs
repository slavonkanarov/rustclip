use super::*;

#[test]
fn parses_arrow_shortcut() {
    let hotkey = parse("Ctrl + Shift + ↑").unwrap();
    assert_eq!(hotkey.modifiers & MOD_CONTROL, MOD_CONTROL);
    assert_eq!(hotkey.key, VK_UP as u32);
}

#[test]
fn rejects_duplicates_and_bare_keys() {
    assert!(parse("D").is_err());
    let mut settings = Settings::default();
    settings.hotkey_next = "Control + Shift + Up".into();
    assert!(validate(&settings).is_err());
}

#[test]
fn selection_shortcut_is_registered_and_validated() {
    let mut settings = Settings::default();
    assert_eq!(bindings(&settings)[4], (5, "Ctrl+Shift+T"));
    assert!(validate(&settings).is_ok());
    settings.hotkey_layout = settings.hotkey_open.clone();
    assert!(validate(&settings).is_err());
}
