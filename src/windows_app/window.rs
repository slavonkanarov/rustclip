use super::*;
use crate::windows_paste;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::{PhysicalPosition, PhysicalSize};
use std::ffi::c_void;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

pub(super) struct InstanceGuard(HANDLE);

impl InstanceGuard {
    pub(super) fn acquire(background: bool) -> Result<Option<Self>, String> {
        let handle = unsafe { CreateMutexW(None, false, w!("Local\\rustclip-instance-v1")) }
            .map_err(|error| format!("Could not check the running instance: {error}"))?;
        let already_running = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        if already_running {
            let _ = unsafe { CloseHandle(handle) };
            if !background {
                open_existing_main()?;
            }
            return Ok(None);
        }
        Ok(Some(Self(handle)))
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

pub(super) fn open_existing_main() -> Result<(), String> {
    for _ in 0..100 {
        if let Ok(window) = unsafe {
            FindWindowExW(
                Some(HWND_MESSAGE),
                None,
                w!("RustclipMessageWindow"),
                PCWSTR::null(),
            )
        } {
            let mut process = 0;
            unsafe { GetWindowThreadProcessId(window, Some(&mut process)) };
            if process != 0 {
                let _ = unsafe { AllowSetForegroundWindow(process) };
            }
            unsafe { PostMessageW(Some(window), WM_APP + 11, WPARAM(0), LPARAM(0)) }
                .map_err(|error| format!("Could not open the main window: {error}"))?;
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err("rustclip is already running but its main window is unavailable.".into())
}

pub fn show_startup_error(message: &str) {
    let wide = wide(message);
    unsafe {
        MessageBoxW(None, PCWSTR(wide.as_ptr()), w!("rustclip"), MB_ICONERROR);
    }
}

pub(super) fn show_startup_warning(message: &str) {
    let wide = wide(message);
    unsafe {
        MessageBoxW(None, PCWSTR(wide.as_ptr()), w!("rustclip"), MB_ICONWARNING);
    }
}

pub(super) fn show_page(ui: &MainWindow, page: i32) {
    if ui.get_saving() {
        return;
    }
    if page != 2 && ui.get_settings_open() {
        ui.invoke_cancel_settings();
    }
    if page == 2 {
        ui.set_settings_open(true);
        ui.set_template_editor_open(false);
    } else {
        ui.set_page(page);
        ui.set_settings_open(false);
        ui.set_template_editor_open(false);
    }
    if let Err(error) = ui.show() {
        set_status(ui, &format!("Could not show window: {error}"));
    } else if let Ok(handle) = ui.window().window_handle().window_handle() {
        if let RawWindowHandle::Win32(handle) = handle.as_raw() {
            let window = HWND(handle.hwnd.get() as *mut c_void);
            unsafe {
                let _ = ShowWindow(window, SW_RESTORE);
                let _ = SetForegroundWindow(window);
            }
        }
    }
    ui.invoke_refresh_preview();
}

pub(super) fn position_preview(
    ui: &MainWindow,
    preview: &PreviewWindow,
    target: usize,
    quick: bool,
) {
    let window = if quick && target != 0 {
        HWND(target as *mut c_void)
    } else if let Ok(handle) = ui.window().window_handle().window_handle() {
        match handle.as_raw() {
            RawWindowHandle::Win32(handle) => HWND(handle.hwnd.get() as *mut c_void),
            _ => unsafe { GetForegroundWindow() },
        }
    } else {
        unsafe { GetForegroundWindow() }
    };
    let mut monitor = MONITORINFO::default();
    monitor.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    let found = unsafe {
        GetMonitorInfoW(
            MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST),
            &mut monitor,
        )
    }
    .as_bool();
    let (left, top, right, bottom) = if found {
        let rect = monitor.rcWork;
        (rect.left, rect.top, rect.right, rect.bottom)
    } else {
        (0, 0, unsafe { GetSystemMetrics(SM_CXSCREEN) }, unsafe {
            GetSystemMetrics(SM_CYSCREEN)
        })
    };
    let scale = preview.window().scale_factor();
    let main_scale = ui.window().scale_factor();
    let minimum_main_width = (760.0 * main_scale) as i32;
    let preferred_width = (370.0 * scale) as i32;
    let minimum_preview_width = (220.0 * scale) as i32;
    let width = if quick {
        preferred_width
    } else {
        preferred_width.min((right - left - minimum_main_width - 12).max(minimum_preview_width))
    };
    let height = (290.0 * scale) as i32;
    let preview_size = PhysicalSize::new(width as u32, height as u32);
    if preview.window().size() != preview_size {
        preview.window().set_size(preview_size);
    }
    let (x, y) = if quick {
        (
            left + (right - left - width) / 2,
            top + (bottom - top - height) / 2,
        )
    } else {
        let size = ui.window().size();
        let available_width = (right - left - width - 12).max(minimum_main_width);
        let main_width = (size.width as i32).min(available_width);
        if main_width != size.width as i32 {
            ui.window()
                .set_size(PhysicalSize::new(main_width as u32, size.height));
        }
        let main = ui.window().position();
        let desired_x = main
            .x
            .max(left + width + 12)
            .min((right - main_width).max(left + width + 12));
        if desired_x != main.x {
            ui.window()
                .set_position(PhysicalPosition::new(desired_x, main.y));
        }
        (desired_x - width - 12, main.y)
    };
    let position = PhysicalPosition::new(
        x.clamp(left, (right - width).max(left)),
        y.clamp(top, (bottom - height).max(top)),
    );
    if preview.window().position() != position {
        preview.window().set_position(position);
    }
}

pub(super) fn capture_target(state: &Arc<Mutex<UiState>>) {
    let target = unsafe { GetForegroundWindow() };
    let mut guard = lock(state);
    if windows_paste::valid_target(target) {
        guard.original_window = target.0 as usize;
        guard.last_external_window = target.0 as usize;
    } else {
        guard.original_window = guard.last_external_window;
    }
}

/// Configure the preview before its native window is created, avoiding a focus flash.
pub(super) fn configure_backend() -> Result<(), String> {
    use slint::winit_030::winit::platform::windows::WindowAttributesExtWindows;
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .with_winit_window_attributes_hook(|attributes| {
            if !attributes.decorations {
                attributes.with_active(false).with_skip_taskbar(true)
            } else {
                attributes
            }
        })
        .select()
        .map_err(|error| error.to_string())
}

pub(super) fn show_preview(preview: &PreviewWindow) {
    if !preview.window().is_visible() {
        let _ = preview.show();
    }
    if let Ok(handle) = preview.window().window_handle().window_handle() {
        if let RawWindowHandle::Win32(handle) = handle.as_raw() {
            let window = HWND(handle.hwnd.get() as *mut c_void);
            unsafe {
                let style = GetWindowLongPtrW(window, GWL_EXSTYLE);
                SetWindowLongPtrW(
                    window,
                    GWL_EXSTYLE,
                    style | WS_EX_NOACTIVATE.0 as isize | WS_EX_TOOLWINDOW.0 as isize,
                );
                let _ = SetWindowPos(
                    window,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
    }
}
