//! Presentation helpers for sizes and dates.
use std::time::SystemTime;

use chrono::{DateTime, Local};

use crate::fs::FileEntry;
use crate::i18n::Msg;

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
    // `get` rather than `[]`: the loop above keeps `unit` inside the array, but
    // saying so with a type instead of relying on it means a later change to the
    // loop cannot turn a file size into a panic.
    let label = UNITS.get(unit).copied().unwrap_or("?");
    format!("{value:.1} {label}")
}

/// Size column text: `<UP>` / `<DIR>` for directories, size otherwise.
///
/// The markers are a Norton Commander convention and are deliberately not
/// translated, so this takes no `Language`.
pub fn entry_size(entry: &FileEntry) -> String {
    if entry.is_parent {
        Msg::MarkerUp.text().into()
    } else if entry.is_dir {
        Msg::MarkerDir.text().into()
    } else {
        size(entry.size)
    }
}

/// Local timestamp `YYYY-MM-DD HH:MM`, or empty if unknown.
pub fn time(time: Option<SystemTime>) -> String {
    time.map(|t| {
        DateTime::<Local>::from(t)
            .format("%Y-%m-%d %H:%M")
            .to_string()
    })
    .unwrap_or_default()
}

const ELLIPSIS: char = '…';

/// `text` cut after `max_chars` characters (the ellipsis included), for text
/// that must stay on one line. Counts characters, not bytes.
pub fn ellipsize_end(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let Some(kept) = max_chars.checked_sub(1) else {
        return String::new();
    };
    let mut cut: String = text.chars().take(kept).collect();
    cut.push(ELLIPSIS);
    cut
}

/// What `ellipsize_middle` keeps of a text: this many characters from the
/// start and this many from the end, with `…` between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MiddleCut {
    pub head: usize,
    pub tail: usize,
}

/// How `text` is cut to fit `max_chars` characters, or `None` when it fits.
/// A budget of zero keeps nothing, not even the `…`.
pub fn middle_cut(text: &str, max_chars: usize) -> Option<MiddleCut> {
    if text.chars().count() <= max_chars {
        return None;
    }
    let kept = max_chars.saturating_sub(1);
    let last_component = text
        .rsplit(['/', '\\'])
        .next()
        .map_or(0, |component| component.chars().count());
    // The tail holds the whole last component and its separator where that
    // fits, else everything there is room for; at least half is the tail.
    let tail = last_component
        .saturating_add(1)
        .max(kept.div_ceil(2))
        .min(kept);
    Some(MiddleCut {
        head: kept.saturating_sub(tail),
        tail,
    })
}

/// `text` with the middle cut out so at most `max_chars` characters remain, for
/// paths: the end stays, so the last component (the file name) is visible.
pub fn ellipsize_middle(text: &str, max_chars: usize) -> String {
    let Some(cut) = middle_cut(text, max_chars) else {
        return text.to_owned();
    };
    if max_chars == 0 {
        return String::new();
    }
    let count = text.chars().count();
    let mut shortened: String = text.chars().take(cut.head).collect();
    shortened.push(ELLIPSIS);
    shortened.extend(text.chars().skip(count.saturating_sub(cut.tail)));
    shortened
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
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

    const LONG_PATH: &str = "/home/user/documents/company-a/2024/invoices/invoice-0042.pdf";

    #[test]
    fn short_text_is_left_alone() {
        assert_eq!(ellipsize_end("invoice.pdf", 11), "invoice.pdf");
        assert_eq!(ellipsize_middle("/a/invoice.pdf", 40), "/a/invoice.pdf");
        assert_eq!(ellipsize_end("", 0), "");
        assert_eq!(ellipsize_middle("", 0), "");
    }

    #[test]
    fn the_end_cut_fits_the_budget_and_marks_the_cut() {
        let cut = ellipsize_end(&"x".repeat(200), 20);
        assert_eq!(cut.chars().count(), 20);
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn the_middle_cut_keeps_the_last_path_component() {
        for max_chars in [20, 25, 30, 40] {
            let cut = ellipsize_middle(LONG_PATH, max_chars);
            assert!(cut.chars().count() <= max_chars, "{cut}");
            assert!(cut.ends_with("/invoice-0042.pdf"), "{cut}");
            assert!(cut.starts_with('/') && cut.contains('…'), "{cut}");
        }
    }

    #[test]
    fn a_budget_smaller_than_the_file_name_keeps_the_end_of_it() {
        let cut = ellipsize_middle(LONG_PATH, 8);
        assert_eq!(cut.chars().count(), 8);
        assert!(cut.starts_with('…') && cut.ends_with("42.pdf"), "{cut}");
    }

    #[test]
    fn non_ascii_text_is_cut_on_characters() {
        let umlauts = "/Büro/Müller & Söhne/Übersicht-Größe-ÄÖÜ-äöüß-€€€.pdf";
        for max_chars in [0, 1, 2, 5, 12, 30] {
            assert!(ellipsize_end(umlauts, max_chars).chars().count() <= max_chars);
            assert!(ellipsize_middle(umlauts, max_chars).chars().count() <= max_chars);
        }
        let cut = ellipsize_middle(umlauts, 30);
        assert!(cut.ends_with("€€€.pdf"), "{cut}");
        assert_eq!(ellipsize_end("日本語のファイル名", 4), "日本語…");
    }
}
