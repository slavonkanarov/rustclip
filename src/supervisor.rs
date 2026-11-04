use std::{
    io,
    process::Command,
    time::{Duration, Instant},
};

const MAX_RESTARTS: usize = 3;
const RESET_AFTER: Duration = Duration::from_secs(300);

fn retry_count(previous: usize, uptime: Duration) -> Option<usize> {
    let previous = if uptime >= RESET_AFTER { 0 } else { previous };
    (previous < MAX_RESTARTS).then_some(previous + 1)
}

pub fn run() -> io::Result<()> {
    let executable = std::env::current_exe()?;
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let mut retries = 0;
    loop {
        let mut command = Command::new(&executable);
        command.args(&args).arg("--internal-run");
        if retries > 0 && !crate::diagnostics::safe_mode() {
            command.arg("--safe-mode");
        }
        let started = Instant::now();
        let mut child = command.spawn()?;
        crate::diagnostics::log(&format!("Application process started pid={}", child.id()));
        let status = child.wait()?;
        if status.success() {
            crate::diagnostics::log("Application exited normally");
            return Ok(());
        }
        crate::diagnostics::log(&format!(
            "Application crashed: {status}; uptime_ms={}",
            started.elapsed().as_millis()
        ));
        retries = retry_count(retries, started.elapsed()).ok_or_else(|| {
            io::Error::other(
                "Repeated crashes; automatic restart stopped. See runtime.log and crash.log.",
            )
        })?;
        crate::diagnostics::log(&format!("Restarting in safe mode (attempt {retries})"));
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_crashes_stop_but_stable_sessions_reset_budget() {
        assert_eq!(retry_count(0, Duration::ZERO), Some(1));
        assert_eq!(retry_count(2, Duration::ZERO), Some(3));
        assert_eq!(retry_count(3, Duration::ZERO), None);
        assert_eq!(retry_count(3, RESET_AFTER), Some(1));
    }
}
