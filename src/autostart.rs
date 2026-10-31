use std::{io, os::windows::ffi::OsStrExt, path::Path, ptr};
use windows_sys::Win32::{Foundation::*, System::Registry::*};
use windows_sys::core::w;

fn command(path: &Path) -> Vec<u16> {
    [b'"' as u16]
        .into_iter()
        .chain(path.as_os_str().encode_wide())
        .chain([b'"' as u16, 0])
        .collect()
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let command = if enabled {
        command(&std::env::current_exe().map_err(|e| e.to_string())?)
    } else {
        Vec::new()
    };
    let mut key = ptr::null_mut();
    let result = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            0,
            ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            ptr::null(),
            &mut key,
            ptr::null_mut(),
        )
    };
    if result != ERROR_SUCCESS {
        return Err(format!(
            "Autostart: {}",
            io::Error::from_raw_os_error(result as i32)
        ));
    }
    let result = unsafe {
        let result = if enabled {
            RegSetValueExW(
                key,
                w!("RustClip"),
                0,
                REG_SZ,
                command.as_ptr().cast(),
                (command.len() * size_of::<u16>()) as u32,
            )
        } else {
            RegDeleteValueW(key, w!("RustClip"))
        };
        RegCloseKey(key);
        result
    };
    if result == ERROR_SUCCESS || (!enabled && result == ERROR_FILE_NOT_FOUND) {
        Ok(())
    } else {
        Err(format!(
            "Autostart: {}",
            io::Error::from_raw_os_error(result as i32)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_command_quotes_unicode_path_with_spaces() {
        assert_eq!(
            command(Path::new(r"C:\Мои программы\RustClip\rustclip.exe")),
            "\"C:\\Мои программы\\RustClip\\rustclip.exe\"\0"
                .encode_utf16()
                .collect::<Vec<_>>()
        );
    }
}
