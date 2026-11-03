use std::{
    fs::{self, OpenOptions},
    io::Write,
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
    sync::Mutex,
};

static LOG_LOCK: Mutex<()> = Mutex::new(());
const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;

pub fn safe_mode() -> bool {
    std::env::args_os().any(|arg| arg == "--safe-mode")
}

fn append(path: &Path, message: &str) -> std::io::Result<()> {
    if path.metadata().is_ok_and(|m| m.len() >= MAX_LOG_BYTES) {
        let previous = path.with_extension("log.old");
        match fs::remove_file(&previous) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        fs::rename(path, previous)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(
        file,
        "{} pid={} thread={:?}: {message}",
        crate::model::now(),
        std::process::id(),
        std::thread::current().name()
    )?;
    file.sync_data()
}

pub fn log(message: &str) {
    write("runtime.log", message);
}

fn write(name: &str, message: &str) {
    let _guard = LOG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = append(&crate::model::directory().join(name), message) {
        eprintln!("RustClip could not write {name}: {e}");
    }
}

pub fn notify(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            crate::clipboard::wide(message).as_ptr(),
            crate::clipboard::wide("RustClip").as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

pub fn install() -> std::io::Result<()> {
    fs::create_dir_all(crate::model::directory())?;
    append(
        &crate::model::directory().join("runtime.log"),
        &format!(
            "Starting version={} safe_mode={}",
            option_env!("RUSTCLIP_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION")),
            safe_mode()
        ),
    )?;
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write(
            "crash.log",
            &format!("{info}\n{}", std::backtrace::Backtrace::force_capture()),
        );
        default(info);
    }));
    Ok(())
}

pub fn recover<T>(operation: &str, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(_) => Err(format!(
            "{operation}: panic recovered; operation skipped (see crash.log)"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_skips_panicking_operation_and_allows_next_one() {
        assert!(recover::<()>("test", || panic!("test panic")).is_err());
        assert_eq!(recover("test", || Ok(42)), Ok(42));
        assert_eq!(
            recover::<()>("test", || Err("busy".into())),
            Err("busy".into())
        );
    }

    #[test]
    fn logs_rotate_and_keep_previous_report() {
        let path = std::env::temp_dir().join(format!("rustclip-log-{}.log", std::process::id()));
        fs::write(&path, vec![b'x'; MAX_LOG_BYTES as usize]).unwrap();
        append(&path, "next report").unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("next report"));
        let previous = path.with_extension("log.old");
        assert_eq!(previous.metadata().unwrap().len(), MAX_LOG_BYTES);
        fs::remove_file(path).unwrap();
        fs::remove_file(previous).unwrap();
    }
}
