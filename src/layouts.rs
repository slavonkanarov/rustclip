use std::collections::HashMap;
use windows_sys::Win32::{
    Globalization::{GetLocaleInfoW, LOCALE_SLOCALIZEDDISPLAYNAME},
    UI::Input::KeyboardAndMouse::*,
};

pub struct Layout {
    pub id: u32,
    pub name: String,
    handle: HKL,
}

pub fn installed() -> Result<Vec<Layout>, String> {
    let count = unsafe { GetKeyboardLayoutList(0, std::ptr::null_mut()) };
    if count <= 0 {
        return Err("Could not list keyboard layouts".into());
    }
    let mut handles = vec![std::ptr::null_mut(); count as usize];
    let count = unsafe { GetKeyboardLayoutList(count, handles.as_mut_ptr()) };
    if count <= 0 {
        return Err("Could not list keyboard layouts".into());
    }
    handles.truncate(count as usize);
    handles
        .into_iter()
        .map(|handle| {
            let id = handle as usize as u32;
            let mut name = [0u16; 256];
            let len = unsafe {
                GetLocaleInfoW(
                    id & 0xffff,
                    LOCALE_SLOCALIZEDDISPLAYNAME,
                    name.as_mut_ptr(),
                    name.len() as i32,
                )
            };
            if len <= 0 {
                return Err("Could not read keyboard layout name".into());
            }
            Ok(Layout {
                id,
                name: format!(
                    "{} ({id:08X})",
                    String::from_utf16_lossy(&name[..len as usize - 1])
                ),
                handle,
            })
        })
        .collect()
}

pub fn validate(first: u32, second: u32) -> Result<(), String> {
    if first == second {
        return Err("Choose two different keyboard layouts".into());
    }
    let layouts = installed()?;
    if [first, second]
        .iter()
        .any(|id| !layouts.iter().any(|layout| layout.id == *id))
    {
        return Err("Choose two installed keyboard layouts in Settings > Hotkeys".into());
    }
    Ok(())
}

fn character(layout: HKL, scan: u32, shifted: bool, altgr: bool) -> Option<char> {
    let vk = unsafe { MapVirtualKeyExW(scan, MAPVK_VSC_TO_VK_EX, layout) };
    if vk == 0 || vk > 255 {
        return None;
    }
    let mut state = [0u8; 256];
    if shifted {
        state[VK_SHIFT as usize] = 0x80;
    }
    if altgr {
        state[VK_CONTROL as usize] = 0x80;
        state[VK_MENU as usize] = 0x80;
    }
    let mut buffer = [0u16; 8];
    // Flag 4 prevents dead keys from changing the calling thread's keyboard state.
    let len = unsafe {
        ToUnicodeEx(
            vk,
            scan,
            state.as_ptr(),
            buffer.as_mut_ptr(),
            buffer.len() as i32,
            4,
            layout,
        )
    };
    if len <= 0 {
        return None;
    }
    let text = String::from_utf16(&buffer[..len as usize]).ok()?;
    let mut chars = text.chars();
    let ch = chars.next()?;
    (chars.next().is_none() && !ch.is_control()).then_some(ch)
}

pub struct Converter {
    first: u32,
    second: u32,
    pairs: Vec<(char, char)>,
}

impl Converter {
    pub fn new(first: u32, second: u32) -> Result<Self, String> {
        if first == second {
            return Err("Choose two different keyboard layouts".into());
        }
        let layouts = installed()?;
        let handle = |id| {
            layouts
                .iter()
                .find(|layout| layout.id == id)
                .map(|layout| layout.handle)
                .ok_or("Choose two installed keyboard layouts in Settings > Hotkeys")
        };
        let first_handle = handle(first)?;
        let second_handle = handle(second)?;
        let mut pairs = Vec::new();
        for altgr in [false, true] {
            for shifted in [false, true] {
                for scan in 1..=0x7f {
                    if let (Some(a), Some(b)) = (
                        character(first_handle, scan, shifted, altgr),
                        character(second_handle, scan, shifted, altgr),
                    ) {
                        pairs.push((a, b));
                    }
                }
            }
        }
        Ok(Self {
            first,
            second,
            pairs,
        })
    }

    pub fn convert(&self, text: &str, active: u32) -> Result<(String, u32), String> {
        let last = text
            .chars()
            .rfind(|ch| ch.is_alphabetic())
            .ok_or("Selection has no letter to detect its keyboard layout")?;
        let in_first = self.pairs.iter().any(|(a, _)| *a == last);
        let in_second = self.pairs.iter().any(|(_, b)| *b == last);
        let forward = match (in_first, in_second) {
            (true, false) => true,
            (false, true) => false,
            (true, true) if active == self.first => true,
            (true, true) if active == self.second => false,
            _ => return Err("Cannot determine the selection's keyboard layout".into()),
        };
        let mut mapping = HashMap::new();
        for &(a, b) in &self.pairs {
            let (from, to) = if forward { (a, b) } else { (b, a) };
            mapping.entry(from).or_insert(to);
        }
        let text = text
            .chars()
            .map(|ch| mapping.get(&ch).copied().unwrap_or(ch))
            .collect();
        Ok((text, if forward { self.second } else { self.first }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn converter() -> Converter {
        let a = "`qwertyuiop[]asdfghjkl;'zxcvbnm,./~QWERTYUIOP{}ASDFGHJKL:\"ZXCVBNM<>?";
        let b = "ёйцукенгшщзхъфывапролджэячсмитьбю.ЁЙЦУКЕНГШЩЗХЪФЫВАПРОЛДЖЭЯЧСМИТЬБЮ,";
        assert_eq!(a.chars().count(), b.chars().count());
        Converter {
            first: 1,
            second: 2,
            pairs: a.chars().zip(b.chars()).collect(),
        }
    }

    #[test]
    fn detects_last_letter_and_preserves_case_and_unknown_characters() {
        let c = converter();
        assert_eq!(c.convert("руддщ", 1).unwrap().0, "hello");
        assert_eq!(c.convert("Hello", 2).unwrap().0, "Руддщ");
        assert_eq!(
            c.convert("руддщ 123!\r\n🦀", 1).unwrap().0,
            "hello 123!\r\n🦀"
        );
        assert_eq!(c.convert("hello руддщ", 1).unwrap().0, "hello hello");
        assert!(c.convert("123!", 1).is_err());
        assert!(c.convert("你好", 1).is_err());
    }

    #[test]
    fn repeated_conversion_roundtrips() {
        let c = converter();
        let text = "Hello, world!\r\n";
        assert_eq!(
            c.convert(&c.convert(text, 1).unwrap().0, 2).unwrap().0,
            text
        );
    }

    #[test]
    fn ambiguous_letter_uses_active_layout() {
        let c = Converter {
            first: 1,
            second: 2,
            pairs: vec![('a', 'q'), ('q', 'w'), ('w', 'a')],
        };
        assert_eq!(c.convert("a", 1).unwrap().0, "q");
        assert_eq!(c.convert("a", 2).unwrap().0, "w");
        let (converted, destination) = c.convert("a", 1).unwrap();
        assert_eq!(c.convert(&converted, destination).unwrap().0, "a");
        assert!(c.convert("a", 3).is_err());
    }
}
