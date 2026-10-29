use super::*;
#[test]
fn duplicate_ignores_metadata_but_not_formats() {
    let a = Clip {
        formats: vec![("13".into(), vec![1, 2])],
        ..Default::default()
    };
    let mut b = a.clone();
    b.starred = true;
    b.id = 42;
    assert!(a.same_content(&b));
    b.formats.push(("HTML Format".into(), vec![3]));
    assert!(!a.same_content(&b));
}
#[test]
fn old_settings_gain_defaults() {
    let s: Settings = serde_json::from_str("{\"monitor\":false}").unwrap();
    assert!(!s.monitor);
    assert!(!s.autostart);
    assert_eq!(s.paste_delay, 200);
}
#[test]
fn settings_can_be_saved_again() {
    let path = std::env::temp_dir().join(format!(
        "rustclip-settings-test-{}.json",
        std::process::id()
    ));
    let mut settings = Settings::default();
    write_json(&path, &settings).unwrap();
    settings.excluded_hotkeys = "Code.exe".into();
    settings.autostart = true;
    write_json(&path, &settings).unwrap();
    let saved: Settings = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved.excluded_hotkeys, "Code.exe");
    assert!(saved.autostart);
    fs::remove_file(path).unwrap();
}
