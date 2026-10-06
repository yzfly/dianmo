//! Append-only log at `%APPDATA%\Dianmo\dianmo.log`. Dianmo never shows error dialogs (they would
//! block a touch-only user); problems go here. Rotated to `dianmo.log.old` at 512 KB on start.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static LOG: Mutex<Option<File>> = Mutex::new(None);

const MAX_BYTES: u64 = 512 * 1024;

pub fn init(path: &Path) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(path, path.with_extension("log.old"));
    }
    if let Ok(f) = OpenOptions::new().create(true).append(true).open(path)
        && let Ok(mut g) = LOG.lock()
    {
        *g = Some(f);
    }
}

pub fn write(msg: &str) {
    let line = format!("{} [{}] {msg}\n", timestamp(SystemTime::now()), std::process::id());
    match LOG.try_lock() {
        Ok(mut g) => {
            if let Some(f) = g.as_mut() {
                let _ = f.write_all(line.as_bytes());
                return;
            }
        }
        // Poisoned (a panic while logging) or contended from the panic hook: still try.
        Err(std::sync::TryLockError::Poisoned(p)) => {
            if let Some(f) = p.into_inner().as_mut() {
                let _ = f.write_all(line.as_bytes());
                return;
            }
        }
        Err(std::sync::TryLockError::WouldBlock) => {}
    }
    eprint!("{line}");
}

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => { $crate::log::write(&format!($($arg)*)) };
}

/// `YYYY-MM-DD hh:mm:ss.mmmZ` (UTC; no time-zone database needed).
pub fn timestamp(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs();
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60,
        d.subsec_millis()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn timestamps() {
        assert_eq!(timestamp(UNIX_EPOCH), "1970-01-01 00:00:00.000Z");
        // 2026-10-06 05:41:22.5 UTC
        let t = UNIX_EPOCH + Duration::from_millis(1_791_265_282_500);
        assert_eq!(timestamp(t), "2026-10-06 05:41:22.500Z");
        let leap = UNIX_EPOCH + Duration::from_secs(951_782_400); // 2000-02-29
        assert_eq!(&timestamp(leap)[..10], "2000-02-29");
    }
}
