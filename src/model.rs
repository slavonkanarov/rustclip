use crate::i18n::Language;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
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
    pub fn memory_bytes(&self) -> usize {
        self.formats.iter().fold(
            self.text
                .len()
                .saturating_add(self.source.len())
                .saturating_add(self.label.len())
                .saturating_add(self.png.len()),
            |total, (name, bytes)| total.saturating_add(name.len()).saturating_add(bytes.len()),
        )
    }
    pub fn title(&self, language: Language) -> String {
        if !self.text.is_empty() {
            self.text
                .replace(['\r', '\n'], " ")
                .chars()
                .take(70)
                .collect()
        } else if !self.png.is_empty() {
            language.text("Picture").into()
        } else {
            format!(
                "{} {}",
                language.text("Data:"),
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
    pub language: Language,
    pub autostart: bool,
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
    pub hotkey_layout: String,
    pub layout_first: u32,
    pub layout_second: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::English,
            autostart: false,
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
            hotkey_layout: "Ctrl+Shift+T".into(),
            layout_first: 0x04090409,
            layout_second: 0x04190419,
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
    validate_clip(clip)?;
    write_json(
        &directory().join("clips").join(format!("{}.json", clip.id)),
        clip,
    )
}
pub const MAX_HISTORY_BYTES: usize = 256 * 1024 * 1024;

pub fn validate_clip(clip: &Clip) -> Result<(), String> {
    if clip.id == u128::MAX || clip.memory_bytes() > 64 * 1024 * 1024 {
        return Err("Invalid clip ID or clip exceeds the 64 MiB limit".into());
    }
    if clip.formats.len() > 64
        || clip.formats.iter().any(|(name, bytes)| {
            !matches!(
                name.as_str(),
                "1" | "7"
                    | "8"
                    | "13"
                    | "15"
                    | "16"
                    | "17"
                    | "HTML Format"
                    | "Rich Text Format"
                    | "PNG"
            ) || bytes.len() > 32 * 1024 * 1024
        })
    {
        return Err("Unsupported clipboard format or format exceeds the 32 MiB limit".into());
    }
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(
    path: &std::path::Path,
    max: u64,
) -> Result<T, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > max {
        return Err("JSON file exceeds the size limit".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
pub fn write_json(path: &std::path::Path, value: &impl Serialize) -> Result<(), String> {
    fs::create_dir_all(path.parent().ok_or("No parent directory")?).map_err(|e| e.to_string())?;
    let temp = path.with_extension("tmp");
    let mut file = fs::File::create(&temp).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, value).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
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
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}
pub fn load() -> (Vec<Clip>, Settings, Vec<String>) {
    let result = load_from(&directory(), MAX_HISTORY_BYTES);
    for error in &result.2 {
        crate::diagnostics::log(&format!("Loading data: {error}"));
    }
    result
}

fn load_from(directory: &std::path::Path, budget: usize) -> (Vec<Clip>, Settings, Vec<String>) {
    let mut errors = Vec::new();
    let settings_path = directory.join("settings.json");
    let settings = if !settings_path.exists() {
        Settings::default()
    } else {
        read_json(&settings_path, 1024 * 1024).unwrap_or_else(|e| {
            errors.push(format!("Settings: {e}"));
            let copy = directory.join(format!("settings.corrupt-{}.json", now()));
            match fs::copy(&settings_path, &copy) {
                Ok(_) => errors.push(format!("Settings recovery copy: {}", copy.display())),
                Err(e) => errors.push(format!("Could not preserve damaged settings: {e}")),
            }
            Settings::default()
        })
    };
    let mut clips: Vec<Clip> = Vec::new();
    let mut used = 0usize;
    let mut skipped = 0;
    match fs::read_dir(directory.join("clips")) {
        Ok(entries) => {
            let mut paths = Vec::new();
            for entry in entries {
                match entry {
                    Ok(entry) if entry.path().extension().is_some_and(|e| e == "json") => {
                        paths.push(entry.path())
                    }
                    Ok(_) => {}
                    Err(e) => errors.push(format!("Read history directory: {e}")),
                }
            }
            paths.sort_by_key(|path| {
                std::cmp::Reverse(
                    path.file_stem()
                        .and_then(|s| s.to_str())
                        .and_then(|s| s.parse::<u128>().ok())
                        .unwrap_or_default(),
                )
            });
            for path in paths {
                match read_json::<Clip>(&path, 256 * 1024 * 1024).and_then(|clip| {
                    validate_clip(&clip)?;
                    Ok(clip)
                }) {
                    Ok(clip) if used.saturating_add(clip.memory_bytes()) <= budget => {
                        used += clip.memory_bytes();
                        clips.push(clip);
                    }
                    Ok(_) => skipped += 1,
                    Err(e) => errors.push(format!("{}: {e}", path.display())),
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => errors.push(format!("Read history directory: {e}")),
    }
    if skipped > 0 {
        errors.push(format!(
            "History memory limit: {skipped} clips not loaded; files remain on disk"
        ));
    }
    clips.sort_by_key(|c| c.id);
    (clips, settings, errors)
}

#[cfg(test)]
mod tests;
