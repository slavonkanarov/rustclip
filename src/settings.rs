use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

pub const ALT: u8 = 1;
pub const CTRL: u8 = 2;
pub const SHIFT: u8 = 4;
pub const WIN: u8 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    OpenHistory,
    OpenSettings,
    OpenTemplates,
    NextEntry,
    PreviousEntry,
    CopySelected,
    PasteSelected,
    DeleteSelected,
    FocusSearch,
    BrowseOlder,
    BrowseNewer,
    CloseView,
    CancelQuick,
}

impl Action {
    pub const ALL: [Self; 13] = [
        Self::OpenHistory,
        Self::OpenSettings,
        Self::OpenTemplates,
        Self::NextEntry,
        Self::PreviousEntry,
        Self::CopySelected,
        Self::PasteSelected,
        Self::DeleteSelected,
        Self::FocusSearch,
        Self::BrowseOlder,
        Self::BrowseNewer,
        Self::CloseView,
        Self::CancelQuick,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::OpenHistory => "Open history",
            Self::OpenSettings => "Open settings",
            Self::OpenTemplates => "Open templates",
            Self::NextEntry => "Next entry",
            Self::PreviousEntry => "Previous entry",
            Self::CopySelected => "Copy selected",
            Self::PasteSelected => "Paste selected",
            Self::DeleteSelected => "Delete selected",
            Self::FocusSearch => "Focus search",
            Self::BrowseOlder => "Select older clip",
            Self::BrowseNewer => "Select newer clip",
            Self::CloseView => "Close view",
            Self::CancelQuick => "Cancel quick selection",
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::OpenHistory => "open_history",
            Self::OpenSettings => "open_settings",
            Self::OpenTemplates => "open_templates",
            Self::NextEntry => "next_entry",
            Self::PreviousEntry => "previous_entry",
            Self::CopySelected => "copy_selected",
            Self::PasteSelected => "paste_selected",
            Self::DeleteSelected => "delete_selected",
            Self::FocusSearch => "focus_search",
            Self::BrowseOlder => "browse_older",
            Self::BrowseNewer => "browse_newer",
            Self::CloseView => "close_view",
            Self::CancelQuick => "cancel_quick",
        }
    }

    pub const fn global(self) -> bool {
        matches!(
            self,
            Self::OpenHistory
                | Self::OpenSettings
                | Self::OpenTemplates
                | Self::BrowseOlder
                | Self::BrowseNewer
                | Self::CancelQuick
        )
    }

    pub const fn optional(self) -> bool {
        matches!(self, Self::OpenSettings | Self::OpenTemplates)
    }

    pub const GLOBAL: [Self; 6] = [
        Self::OpenHistory,
        Self::OpenSettings,
        Self::OpenTemplates,
        Self::BrowseOlder,
        Self::BrowseNewer,
        Self::CancelQuick,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Binding {
    pub key: u32,
    pub modifiers: u8,
}

impl Binding {
    pub const fn new(key: u32, modifiers: u8) -> Self {
        Self { key, modifiers }
    }

    pub fn label(self) -> String {
        if self.key == 0 {
            return "Not set".into();
        }
        let mut parts = Vec::new();
        for (flag, name) in [(WIN, "Win"), (CTRL, "Ctrl"), (ALT, "Alt"), (SHIFT, "Shift")] {
            if self.modifiers & flag != 0 {
                parts.push(name);
            }
        }
        parts.push(key_name(self.key));
        parts.join(" + ")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Settings {
    pub bindings: [Binding; 13],
    pub start_hidden: bool,
    pub start_with_windows: bool,
    pub paste_enabled: bool,
    pub restore_clipboard: bool,
    pub paste_delay_ms: u32,
    pub history_limit_mb: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            bindings: [
                Binding::new(b'H' as u32, WIN | SHIFT),
                Binding::new(0, 0),
                Binding::new(0, 0),
                Binding::new(0x28, 0),
                Binding::new(0x26, 0),
                Binding::new(0x0d, 0),
                Binding::new(0x0d, CTRL | SHIFT),
                Binding::new(0x2e, CTRL | SHIFT),
                Binding::new(b'F' as u32, CTRL),
                Binding::new(0x26, ALT | SHIFT),
                Binding::new(0x28, ALT | SHIFT),
                Binding::new(0x1b, 0),
                Binding::new(0x1b, ALT | SHIFT),
            ],
            start_hidden: true,
            start_with_windows: false,
            paste_enabled: true,
            restore_clipboard: false,
            paste_delay_ms: 150,
            history_limit_mb: (crate::store::DEFAULT_LIMIT / (1024 * 1024)) as u32,
        }
    }
}

impl Settings {
    pub fn binding(&self, action: Action) -> Binding {
        self.bindings[action.index()]
    }

    pub fn validate(&self) -> Result<(), String> {
        if !(16..=4096).contains(&self.history_limit_mb) {
            return Err("History limit must be 16–4096 MiB.".into());
        }
        if self.paste_delay_ms > 2000 {
            return Err("Paste delay must be 0–2000 ms.".into());
        }
        for action in Action::ALL {
            let binding = self.binding(action);
            if binding.key == 0 && action.optional() {
                if binding.modifiers != 0 {
                    return Err(format!("Choose a key or clear {}.", action.name()));
                }
                continue;
            }
            if !valid_key(binding.key) {
                return Err(format!("Choose a key for {}.", action.name()));
            }
            if binding.modifiers & !0x0f != 0 {
                return Err(format!("Invalid modifiers for {}.", action.name()));
            }
            if action.global() && binding.modifiers == 0 {
                return Err(format!("{} needs at least one modifier.", action.name()));
            }
            for other in Action::ALL {
                if action.index() < other.index()
                    && binding == self.binding(other)
                    && action.global() == other.global()
                {
                    return Err(format!(
                        "{} conflicts with {}.",
                        action.name(),
                        other.name()
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        let mut settings = Self::default();
        let mut version = 1;
        for line in content.lines() {
            let Some((name, value)) = line.split_once('=') else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Invalid settings format",
                ));
            };
            match name {
                "settings_version" => {
                    version = parse_number(value)?;
                    continue;
                }
                "start_hidden" => {
                    settings.start_hidden = parse_bool(value)?;
                    continue;
                }
                "start_with_windows" => {
                    settings.start_with_windows = parse_bool(value)?;
                    continue;
                }
                "paste_enabled" => {
                    settings.paste_enabled = parse_bool(value)?;
                    continue;
                }
                "restore_clipboard" => {
                    settings.restore_clipboard = parse_bool(value)?;
                    continue;
                }
                "paste_delay_ms" => {
                    settings.paste_delay_ms = parse_number(value)?;
                    continue;
                }
                "history_limit_mb" => {
                    settings.history_limit_mb = parse_number(value)?;
                    continue;
                }
                _ => {}
            }
            let Some(action) = Action::ALL.into_iter().find(|action| action.key() == name) else {
                continue;
            };
            let Some((key, modifiers)) = value.split_once(',') else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Invalid shortcut",
                ));
            };
            let key = key
                .parse::<u32>()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid shortcut key"))?;
            let modifiers = modifiers.parse::<u8>().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "Invalid shortcut modifiers")
            })?;
            settings.bindings[action.index()] = Binding::new(key, modifiers);
        }
        if version < 2 {
            if settings.binding(Action::OpenHistory) == Binding::new(b'J' as u32, WIN | SHIFT) {
                settings.bindings[Action::OpenHistory.index()] =
                    Binding::new(b'H' as u32, WIN | SHIFT);
            }
            for (action, key) in [(Action::OpenSettings, b'K'), (Action::OpenTemplates, b'Y')] {
                if settings.binding(action) == Binding::new(key as u32, WIN | SHIFT) {
                    settings.bindings[action.index()] = Binding::new(0, 0);
                }
            }
        }
        settings
            .validate()
            .map_err(|message| io::Error::new(io::ErrorKind::InvalidData, message))?;
        Ok(settings)
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        self.validate()
            .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid settings path"))?;
        fs::create_dir_all(parent)?;
        let temporary = path.with_extension("tmp");
        let mut file = File::create(&temporary)?;
        writeln!(file, "settings_version=3")?;
        for action in Action::ALL {
            let binding = self.binding(action);
            writeln!(
                file,
                "{}={},{}",
                action.key(),
                binding.key,
                binding.modifiers
            )?;
        }
        writeln!(file, "start_hidden={}", u8::from(self.start_hidden))?;
        writeln!(
            file,
            "start_with_windows={}",
            u8::from(self.start_with_windows)
        )?;
        writeln!(file, "paste_enabled={}", u8::from(self.paste_enabled))?;
        writeln!(
            file,
            "restore_clipboard={}",
            u8::from(self.restore_clipboard)
        )?;
        writeln!(file, "paste_delay_ms={}", self.paste_delay_ms)?;
        writeln!(file, "history_limit_mb={}", self.history_limit_mb)?;
        file.sync_all()?;
        drop(file);
        fs::rename(temporary, path)
    }
}

fn parse_bool(value: &str) -> io::Result<bool> {
    match value {
        "1" => Ok(true),
        "0" => Ok(false),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid boolean setting",
        )),
    }
}

fn parse_number(value: &str) -> io::Result<u32> {
    value
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid numeric setting"))
}

pub fn valid_key(key: u32) -> bool {
    matches!(key, 0x08 | 0x09 | 0x0d | 0x1b | 0x20 | 0x21..=0x28 | 0x2d | 0x2e | 0x30..=0x39 | 0x41..=0x5a | 0x70..=0x87)
}

pub fn key_name(key: u32) -> &'static str {
    match key {
        0 => "None",
        0x08 => "Backspace",
        0x09 => "Tab",
        0x0d => "Enter",
        0x1b => "Esc",
        0x20 => "Space",
        0x21 => "Page Up",
        0x22 => "Page Down",
        0x23 => "End",
        0x24 => "Home",
        0x25 => "Left",
        0x26 => "Up",
        0x27 => "Right",
        0x28 => "Down",
        0x2d => "Insert",
        0x2e => "Delete",
        0x30 => "0",
        0x31 => "1",
        0x32 => "2",
        0x33 => "3",
        0x34 => "4",
        0x35 => "5",
        0x36 => "6",
        0x37 => "7",
        0x38 => "8",
        0x39 => "9",
        0x41 => "A",
        0x42 => "B",
        0x43 => "C",
        0x44 => "D",
        0x45 => "E",
        0x46 => "F",
        0x47 => "G",
        0x48 => "H",
        0x49 => "I",
        0x4a => "J",
        0x4b => "K",
        0x4c => "L",
        0x4d => "M",
        0x4e => "N",
        0x4f => "O",
        0x50 => "P",
        0x51 => "Q",
        0x52 => "R",
        0x53 => "S",
        0x54 => "T",
        0x55 => "U",
        0x56 => "V",
        0x57 => "W",
        0x58 => "X",
        0x59 => "Y",
        0x5a => "Z",
        0x70 => "F1",
        0x71 => "F2",
        0x72 => "F3",
        0x73 => "F4",
        0x74 => "F5",
        0x75 => "F6",
        0x76 => "F7",
        0x77 => "F8",
        0x78 => "F9",
        0x79 => "F10",
        0x7a => "F11",
        0x7b => "F12",
        0x7c => "F13",
        0x7d => "F14",
        0x7e => "F15",
        0x7f => "F16",
        0x80 => "F17",
        0x81 => "F18",
        0x82 => "F19",
        0x83 => "F20",
        0x84 => "F21",
        0x85 => "F22",
        0x86 => "F23",
        0x87 => "F24",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn defaults_offer_quick_selection_without_settings_or_templates_hotkeys() {
        let settings = Settings::default();
        assert_eq!(
            settings.binding(Action::OpenHistory).label(),
            "Win + Shift + H"
        );
        assert_eq!(
            settings.binding(Action::BrowseOlder).label(),
            "Alt + Shift + Up"
        );
        assert_eq!(
            settings.binding(Action::BrowseNewer).label(),
            "Alt + Shift + Down"
        );
        assert_eq!(settings.binding(Action::OpenSettings).label(), "Not set");
        assert_eq!(settings.binding(Action::OpenTemplates).label(), "Not set");
        assert!(Action::GLOBAL.iter().all(|action| action.global()));
        settings.validate().unwrap();
    }

    #[test]
    fn duplicate_bindings_are_rejected() {
        let mut settings = Settings::default();
        settings.bindings[Action::OpenSettings.index()] = settings.binding(Action::OpenHistory);
        assert!(settings.validate().unwrap_err().contains("conflicts"));
    }

    #[test]
    fn storage_limit_and_paste_delay_are_bounded() {
        let mut settings = Settings::default();
        settings.history_limit_mb = 15;
        assert!(settings.validate().unwrap_err().contains("History limit"));
        settings.history_limit_mb = 64;
        settings.paste_delay_ms = 2001;
        assert!(settings.validate().unwrap_err().contains("Paste delay"));
    }

    #[test]
    fn win_flag_round_trips_through_settings_file() {
        let path = std::env::temp_dir().join(format!(
            "rustclip-settings-{}-{}.conf",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut settings = Settings::default();
        settings.start_hidden = false;
        settings.start_with_windows = true;
        settings.restore_clipboard = true;
        settings.paste_delay_ms = 200;
        settings.history_limit_mb = 128;
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), settings);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn legacy_global_defaults_migrate_without_discarding_other_settings() {
        let path = std::env::temp_dir().join(format!(
            "rustclip-legacy-settings-{}.conf",
            std::process::id()
        ));
        fs::write(
            &path,
            "open_history=74,12\nopen_settings=75,12\nopen_templates=89,12\nhistory_limit_mb=128\n",
        )
        .unwrap();
        let settings = Settings::load(&path).unwrap();
        assert_eq!(
            settings.binding(Action::OpenHistory).label(),
            "Win + Shift + H"
        );
        assert_eq!(
            settings.binding(Action::BrowseNewer).label(),
            "Alt + Shift + Down"
        );
        assert_eq!(settings.binding(Action::OpenSettings).label(), "Not set");
        assert_eq!(settings.binding(Action::OpenTemplates).label(), "Not set");
        assert_eq!(settings.history_limit_mb, 128);
        fs::remove_file(path).unwrap();
    }
}
