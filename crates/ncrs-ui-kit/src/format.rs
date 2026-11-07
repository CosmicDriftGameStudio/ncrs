//! Presentation helpers for sizes and dates.
use std::fmt::Write;
use std::path::Path;
use std::time::SystemTime;

use chrono::{DateTime, Datelike, Local};
use ncrs_core::{DateFormat, FileEntry, SizeFormat, WindowsAttributes};
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_index::Amount;

/// How many minor units (cents) make one unit of an amount.
pub const MINOR_UNITS_PER_UNIT: i64 = 100;

/// The amount as the field shows it, parseable by the review's `parse_amount`.
pub fn amount_text(amount: &Amount) -> String {
    format!("{} {}", amount_digits(amount), amount.currency.as_str())
}

/// The number of an amount without its currency, with two decimals.
pub fn amount_digits(amount: &Amount) -> String {
    let magnitude = amount.minor_units.unsigned_abs();
    let unit = MINOR_UNITS_PER_UNIT.unsigned_abs();
    let sign = if amount.minor_units < 0 { "-" } else { "" };
    format!(
        "{sign}{}.{:02}",
        magnitude.div_euclid(unit),
        magnitude.rem_euclid(unit)
    )
}

/// Appended to a computed folder size that missed unreadable parts. Untranslated
/// like the markers.
const PARTIAL_SIZE_MARK: char = '+';

/// Size column text: `<UP>` / `<DIR>` for directories (the computed size once
/// there is one), size otherwise.
///
/// The markers are a classic dual-panel convention and are deliberately not
/// translated, so this takes no `Language`.
pub fn entry_size(entry: &FileEntry) -> String {
    entry_size_in(entry, SizeFormat::Readable)
}

/// [`entry_size`] with the sizes written as `format` says.
pub fn entry_size_in(entry: &FileEntry, format: SizeFormat) -> String {
    if entry.is_parent {
        Msg::MarkerUp.text().into()
    } else if let (true, Some(computed)) = (entry.is_dir, entry.folder_size) {
        let text = size_in(computed.bytes, format);
        if computed.partial {
            format!("{text}{PARTIAL_SIZE_MARK}")
        } else {
            text
        }
    } else if entry.is_dir {
        Msg::MarkerDir.text().into()
    } else {
        size_in(entry.size, format)
    }
}

/// `1 234 567 B`, `1 206 KB` or `1.2 MB`, as the Size setting says. A file
/// with content is never shown as 0 KB.
pub fn size_in(bytes: u64, format: SizeFormat) -> String {
    match format {
        SizeFormat::Bytes => format!("{} B", grouped(bytes)),
        SizeFormat::Kilobytes => {
            let rounded = bytes.saturating_add(512).div_euclid(1024);
            let kilobytes = if bytes == 0 { 0 } else { rounded.max(1) };
            format!("{} KB", grouped(kilobytes))
        }
        SizeFormat::Readable => size(bytes),
    }
}

/// The extension of a file without its dot; empty for a folder and for a name
/// without one.
pub fn extension(entry: &FileEntry) -> String {
    if entry.is_dir || entry.is_parent {
        return String::new();
    }
    Path::new(&entry.name)
        .extension()
        .map(|extension| extension.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `drwxr-xr-x`, `-rw-r--r--` or `lrwxrwxrwx`; `s`/`S` and `t`/`T` mark the
/// set-id and sticky bits with and without the execute bit below them. On
/// Windows the attributes: `R--`, `-H-`, `--S`. Empty when the source knows
/// neither.
pub fn permissions(entry: &FileEntry) -> String {
    if let Some(attributes) = entry.details.windows {
        return windows_attributes(attributes);
    }
    let Some(mode) = entry.details.mode else {
        return String::new();
    };
    let kind = if entry.is_symlink {
        'l'
    } else if entry.is_dir {
        'd'
    } else {
        '-'
    };
    permission_string(mode, kind)
}

/// The nine permission characters of `mode` behind `kind` (`d`, `l` or `-`).
pub fn permission_string(mode: u32, kind: char) -> String {
    struct Class {
        read: u32,
        write: u32,
        execute: u32,
        special: u32,
        special_executable: char,
        special_not_executable: char,
    }
    const CLASSES: [Class; 3] = [
        Class {
            read: 0o400,
            write: 0o200,
            execute: 0o100,
            special: 0o4000,
            special_executable: 's',
            special_not_executable: 'S',
        },
        Class {
            read: 0o040,
            write: 0o020,
            execute: 0o010,
            special: 0o2000,
            special_executable: 's',
            special_not_executable: 'S',
        },
        Class {
            read: 0o004,
            write: 0o002,
            execute: 0o001,
            special: 0o1000,
            special_executable: 't',
            special_not_executable: 'T',
        },
    ];
    let mut text = String::with_capacity(10);
    text.push(kind);
    for class in CLASSES {
        text.push(if mode & class.read != 0 { 'r' } else { '-' });
        text.push(if mode & class.write != 0 { 'w' } else { '-' });
        text.push(
            match (mode & class.execute != 0, mode & class.special != 0) {
                (true, true) => class.special_executable,
                (false, true) => class.special_not_executable,
                (true, false) => 'x',
                (false, false) => '-',
            },
        );
    }
    text
}

fn windows_attributes(attributes: WindowsAttributes) -> String {
    let flag = |set: bool, letter: char| if set { letter } else { '-' };
    [
        flag(attributes.read_only, 'R'),
        flag(attributes.hidden, 'H'),
        flag(attributes.system, 'S'),
    ]
    .into_iter()
    .collect()
}

/// Whole megabytes (1 MB = 1,000,000 bytes), rounded: the one unit and rounding
/// of a download and of the model that is downloaded.
fn whole_megabytes(bytes: u64) -> u64 {
    (bytes + 500_000).div_euclid(1_000_000)
}

/// `639 MB`: the size of a download as its card and its progress line say it.
pub fn megabytes(bytes: u64) -> String {
    format!("{} MB", whole_megabytes(bytes))
}

/// Megabytes (1 MB = 1,000,000 bytes) received and in all, and the percent
/// done, rounded and as text for the line under a download's bar.
pub fn download_figures(received: u64, total: u64) -> (String, String, String) {
    let megabytes = whole_megabytes;
    let percent = if total == 0 {
        0
    } else {
        (u128::from(received.min(total)) * 100 + u128::from(total).div_euclid(2))
            .div_euclid(u128::from(total))
    };
    (
        megabytes(received).to_string(),
        megabytes(total).to_string(),
        percent.to_string(),
    )
}

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

/// The folder of `path` as a hit list shows it below the name: relative to
/// `scope` (`./` for a file directly in it), the full folder outside of it.
pub fn folder_below(path: &std::path::Path, scope: &std::path::Path) -> String {
    let folder = path.parent().unwrap_or(path);
    match folder.strip_prefix(scope) {
        Ok(relative) if relative.as_os_str().is_empty() => "./".to_owned(),
        Ok(relative) => relative.to_string_lossy().into_owned(),
        Err(_) => folder.to_string_lossy().into_owned(),
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

/// A time as a date column writes it. Empty when unknown.
pub fn time_in(time: Option<SystemTime>, format: DateFormat, lang: Language) -> String {
    time.map(|time| date_text(DateTime::<Local>::from(time), Local::now(), format, lang))
        .unwrap_or_default()
}

/// `at` written as `format` says, seen from `now`.
pub fn date_text(
    at: DateTime<Local>,
    now: DateTime<Local>,
    format: DateFormat,
    lang: Language,
) -> String {
    match format {
        DateFormat::Iso => at.format("%Y-%m-%d %H:%M").to_string(),
        DateFormat::Short => short_date(at, now, lang),
        DateFormat::Relative => relative_date(at, now, lang),
    }
}

fn relative_date(at: DateTime<Local>, now: DateTime<Local>, lang: Language) -> String {
    let age = now.signed_duration_since(at);
    if age >= chrono::Duration::zero() {
        if age < chrono::Duration::hours(1) {
            let minutes = age.num_minutes().max(1).to_string();
            return fill_template(lang.text(Msg::FormatDateMinutesAgo), &[("{n}", &minutes)]);
        }
        if age < chrono::Duration::hours(24) {
            let hours = age.num_hours().to_string();
            return fill_template(lang.text(Msg::FormatDateHoursAgo), &[("{n}", &hours)]);
        }
        if at.date_naive().succ_opt() == Some(now.date_naive()) {
            let time = at.format("%H:%M").to_string();
            return fill_template(lang.text(Msg::FormatDateYesterday), &[("{time}", &time)]);
        }
    }
    short_date(at, now, lang)
}

fn short_date(at: DateTime<Local>, now: DateTime<Local>, lang: Language) -> String {
    let pattern = if at.year() == now.year() {
        lang.text(Msg::FormatDateShort)
    } else {
        lang.text(Msg::FormatDateShortWithYear)
    };
    let mut written = String::new();
    // A pattern chrono cannot write fails here instead of panicking in
    // `to_string`; the fixed ISO form stands in.
    if write!(written, "{}", at.format(pattern)).is_err() {
        return at.format("%Y-%m-%d %H:%M").to_string();
    }
    written
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

/// `text` with the middle cut out so at most `max_chars` characters remain,
/// about half from the start and the rest from the end: for a single name,
/// where both its beginning and its end tell it apart from similar ones.
pub fn ellipsize_keeping_both_ends(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_owned();
    }
    if max_chars == 0 {
        return String::new();
    }
    let kept = max_chars - 1;
    let head = kept.div_ceil(2);
    let tail = kept - head;
    let mut shortened: String = text.chars().take(head).collect();
    shortened.push(ELLIPSIS);
    shortened.extend(text.chars().skip(count - tail));
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
    let reversed: Vec<char> = count.to_string().chars().rev().collect();
    let groups: Vec<String> = reversed
        .chunks(3)
        .map(|chunk| chunk.iter().rev().collect())
        .collect();
    groups
        .iter()
        .rev()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\u{a0}")
}

/// `3 files`, `1 file`: how many files, worded for `lang`.
pub fn files_counted(count: usize, lang: Language) -> String {
    let msg = if count == 1 {
        Msg::PanelFooterFilesOne
    } else {
        Msg::PanelFooterFiles
    };
    lang.text(msg).replace("{count}", &grouped(count as u64))
}

/// `3 folders`, `1 folder`: how many folders, worded for `lang`.
pub fn folders_counted(count: usize, lang: Language) -> String {
    let msg = if count == 1 {
        Msg::PanelFooterFoldersOne
    } else {
        Msg::PanelFooterFolders
    };
    lang.text(msg).replace("{count}", &grouped(count as u64))
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
    let mut head_used = 0.0;
    let head: String = text
        .chars()
        .take_while(|glyph| {
            head_used += glyph_em(*glyph) * font_size;
            head_used <= head_budget
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

/// `~` for the home directory and what lies below it, so the card shows
/// short paths.
pub fn shown_path(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        Err(_) => path.display().to_string(),
    }
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
    fn download_figures_round_to_megabytes_and_clamp_the_percentage() {
        let figures = |received, total| download_figures(received, total);
        assert_eq!(
            figures(312_400_000, 639_000_000),
            ("312".to_owned(), "639".to_owned(), "49".to_owned())
        );
        assert_eq!(figures(0, 0).2, "0");
        assert_eq!(figures(900, 600).2, "100");
    }

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

    #[test]
    fn a_long_name_keeps_its_start_and_its_end() {
        let name = "2021_01_29_Abrechnung_KD401N05013001112401591.pdf";
        let cut = ellipsize_keeping_both_ends(name, 26);
        assert_eq!(cut.chars().count(), 26);
        assert_eq!(cut, "2021_01_29_Ab…12401591.pdf");
        assert!(cut.contains('…'));
        assert_eq!(ellipsize_keeping_both_ends("short.pdf", 26), "short.pdf");
        assert_eq!(ellipsize_keeping_both_ends("anything", 0), "");
        assert_eq!(ellipsize_keeping_both_ends("Büro-Größe-€€€", 6), "Bür…€€");
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

    fn plain_file(name: &str) -> FileEntry {
        FileEntry {
            name: name.into(),
            path: name.into(),
            is_dir: false,
            is_symlink: false,
            is_parent: false,
            size: 0,
            modified: None,
            folder_size: None,
            details: Default::default(),
        }
    }

    #[test]
    fn sizes_follow_the_size_setting() {
        assert_eq!(
            size_in(1_234_567, SizeFormat::Bytes),
            "1\u{a0}234\u{a0}567 B"
        );
        assert_eq!(size_in(1_234_567, SizeFormat::Kilobytes), "1\u{a0}206 KB");
        assert_eq!(size_in(1_234_567, SizeFormat::Readable), "1.2 MB");
        assert_eq!(size_in(0, SizeFormat::Kilobytes), "0 KB");
        assert_eq!(size_in(10, SizeFormat::Kilobytes), "1 KB");
        assert_eq!(
            size_in(u64::MAX, SizeFormat::Kilobytes).chars().last(),
            Some('B')
        );
    }

    #[test]
    fn permissions_show_type_bits_and_the_special_bits() {
        assert_eq!(permission_string(0o755, 'd'), "drwxr-xr-x");
        assert_eq!(permission_string(0o644, '-'), "-rw-r--r--");
        assert_eq!(permission_string(0o777, 'l'), "lrwxrwxrwx");
        assert_eq!(permission_string(0o4755, '-'), "-rwsr-xr-x");
        assert_eq!(permission_string(0o4644, '-'), "-rwSr--r--");
        assert_eq!(permission_string(0o2755, 'd'), "drwxr-sr-x");
        assert_eq!(permission_string(0o1777, 'd'), "drwxrwxrwt");
        assert_eq!(permission_string(0o1776, 'd'), "drwxrwxrwT");

        let mut entry = plain_file("a");
        assert_eq!(permissions(&entry), "");
        entry.details.mode = Some(0o640);
        assert_eq!(permissions(&entry), "-rw-r-----");
        entry.is_symlink = true;
        assert!(permissions(&entry).starts_with('l'));
    }

    #[test]
    fn windows_attributes_are_three_letters() {
        let mut entry = plain_file("a");
        let attributes = |read_only, hidden, system| {
            Some(WindowsAttributes {
                read_only,
                hidden,
                system,
            })
        };
        entry.details.windows = attributes(true, false, false);
        assert_eq!(permissions(&entry), "R--");
        entry.details.windows = attributes(false, true, false);
        assert_eq!(permissions(&entry), "-H-");
        entry.details.windows = attributes(false, false, true);
        assert_eq!(permissions(&entry), "--S");
    }

    #[test]
    fn the_extension_is_empty_for_folders_and_names_without_a_dot() {
        assert_eq!(extension(&plain_file("report.final.PDF")), "PDF");
        assert_eq!(extension(&plain_file("Makefile")), "");
        assert_eq!(extension(&plain_file(".bashrc")), "");
        let mut folder = plain_file("archive.d");
        folder.is_dir = true;
        assert_eq!(extension(&folder), "");
    }

    fn local(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Local> {
        use chrono::TimeZone;
        Local
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .single()
            .unwrap()
    }

    #[test]
    fn dates_are_written_in_the_chosen_format() {
        let now = local(2026, 11, 7, 15, 0);
        let written = |at, format| date_text(at, now, format, Language::English);

        assert_eq!(
            written(local(2026, 11, 7, 14, 3), DateFormat::Iso),
            "2026-11-07 14:03"
        );
        assert_eq!(
            written(local(2026, 11, 5, 16, 12), DateFormat::Short),
            "Nov 5 16:12"
        );
        assert_eq!(
            written(local(2025, 11, 5, 16, 12), DateFormat::Short),
            "Nov 5 2025"
        );
        assert_eq!(
            date_text(
                local(2026, 11, 5, 16, 12),
                now,
                DateFormat::Short,
                Language::German
            ),
            "05.11. 16:12"
        );
    }

    #[test]
    fn relative_dates_step_from_minutes_to_hours_to_yesterday_to_the_short_date() {
        let now = local(2026, 11, 7, 15, 0);
        let relative = |at| date_text(at, now, DateFormat::Relative, Language::English);

        assert_eq!(relative(local(2026, 11, 7, 14, 55)), "5 min ago");
        assert_eq!(relative(local(2026, 11, 7, 15, 0)), "1 min ago");
        assert_eq!(relative(local(2026, 11, 7, 12, 0)), "3 h ago");
        assert_eq!(relative(local(2026, 11, 6, 9, 30)), "yesterday 09:30");
        assert_eq!(relative(local(2026, 11, 5, 9, 30)), "Nov 5 09:30");
        assert_eq!(relative(local(2026, 11, 8, 9, 30)), "Nov 8 09:30");
    }

    #[test]
    fn a_computed_folder_shows_its_size_and_marks_a_partial_one() {
        let folder = |folder_size| FileEntry {
            name: "d".into(),
            path: "d".into(),
            is_dir: true,
            is_symlink: false,
            is_parent: false,
            size: 0,
            modified: None,
            folder_size,
            details: Default::default(),
        };
        let computed = |bytes, partial| Some(ncrs_core::FolderSize { bytes, partial });

        assert_eq!(entry_size(&folder(None)), Msg::MarkerDir.text());
        assert_eq!(entry_size(&folder(computed(1536, false))), "1.5 KB");
        assert_eq!(entry_size(&folder(computed(1536, true))), "1.5 KB+");
    }
}
