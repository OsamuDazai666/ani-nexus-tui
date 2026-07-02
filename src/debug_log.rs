//! Debug logging to file - helps diagnose FlareSolverr/browser issues
//!
//! Log file location: ~/.local/share/nexus-tui/debug.log

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

static LOGGER: Mutex<Option<DebugLogger>> = Mutex::new(None);

pub struct DebugLogger {
    path: PathBuf,
}

impl DebugLogger {
    fn new() -> Self {
        let path = log_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        Self { path }
    }

    fn log(&self, msg: &str) {
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            let ts = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(f, "[{}] {}", ts, msg);
        }
    }
}

fn log_path() -> PathBuf {
    directories::ProjectDirs::from("dev", "nexus", "nexus-tui")
        .map(|d| d.data_local_dir().join("debug.log"))
        .unwrap_or_else(|| PathBuf::from("/tmp/nexus-tui-debug.log"))
}

/// Log a debug message to file
pub fn debug_log(msg: &str) {
    let mut guard = LOGGER.lock().unwrap();
    if guard.is_none() {
        *guard = Some(DebugLogger::new());
    }
    if let Some(logger) = guard.as_ref() {
        logger.log(msg);
    }
}

/// Log with formatting
#[macro_export]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        $crate::debug_log::debug_log(&format!($($arg)*))
    };
}
