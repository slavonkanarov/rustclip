#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;
mod autostart;
mod clipboard;
mod diagnostics;
mod hotkeys;
mod i18n;
mod images;
mod model;
mod secrets;
mod supervisor;
mod updates;

use app::App;
use eframe::egui;

fn main() {
    if let Err(e) = diagnostics::install() {
        diagnostics::notify(&format!("Cannot initialize diagnostic logs: {e}"));
        std::process::exit(1);
    }
    if std::env::args_os().any(|arg| arg == "--internal-run") {
        if let Err(e) = run_app() {
            diagnostics::log(&format!("Application error: {e}"));
            std::process::exit(1);
        }
        return;
    }
    use windows_sys::Win32::{Foundation::*, System::Threading::*, UI::WindowsAndMessaging::*};
    let demo = std::env::args().any(|arg| arg == "--demo");
    let mutex_name = if demo {
        "Local\\RustClip.DemoSingleInstance"
    } else {
        "Local\\RustClip.SingleInstance"
    };
    let mutex = unsafe { CreateMutexW(std::ptr::null(), 0, clipboard::wide(mutex_name).as_ptr()) };
    if mutex.is_null() {
        diagnostics::notify(&format!(
            "Cannot create application mutex: {}",
            std::io::Error::last_os_error()
        ));
        std::process::exit(1);
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            let hwnd = FindWindowW(
                clipboard::wide("RustClip.Listener").as_ptr(),
                std::ptr::null(),
            );
            PostMessageW(hwnd, WM_APP + 1, 0, WM_LBUTTONUP as isize);
            CloseHandle(mutex);
        }
        return;
    }
    let result = supervisor::run();
    unsafe {
        CloseHandle(mutex);
    }
    if let Err(e) = result {
        diagnostics::log(&format!("Supervisor stopped: {e}"));
        diagnostics::notify(&format!("{e}\nLogs: {}", model::directory().display()));
        std::process::exit(1);
    }
}

fn run_app() -> eframe::Result {
    let initial_target =
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() } as usize;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RustClip (Clipboard history)")
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
                    .expect("valid app icon"),
            )
            .with_inner_size([1120., 800.])
            .with_min_inner_size([880., 560.]),
        ..Default::default()
    };
    eframe::run_native(
        "RustClip",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, initial_target)))),
    )
}
