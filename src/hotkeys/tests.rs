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
