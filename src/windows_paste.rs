use crate::settings::Settings;
use crate::store::ClipData;
use crate::windows_clipboard::{self, RestoreOutcome};
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
    SetForegroundWindow,
};

pub fn paste(
    clip: &ClipData,
    target: usize,
    monitor_window: usize,
    settings: &Settings,
    own_sequence: &AtomicU32,
    expected_sequence: Option<u32>,
) -> Result<&'static str, String> {
    if !settings.paste_enabled {
        return Err("Automatic paste is disabled in settings.".into());
    }
    // Do not inject Ctrl+V while Alt, Shift, or a shortcut key modifier is still held.
    let deadline = Instant::now() + Duration::from_secs(2);
    while [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN]
        .iter()
        .any(|key| unsafe { GetAsyncKeyState(key.0 as i32) < 0 })
    {
        if Instant::now() >= deadline {
            return Err("Release shortcut modifiers before pasting.".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    let target = HWND(target as *mut c_void);
    let owner = HWND(monitor_window as *mut c_void);
    if !valid_target(target) {
        return Err("The original window is no longer available.".into());
    }
    if !unsafe { SetForegroundWindow(target) }.as_bool() {
        return Err(
            "Could not activate the original window; the clipboard was not changed.".into(),
        );
    }
    thread::sleep(Duration::from_millis(35));
    if unsafe { GetForegroundWindow() } != target {
        return Err(
            "The original window did not become active; the clipboard was not changed.".into(),
        );
    }
    let snapshot = if settings.restore_clipboard {
        Some(unsafe { windows_clipboard::snapshot_all()? })
    } else {
        None
    };
    let written = unsafe { windows_clipboard::write_checked(clip, owner, expected_sequence)? };
    own_sequence.store(written.sequence, Ordering::Release);
    if !valid_target(target) || unsafe { GetForegroundWindow() } != target {
        return Err(
            "The original window changed; the selected clip remains on the clipboard.".into(),
        );
    }
    if !send_ctrl_v() {
        return Err(
            "Windows rejected paste input; the selected clip remains on the clipboard.".into(),
        );
    }
    if let Some(snapshot) = snapshot {
        thread::sleep(Duration::from_millis(u64::from(settings.paste_delay_ms)));
        match unsafe { windows_clipboard::restore_all(snapshot, written.sequence, owner) } {
            Ok(RestoreOutcome::Restored { sequence, complete }) => {
                own_sequence.store(sequence, Ordering::Release);
                if complete {
                    Ok("Pasted; previous clipboard restored.")
                } else {
                    Ok("Pasted; some previous clipboard formats could not be restored.")
                }
            }
            Ok(RestoreOutcome::ChangedExternally) => {
                Ok("Pasted; a newer clipboard change was kept.")
            }
            Err(_) => Ok("Pasted; previous clipboard could not be restored."),
        }
    } else if written.complete {
        Ok("Pasted into the original window.")
    } else {
        Ok("Pasted; some clip formats were unavailable.")
    }
}

pub fn valid_target(window: HWND) -> bool {
    unsafe {
        if !IsWindow(Some(window)).as_bool() || !IsWindowVisible(window).as_bool() {
            return false;
        }
        let mut process = 0_u32;
        if GetWindowThreadProcessId(window, Some(&mut process)) == 0
            || process == std::process::id()
        {
            return false;
        }
        let mut class = [0_u16; 128];
        let length = GetClassNameW(window, &mut class);
        if length > 0 {
            let class = String::from_utf16_lossy(&class[..length as usize]);
            if matches!(
                class.as_str(),
                "Shell_TrayWnd"
                    | "Shell_SecondaryTrayWnd"
                    | "NotifyIconOverflowWindow"
                    | "Progman"
                    | "WorkerW"
            ) {
                return false;
            }
        }
        true
    }
}

fn send_ctrl_v() -> bool {
    let make = |key, key_up| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: if key_up {
                    KEYEVENTF_KEYUP
                } else {
                    Default::default()
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let input = [
        make(VK_CONTROL, false),
        make(VK_V, false),
        make(VK_V, true),
        make(VK_CONTROL, true),
    ];
    let sent = unsafe { SendInput(&input, std::mem::size_of::<INPUT>() as i32) };
    if sent != input.len() as u32 {
        let release = [make(VK_V, true), make(VK_CONTROL, true)];
        let _ = unsafe { SendInput(&release, std::mem::size_of::<INPUT>() as i32) };
        false
    } else {
        true
    }
}
