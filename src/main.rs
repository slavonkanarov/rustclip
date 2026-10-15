#![cfg_attr(windows, windows_subsystem = "windows")]

mod history_selection;
mod settings;
mod store;
mod templates;

#[cfg(windows)]
mod image_preview;
#[cfg(windows)]
mod windows_app;
#[cfg(windows)]
mod windows_autostart;
#[cfg(windows)]
mod windows_clipboard;
#[cfg(windows)]
mod windows_paste;

#[cfg(windows)]
fn main() {
    if let Err(error) = windows_app::run() {
        windows_app::show_startup_error(&error.to_string());
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("rustclip currently runs on Windows only.");
}
