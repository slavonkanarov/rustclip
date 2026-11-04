use serde::{Deserialize, Serialize};
use std::{borrow::Cow, collections::HashMap, sync::OnceLock};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "en")]
    English,
    #[serde(rename = "ru")]
    Russian,
}

impl Language {
    pub const ALL: [Self; 2] = [Self::English, Self::Russian];

    pub fn name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Russian => Self::Russian.text("Russian"),
        }
    }

    // English source strings are stable catalog keys. Add a locale file and
    // a variant here to support another language.
    pub fn text<'a>(self, key: &'a str) -> &'a str {
        match self {
            Self::English => key,
            Self::Russian => {
                static RUSSIAN: OnceLock<HashMap<String, String>> = OnceLock::new();
                RUSSIAN
                    .get_or_init(|| {
                        serde_json::from_str(include_str!("i18n/ru.json"))
                            .expect("valid Russian catalog")
                    })
                    .get(key)
                    .map(String::as_str)
                    .unwrap_or(key)
            }
        }
    }

    pub fn message<'a>(self, message: &'a str) -> Cow<'a, str> {
        if self == Self::English {
            return Cow::Borrowed(message);
        }
        if let Some((first, rest)) = message.split_once('\n') {
            return Cow::Owned(format!("{}\n{}", self.message(first), self.message(rest)));
        }
        let translated = self.text(message);
        if translated != message {
            return Cow::Borrowed(translated);
        }
        if let Some((prefix, detail)) = message.split_once(": ") {
            let translated = self.text(prefix);
            if translated != prefix {
                return Cow::Owned(format!("{translated}: {detail}"));
            }
        }
        if let Some(detail) = message.strip_prefix("Failed to write format ") {
            return Cow::Owned(format!("{} {detail}", self.text("Failed to write format")));
        }
        if let Some(detail) = message.strip_prefix("Hotkey ")
            && let Some(key) = detail.strip_suffix(" is in use by another program")
        {
            return Cow::Owned(format!(
                "{} {key} {}",
                self.text("Hotkey"),
                self.text("is in use by another program")
            ));
        }
        Cow::Borrowed(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_names_and_fallback() {
        assert_eq!(Language::English.text("Settings"), "Settings");
        assert_eq!(Language::Russian.text("Settings"), "Настройки");
        assert_eq!(Language::English.text("RustClip"), "RustClip");
        assert_eq!(
            Language::Russian.message("Unknown key: F13"),
            "Неизвестная клавиша: F13"
        );
    }
}
