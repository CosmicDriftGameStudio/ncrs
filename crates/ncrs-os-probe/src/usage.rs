//! When a file was last opened and how often, where the operating system keeps that:
//! Spotlight on macOS. Everywhere else, and in the App Store build, nothing is known.

use std::path::Path;
use std::time::SystemTime;
#[cfg(any(all(target_os = "macos", not(feature = "sandbox")), test))]
use std::time::{Duration, UNIX_EPOCH};

/// What the system knows about the use of one file or folder. `None` is "not known".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileUsage {
    pub last_opened: Option<SystemTime>,
    pub open_count: Option<u32>,
}

/// Blocking: one `mdls` call on macOS. Not known on other systems or in the sandbox.
pub fn file_usage(path: &Path) -> FileUsage {
    read_usage(path)
}

#[cfg(all(target_os = "macos", not(feature = "sandbox")))]
fn read_usage(path: &Path) -> FileUsage {
    let output = std::process::Command::new("mdls")
        .args([
            "-raw",
            "-name",
            "kMDItemLastUsedDate",
            "-name",
            "kMDItemUseCount",
        ])
        .arg(path)
        .output();
    match output {
        Ok(done) if done.status.success() => parse_usage(&String::from_utf8_lossy(&done.stdout)),
        Ok(_) | Err(_) => FileUsage::default(),
    }
}

#[cfg(not(all(target_os = "macos", not(feature = "sandbox"))))]
fn read_usage(_path: &Path) -> FileUsage {
    FileUsage::default()
}

/// The output of `mdls -raw` for the date and the count: both values, separated by a NUL
/// (or a line break); `(null)` for one the system does not have.
#[cfg(any(all(target_os = "macos", not(feature = "sandbox")), test))]
pub fn parse_usage(raw: &str) -> FileUsage {
    let mut values = raw.split(['\0', '\n']).map(str::trim);
    let last_opened = values.next().and_then(parse_date);
    let open_count = values.next().and_then(|count| count.parse().ok());
    FileUsage {
        last_opened,
        open_count,
    }
}

/// `2026-09-17 10:21:33 +0000`, as Spotlight writes it.
#[cfg(any(all(target_os = "macos", not(feature = "sandbox")), test))]
fn parse_date(text: &str) -> Option<SystemTime> {
    let mut parts = text.split_whitespace();
    let mut date = parts.next()?.split('-');
    let year: i64 = date.next()?.parse().ok()?;
    let month: i64 = date.next()?.parse().ok()?;
    let day: i64 = date.next()?.parse().ok()?;
    let mut clock = parts.next()?.split(':');
    let hours: i64 = clock.next()?.parse().ok()?;
    let minutes: i64 = clock.next()?.parse().ok()?;
    let seconds: i64 = clock.next()?.parse().ok()?;
    let zone = parts.next().unwrap_or("+0000");
    let zone_sign: i64 = if zone.starts_with('-') { -1 } else { 1 };
    let zone_digits = zone.trim_start_matches(['+', '-']);
    let zone_hours: i64 = zone_digits.get(..2)?.parse().ok()?;
    let zone_minutes: i64 = zone_digits.get(2..4)?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let offset = zone_sign * (zone_hours * 3_600 + zone_minutes * 60);
    let epoch = days_from_civil(year, month, day) * 86_400 + hours * 3_600 + minutes * 60 + seconds
        - offset;
    u64::try_from(epoch)
        .ok()
        .map(|since_epoch| UNIX_EPOCH + Duration::from_secs(since_epoch))
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
#[cfg(any(all(target_os = "macos", not(feature = "sandbox")), test))]
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let march_year = if month <= 2 { year - 1 } else { year };
    let era = march_year.div_euclid(400);
    let year_of_era = march_year.rem_euclid(400);
    let shifted_month = (month + 9).rem_euclid(12);
    let day_of_year = (153 * shifted_month + 2).div_euclid(5) + day - 1;
    let day_of_era =
        year_of_era * 365 + year_of_era.div_euclid(4) - year_of_era.div_euclid(100) + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seconds(time: Option<SystemTime>) -> Option<u64> {
        time.and_then(|at| at.duration_since(UNIX_EPOCH).ok())
            .map(|since| since.as_secs())
    }

    #[test]
    fn a_date_and_a_count_are_read() {
        let usage = parse_usage("2026-09-17 10:21:33 +0000\u{0}3");
        assert_eq!(seconds(usage.last_opened), Some(1_789_640_493));
        assert_eq!(usage.open_count, Some(3));
    }

    #[test]
    fn the_zone_offset_moves_the_time() {
        let utc = parse_usage("2026-09-17 10:00:00 +0000\u{0}1").last_opened;
        let berlin = parse_usage("2026-09-17 12:00:00 +0200\u{0}1").last_opened;
        assert_eq!(utc, berlin);
    }

    #[test]
    fn a_missing_value_stays_none() {
        let usage = parse_usage("(null)\u{0}(null)");
        assert_eq!(usage, FileUsage::default());
        let only_date = parse_usage("2026-01-02 03:04:05 +0000\u{0}(null)");
        assert!(only_date.last_opened.is_some());
        assert_eq!(only_date.open_count, None);
        assert_eq!(parse_usage(""), FileUsage::default());
    }

    #[test]
    fn the_epoch_is_day_zero() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
    }
}
