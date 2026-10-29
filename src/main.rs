#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;
mod autostart;
mod clipboard;
mod hotkeys;
mod model;
mod secrets;

use app::App;
use eframe::egui;
use std::io::Write;

fn main() -> eframe::Result {
    let default_panic_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let directory = model::directory();
        if std::fs::create_dir_all(&directory).is_ok()
            && let Ok(mut log) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(directory.join("crash.log"))
        {
            let _ = writeln!(
                log,
                "{}: {info}\n{}",
                model::now(),
                std::backtrace::Backtrace::force_capture()
            );
        }
        default_panic_hook(info);
    }));
    use windows_sys::Win32::{Foundation::*, System::Threading::*, UI::WindowsAndMessaging::*};
    let initial_target = unsafe { GetForegroundWindow() } as usize;
    let demo = std::env::args().any(|arg| arg == "--demo");
    let mutex_name = if demo {
        "Local\\RustClip.DemoSingleInstance"
    } else {
        "Local\\RustClip.SingleInstance"
    };
    let mutex = unsafe { CreateMutexW(std::ptr::null(), 0, clipboard::wide(mutex_name).as_ptr()) };
    if !mutex.is_null() && unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            let hwnd = FindWindowW(
                clipboard::wide("RustClip.Listener").as_ptr(),
                std::ptr::null(),
            );
            PostMessageW(hwnd, WM_APP + 1, 0, WM_LBUTTONUP as isize);
            CloseHandle(mutex);
        }
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RustClip (История буфера обмена)")
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
                    .expect("valid app icon"),
            )
            .with_inner_size([1120., 800.])
            .with_min_inner_size([880., 560.]),
        ..Default::default()
    };
    let result = eframe::run_native(
        "RustClip",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, initial_target)))),
    );
    if !mutex.is_null() {
        unsafe {
            CloseHandle(mutex);
        }
    }
    result
}
