//! Keeps panics in a log file. A panic inside a gpui or AppKit callback aborts the
//! process, and the system crash report then only says "abort"; this log keeps the
//! message, location and backtrace so a report can be traced.

use std::backtrace::Backtrace;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The log starts over once it grows past this size.
const MAX_BYTES: u64 = 256 * 1024;

/// Records every panic of the process, then runs the previous hook.
pub fn install(version: String) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(path) = log_path() {
            let _ = append(&path, &entry(&version, info));
        }
        previous(info);
    }));
}

/// Self tests must not write to the user's logs.
fn log_path() -> Option<PathBuf> {
    if std::env::vars_os().any(|(name, _)| name.to_string_lossy().starts_with("ANGELLY_SELFTEST")) {
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Logs/Angelly/panic.log"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        angelly_core::config::Env::from_process().data_dir().map(|dir| dir.join("panic.log"))
    }
}

fn entry(version: &str, info: &PanicHookInfo<'_>) -> String {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let thread = std::thread::current();
    format!(
        "--- unix time {seconds}, angelly {version}, thread {}\n{info}\n{}\n\n",
        thread.name().unwrap_or("unnamed"),
        Backtrace::force_capture()
    )
}

fn append(path: &Path, text: &str) -> io::Result<()> {
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory)?;
    }
    if fs::metadata(path).is_ok_and(|meta| meta.len() > MAX_BYTES) {
        fs::remove_file(path)?;
    }
    OpenOptions::new().create(true).append(true).open(path)?.write_all(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use super::*;

    #[test]
    fn entries_accumulate_and_the_log_starts_over_when_large() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs").join("panic.log");
        append(&path, "first\n").unwrap();
        append(&path, "second\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "first\nsecond\n");

        fs::write(&path, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        append(&path, "fresh\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "fresh\n");
    }
}
