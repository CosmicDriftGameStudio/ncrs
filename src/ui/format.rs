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

const NARROW_GLYPHS: &str = "iljtfr.,:;'|! ";
const WIDE_GLYPHS: &str = "mwMW@%—–";
const NARROW_EM: f32 = 0.30;
const WIDE_EM: f32 = 0.85;
const CAPITAL_OR_DIGIT_EM: f32 = 0.62;
const OTHER_ASCII_EM: f32 = 0.52;
const OTHER_LETTER_EM: f32 = 0.62;
const FULL_WIDTH_EM: f32 = 1.0;
/// Names without a separator keep this many glyphs before the extension.
const KEPT_BEFORE_EXTENSION: f32 = 3.0;

/// How wide `glyph` is at most, in ems. Deliberately on the wide side: a text
/// that is estimated to fit must fit, so the cut loses a glyph now and then
/// rather than letting text touch the next column.
fn glyph_em(glyph: char) -> f32 {
    if NARROW_GLYPHS.contains(glyph) {
        NARROW_EM
    } else if WIDE_GLYPHS.contains(glyph) {
        WIDE_EM
    } else if glyph.is_ascii_uppercase() || glyph.is_ascii_digit() {
        CAPITAL_OR_DIGIT_EM
    } else if glyph.is_ascii() {
        OTHER_ASCII_EM
    } else if glyph.is_alphabetic() && !is_full_width(glyph) {
        OTHER_LETTER_EM
    } else {
        FULL_WIDTH_EM
    }
}

fn is_full_width(glyph: char) -> bool {
    matches!(glyph as u32, 0x1100..=0x11FF | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE6F | 0xFF00..=0xFF60 | 0x1F000..=0x1FAFF)
}

/// An estimate of the width of `text` in pixels at `font_size`, never below
/// the real width of the UI font.
pub fn estimated_width(text: &str, font_size: f32) -> f32 {
    text.chars().map(glyph_em).sum::<f32>() * font_size
}

/// A count with a no-break space between the thousands: `2 421`.
pub fn grouped(count: u64) -> String {
    let digits = count.to_string();
    let first = digits.len() % 3;
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (position, digit) in digits.chars().enumerate() {
        if position > 0 && position % 3 == first % 3 {
            grouped.push('\u{a0}');
        }
        grouped.push(digit);
    }
    grouped
}

/// `text` as it is, or cut at the end with `…` to fit `width_px`.
pub fn fit_end(text: &str, width_px: f32, font_size: f32) -> String {
    if estimated_width(text, font_size) <= width_px {
        return text.to_owned();
    }
    let budget = width_px - estimated_width("…", font_size);
    let mut used = 0.0;
    let mut cut: String = text
        .chars()
        .take_while(|glyph| {
            used += glyph_em(*glyph) * font_size;
            used <= budget
        })
        .collect();
    if width_px >= estimated_width("…", font_size) {
        cut.push(ELLIPSIS);
    }
    cut
}

/// `text` as it is, or with the middle cut out to fit `width_px`. A path keeps
/// its last component; a bare name keeps its extension and a few glyphs
/// before it.
pub fn fit_middle(text: &str, width_px: f32, font_size: f32) -> String {
    if estimated_width(text, font_size) <= width_px {
        return text.to_owned();
    }
    let ellipsis = estimated_width("…", font_size);
    if width_px < ellipsis {
        return String::new();
    }
    let budget = width_px - ellipsis;
    let tail_wanted = match text.rfind(['/', '\\']) {
        Some(separator) => {
            let component = text.get(separator..).unwrap_or_default();
            estimated_width(component, font_size).max(budget / 2.0)
        }
        None => {
            let extension = text
                .rfind('.')
                .and_then(|dot| text.get(dot..))
                .unwrap_or_default();
            (estimated_width(extension, font_size)
                + KEPT_BEFORE_EXTENSION * OTHER_ASCII_EM * font_size)
                .min(budget / 2.0)
        }
    };
    let tail_budget = tail_wanted.min(budget);
    let head_budget = budget - tail_budget;
    let mut used = 0.0;
    let head: String = text
        .chars()
        .take_while(|glyph| {
            used += glyph_em(*glyph) * font_size;
            used <= head_budget
        })
        .collect();
    let mut used = 0.0;
    let mut tail: Vec<char> = text
        .chars()
        .rev()
        .take_while(|glyph| {
            used += glyph_em(*glyph) * font_size;
            used <= tail_budget
        })
        .collect();
    tail.reverse();
    format!("{head}{ELLIPSIS}{}", tail.into_iter().collect::<String>())
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
    fn counts_are_grouped_by_thousands() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(2421), "2\u{a0}421");
        assert_eq!(grouped(12345), "12\u{a0}345");
        assert_eq!(grouped(1_234_567), "1\u{a0}234\u{a0}567");
    }

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
    const BODY: f32 = 12.0;

    #[test]
    fn narrow_glyphs_take_less_room_than_capitals_digits_and_wide_ones() {
        assert!(estimated_width("illi", BODY) < estimated_width("abcd", BODY));
        assert!(estimated_width("abcd", BODY) < estimated_width("ABCD", BODY));
        assert!(estimated_width("1234", BODY) > estimated_width("abcd", BODY));
        assert!(estimated_width("mmmm", BODY) > estimated_width("ABCD", BODY));
        assert!(estimated_width("日本", BODY) > estimated_width("ÄÖ", BODY));
        assert_eq!(estimated_width("", BODY), 0.0);
    }

    #[test]
    fn the_end_cut_never_exceeds_the_width_and_marks_the_cut() {
        let name = "197812 – Rechnung 2025-0012 Stadtwerke Musterstadt GmbH.pdf";
        for width in [0.0, 5.0, 40.0, 120.0, 200.0, 300.0] {
            let cut = fit_end(name, width, BODY);
            assert!(estimated_width(&cut, BODY) <= width, "{width}: {cut}");
        }
        let cut = fit_end(name, 120.0, BODY);
        assert!(cut.ends_with('…') && cut.starts_with("197812"), "{cut}");
        assert_eq!(fit_end("short.pdf", 200.0, BODY), "short.pdf");
    }

    #[test]
    fn the_middle_cut_keeps_the_extension_of_a_name() {
        let name = "22dd1abd-d1f8-45fe-aae7-5c0ffee12345.pdf";
        for width in [90.0, 120.0, 160.0, 200.0] {
            let cut = fit_middle(name, width, BODY);
            assert!(estimated_width(&cut, BODY) <= width, "{width}: {cut}");
            assert!(cut.ends_with(".pdf") && cut.contains('…'), "{cut}");
            assert!(cut.starts_with("22dd"), "{cut}");
        }
        assert_eq!(fit_middle("a.pdf", 200.0, BODY), "a.pdf");
    }

    #[test]
    fn the_middle_cut_keeps_the_last_component_of_a_path() {
        let cut = fit_middle(LONG_PATH, 260.0, BODY);
        assert!(estimated_width(&cut, BODY) <= 260.0, "{cut}");
        assert!(
            cut.ends_with("/invoice-0042.pdf") && cut.starts_with('/'),
            "{cut}"
        );
        assert_eq!(fit_middle(LONG_PATH, 3.0, BODY), "");
    }
}
