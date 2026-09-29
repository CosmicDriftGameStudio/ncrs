//! Presentation helpers for sizes and dates.

use std::time::SystemTime;

use chrono::{DateTime, Local};

use crate::fs::FileEntry;

/// Human readable size, e.g. `512 B`, `1.4 KB`, `3.0 GB`.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// Size column text: `<UP>` / `<DIR>` for directories, size otherwise.
pub fn entry_size(entry: &FileEntry) -> String {
    if entry.is_parent {
        "<UP>".into()
    } else if entry.is_dir {
        "<DIR>".into()
    } else {
        size(entry.size)
    }
}

/// Local timestamp `YYYY-MM-DD HH:MM`, or empty if unknown.
pub fn time(time: Option<SystemTime>) -> String {
    time.map(|t| DateTime::<Local>::from(t).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sizes() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(1023), "1023 B");
        assert_eq!(size(1536), "1.5 KB");
        assert_eq!(size(5 * 1024 * 1024), "5.0 MB");
    }
}
