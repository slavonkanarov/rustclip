use serde::{Deserialize, Serialize};
use std::{
    fs,
    os::windows::ffi::OsStrExt,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Clip {
    pub id: u128,
    pub text: String,
    pub source: String,
    pub formats: Vec<(String, Vec<u8>)>,
    pub png: Vec<u8>,
    pub starred: bool,
    pub label: String,
    pub snippet: bool,
}

impl Clip {
    pub fn title(&self) -> String {
        if !self.text.is_empty() {
            self.text
                .replace(['\r', '\n'], " ")
                .chars()
                .take(70)
                .collect()
        } else if !self.png.is_empty() {
            "Картинка".into()
        } else {
            format!(
                "Данные: {}",
                self.formats
                    .iter()
                    .map(|f| f.0.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
    pub fn same_content(&self, other: &Self) -> bool {
        self.formats == other.formats
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub monitor: bool,
    pub auto_paste: bool,
    pub restore: bool,
    pub ctrl_v: bool,
    pub hide_after: bool,
    pub limit: usize,
    pub duplicates: usize,
    pub paste_delay: u64,
    pub preview_ms: u64,
    pub max_text_kb: usize,
    pub newest_first: bool,
    pub excluded_capture: String,
    pub excluded_hotkeys: String,
    pub recent_apps: Vec<String>,
    pub excluded_formats: String,
    pub hotkey_open: String,
    pub hotkey_previous: String,
    pub hotkey_next: String,
    pub hotkey_plain_paste: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            monitor: true,
            auto_paste: true,
            restore: true,
            ctrl_v: false,
            hide_after: true,
            limit: 50000,
            duplicates: 5,
            paste_delay: 200,
            preview_ms: 0,
            max_text_kb: 100,
            newest_first: false,
            excluded_capture: "FactoryGame.exe\nPioneerGame.exe".into(),
            excluded_hotkeys: "rustclip.exe\nPioneerGame.exe".into(),
            recent_apps: Vec::new(),
            excluded_formats: String::new(),
            hotkey_open: "Ctrl+Shift+D".into(),
            hotkey_previous: "Ctrl+Shift+Up".into(),
            hotkey_next: "Ctrl+Shift+Down".into(),
            hotkey_plain_paste: "Ctrl+Shift+V".into(),
        }
    }
}
pub fn now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
pub fn directory() -> PathBuf {
    if let Some(path) = std::env::var_os("RUSTCLIP_DATA_DIR") {
        return PathBuf::from(path);
    }
    PathBuf::from(std::env::var_os("APPDATA").unwrap_or_else(|| ".".into())).join("RustClip")
}
pub fn export_clip(clip: &Clip) -> Result<PathBuf, String> {
    let folder = directory().join("exports");
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let (extension, bytes) = if !clip.png.is_empty() {
        ("png", clip.png.as_slice())
    } else {
        ("txt", clip.text.as_bytes())
    };
    let path = folder.join(format!("{}-{}.{}", clip.id, now(), extension));
    fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path)
}
pub fn backup(clips: &[Clip], settings: &Settings) -> Result<PathBuf, String> {
    let path = directory()
        .join("Backup")
        .join(format!("backup-{}.json", now()));
    write_json(&path, &(clips, settings))?;
    Ok(path)
}
pub fn save_clip(clip: &Clip) -> Result<(), String> {
    write_json(
        &directory().join("clips").join(format!("{}.json", clip.id)),
        clip,
    )
}
pub fn write_json(path: &std::path::Path, value: &impl Serialize) -> Result<(), String> {
    fs::create_dir_all(path.parent().ok_or("No parent directory")?).map_err(|e| e.to_string())?;
    let temp = path.with_extension("tmp");
    fs::write(&temp, serde_json::to_vec(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MoveFileExW};
    let wide = |path: &std::path::Path| {
        path.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    if unsafe {
        MoveFileExW(
            wide(&temp).as_ptr(),
            wide(path).as_ptr(),
            MOVEFILE_REPLACE_EXISTING,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}
pub fn load() -> (Vec<Clip>, Settings, Vec<String>) {
    let mut errors = Vec::new();
    let settings = match fs::read(directory().join("settings.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            errors.push(format!("Настройки: {e}"));
            Settings::default()
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Settings::default(),
        Err(e) => {
            errors.push(e.to_string());
            Settings::default()
        }
    };
    let mut clips: Vec<Clip> = Vec::new();
    if let Ok(entries) = fs::read_dir(directory().join("clips")) {
        for entry in entries
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|e| e == "json"))
        {
            match fs::read(entry.path())
                .map_err(|e| e.to_string())
                .and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string()))
            {
                Ok(clip) => clips.push(clip),
                Err(e) => errors.push(format!("{}: {e}", entry.path().display())),
            }
        }
    }
    clips.sort_by_key(|c| c.id);
    (clips, settings, errors)
}

#[cfg(test)]
mod tests {
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
        write_json(&path, &settings).unwrap();
        let saved: Settings = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.excluded_hotkeys, "Code.exe");
        fs::remove_file(path).unwrap();
    }
}
