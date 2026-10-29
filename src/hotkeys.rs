use crate::model::Settings;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

pub struct Hotkey {
    pub modifiers: u32,
    pub key: u32,
}

pub fn parse(value: &str) -> Result<Hotkey, String> {
    let mut modifiers = MOD_NOREPEAT;
    let mut key = None;
    for part in value.split('+') {
        let part = part.trim().to_ascii_uppercase();
        match part.as_str() {
            "CTRL" | "CONTROL" => modifiers |= MOD_CONTROL,
            "SHIFT" => modifiers |= MOD_SHIFT,
            "ALT" => modifiers |= MOD_ALT,
            "WIN" | "META" => modifiers |= MOD_WIN,
            _ => {
                if key.is_some() {
                    return Err(format!("Лишняя клавиша в комбинации: {value}"));
                }
                key = Some(match part.as_str() {
                    "UP" | "↑" => VK_UP as u32,
                    "DOWN" | "↓" => VK_DOWN as u32,
                    "LEFT" | "←" => VK_LEFT as u32,
                    "RIGHT" | "→" => VK_RIGHT as u32,
                    "SPACE" => VK_SPACE as u32,
                    "TAB" => VK_TAB as u32,
                    "ESC" | "ESCAPE" => VK_ESCAPE as u32,
                    "ENTER" | "RETURN" => VK_RETURN as u32,
                    "INSERT" | "INS" => VK_INSERT as u32,
                    "DELETE" | "DEL" => VK_DELETE as u32,
                    "HOME" => VK_HOME as u32,
                    "END" => VK_END as u32,
                    "PAGEUP" | "PGUP" => VK_PRIOR as u32,
                    "PAGEDOWN" | "PGDN" => VK_NEXT as u32,
                    _ if part.len() == 1 && part.as_bytes()[0].is_ascii_alphanumeric() => {
                        part.as_bytes()[0] as u32
                    }
                    _ if part.starts_with('F') => match part[1..].parse::<u32>() {
                        Ok(n @ 1..=24) => VK_F1 as u32 + n - 1,
                        _ => return Err(format!("Неизвестная клавиша: {part}")),
                    },
                    _ => return Err(format!("Неизвестная клавиша: {part}")),
                });
            }
        }
    }
    if modifiers == MOD_NOREPEAT || key.is_none() {
        return Err(format!("Нужны модификатор и клавиша: {value}"));
    }
    Ok(Hotkey {
        modifiers,
        key: key.unwrap(),
    })
}

pub fn bindings(settings: &Settings) -> [(i32, &str); 4] {
    [
        (1, &settings.hotkey_open),
        (2, &settings.hotkey_previous),
        (3, &settings.hotkey_next),
        (4, &settings.hotkey_plain_paste),
    ]
}

pub fn validate(settings: &Settings) -> Result<(), String> {
    let mut used = Vec::new();
    for (_, value) in bindings(settings) {
        let hotkey = parse(value)?;
        let pair = (hotkey.modifiers & !MOD_NOREPEAT, hotkey.key);
        if used.contains(&pair) {
            return Err(format!("Повторяющаяся комбинация: {value}"));
        }
        used.push(pair);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
