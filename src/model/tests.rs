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
    assert!(s.check_updates);
    assert!(!s.auto_update);
    assert!(!s.prerelease_updates);
    assert_eq!(s.paste_delay, 200);
    assert_eq!(s.language, Language::English);
}

#[test]
fn language_survives_settings_roundtrip() {
    let mut settings = Settings::default();
    settings.language = Language::Russian;
    let json = serde_json::to_value(&settings).unwrap();
    assert_eq!(json["language"], "ru");
    let restored: Settings = serde_json::from_value(json).unwrap();
    assert_eq!(restored.language, Language::Russian);
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

#[test]
fn image_clip_save_flushes_all_buffered_bytes() {
    let path = std::env::temp_dir().join(format!(
        "rustclip-image-save-test-{}.json",
        std::process::id()
    ));
    let clip = Clip {
        id: 1,
        formats: vec![("PNG".into(), (0..=255).cycle().take(1024 * 1024).collect())],
        png: include_bytes!("../../assets/icon.png").to_vec(),
        ..Default::default()
    };
    write_json(&path, &clip).unwrap();
    let saved: Clip = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved.id, clip.id);
    assert_eq!(saved.formats, clip.formats);
    assert_eq!(saved.png, clip.png);
    assert!(!path.with_extension("tmp").exists());
    fs::remove_file(path).unwrap();
}

#[test]
fn corrupt_clip_does_not_prevent_loading_healthy_history() {
    let root = std::env::temp_dir().join(format!("rustclip-load-test-{}", std::process::id()));
    let clips = root.join("clips");
    fs::create_dir_all(&clips).unwrap();
    fs::write(clips.join("1.json"), b"{broken").unwrap();
    let clip = Clip {
        id: 2,
        text: "healthy".into(),
        ..Default::default()
    };
    write_json(&clips.join("2.json"), &clip).unwrap();
    let (loaded, _, errors) = load_from(&root, MAX_HISTORY_BYTES);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].text, "healthy");
    assert!(errors.iter().any(|e| e.contains("1.json")));
    let (loaded, _, errors) = load_from(&root, 1);
    assert!(loaded.is_empty());
    assert!(errors.iter().any(|e| e.contains("memory limit")));
    assert!(clips.join("2.json").exists());
    fs::remove_file(clips.join("1.json")).unwrap();
    fs::remove_file(clips.join("2.json")).unwrap();
    fs::remove_dir(clips).unwrap();
    fs::remove_dir(root).unwrap();
}

#[test]
fn unsafe_native_formats_and_overflowing_ids_are_rejected() {
    let mut clip = Clip {
        formats: vec![("2".into(), vec![1])],
        ..Default::default()
    };
    assert!(validate_clip(&clip).is_err());
    clip.formats = vec![("13".into(), vec![0, 0])];
    assert!(validate_clip(&clip).is_ok());
    clip.id = u128::MAX;
    assert!(validate_clip(&clip).is_err());
}

#[test]
fn oversized_json_is_rejected() {
    let path =
        std::env::temp_dir().join(format!("rustclip-json-limit-{}.json", std::process::id()));
    fs::write(&path, b"\"too long\"").unwrap();
    assert!(
        read_json::<String>(&path, 4)
            .unwrap_err()
            .contains("size limit")
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn corrupt_settings_are_preserved_before_falling_back_to_defaults() {
    let root =
        std::env::temp_dir().join(format!("rustclip-settings-recovery-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("settings.json");
    fs::write(&path, b"{broken").unwrap();
    let (_, settings, errors) = load_from(&root, MAX_HISTORY_BYTES);
    assert!(settings == Settings::default());
    assert!(
        errors
            .iter()
            .any(|e| e.starts_with("Settings recovery copy:"))
    );
    let copies: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("settings.corrupt-")
        })
        .collect();
    assert_eq!(copies.len(), 1);
    assert_eq!(fs::read(&copies[0]).unwrap(), b"{broken");
    fs::remove_file(&copies[0]).unwrap();
    fs::remove_file(path).unwrap();
    fs::remove_dir(root).unwrap();
}
