use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ
     RegCloseKey,

    RegCreateKeyExW, RegDeleteValueW, RegSetValueExW,
};
use windows::core::{PCWSTR, w};

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let bytes = if enabled {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let command = format!("\"{}\" --background", exe.display());
        command
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<u8>>()
    } else {
        Vec::new()
    };
    let mut key = HKEY::default();
    let code = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
    };
    if code != ERROR_SUCCESS {
        return Err(format!("Could not open Windows startup settings: {code:?}"));
    }
    let result = if enabled {
        let code = unsafe { RegSetValueExW(key, w!("rustclip"), None, REG_SZ, Some(&bytes)) };
        if code == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!("Could not enable startup: {code:?}"))
        }
    } else {
        let code = unsafe { RegDeleteValueW(key, w!("rustclip")) };
        if code == ERROR_SUCCESS || code == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(format!("Could not disable startup: {code:?}"))
        }
    };
    let _ = unsafe { RegCloseKey(key) };
    result
}
