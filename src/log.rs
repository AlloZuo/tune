//! Simple file-based error logger.
//!
//! Initialized once at startup; writes timestamped ERROR lines
//! to a log file (defaults to `tune.log` in the working directory).
//!
//! Opens and closes the file on every write so the log is visible
//! immediately without needing to close the app.
//!
//! # Usage
//! ```ignore
//! log::init("tune.log");
//! log_error!("something went wrong: {}", err);
//! ```

use std::sync::OnceLock;

/// Path to the log file (set once at init).
pub(crate) static LOG_PATH: OnceLock<String> = OnceLock::new();

/// Store the log file path for open-on-write access.
/// Call once at startup; subsequent calls are silently ignored.
pub fn init(path: &str) {
    LOG_PATH.set(path.to_string()).ok();
}

/// Write a timestamped error line to the log file.
/// Opens, writes, and closes the file on every call so data is
/// immediately visible on disk regardless of OS buffering.
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        if let Some(path) = $crate::log::LOG_PATH.get() {
            use std::fs::OpenOptions;
            use std::io::Write;
            if let Ok(mut f) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let ts = $crate::log::timestamp();
                let _ = writeln!(f, "[{}] ERROR: {}", ts, format_args!($($arg)*));
                let _ = f.flush();
            }
        }
    };
}

/// Return a human-readable local timestamp `YYYY-MM-DD HH:MM:SS`.
pub(crate) fn timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}
