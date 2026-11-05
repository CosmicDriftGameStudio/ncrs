//! Reads the date of a letter from its text: the date line of the letter head
//! first, then a labelled date, then the newest date near the top. The text is
//! only searched, never acted on.

use std::ops::Range;
use std::sync::LazyLock;

use chrono::{Datelike as _, NaiveDate};
use ncrs_index::compile_bounded_regex;
use regex::Regex;

use crate::extract::parse_date;

const YEARS: Range<i32> = 1900..2100;
/// A letter head with address blocks is longer than a third of a short letter.
const MIN_HEADER_LINES: usize = 12;
/// Longest `<place>,` in front of a date in the letter head.
const MAX_PLACE_CHARS: usize = 40;
const DATE_LABELS: [&str; 5] = [
    "rechnungsdatum",
    "belegdatum",
    "invoice date",
    "datum",
    "date",
];
/// What stands in front of the owner's birth date, which is never the date of
/// a letter.
const BIRTH_LABELS: [&str; 10] = [
    "geb.",
    "geboren",
    "geburtsdatum",
    "geb.-datum",
    "geb.datum",
    "geburtstag",
    "date of birth",
    "birth date",
    "born",
    "dob",
];
/// How far in front of a date on its line a birth label still belongs to it.
const BIRTH_LABEL_REACH: usize = 30;
const GERMAN_MONTHS: [(&str, u32); 13] = [
    ("januar", 1),
    ("februar", 2),
    ("märz", 3),
    ("maerz", 3),
    ("april", 4),
    ("mai", 5),
    ("juni", 6),
    ("juli", 7),
    ("august", 8),
    ("september", 9),
    ("oktober", 10),
    ("november", 11),
    ("dezember", 12),
];

static DATE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile_bounded_regex(
        r"\b(?:(?P<numeric>\d{1,2}\.\d{1,2}\.\d{4}|\d{4}-\d{2}-\d{2}|\d{1,2}/\d{1,2}/\d{4})|(?P<day>\d{1,2})\.\s*(?P<month>\p{L}+)\s+(?P<year>\d{4}))\b",
    )
    .ok()
});

struct FoundDate {
    range: Range<usize>,
    date: NaiveDate,
}

fn dates_in(line: &str) -> Vec<FoundDate> {
    let Some(regex) = DATE.as_ref() else {
        return Vec::new();
    };
    regex
        .captures_iter(line)
        .filter_map(|captures| {
            let whole = captures.get(0)?;
            let date = if let Some(numeric) = captures.name("numeric") {
                parse_date(numeric.as_str())?
            } else {
                let month_word = captures.name("month")?.as_str().to_lowercase();
                let month = GERMAN_MONTHS
                    .iter()
                    .find(|(name, _)| *name == month_word)
                    .map(|(_, number)| *number)?;
                NaiveDate::from_ymd_opt(
                    captures.name("year")?.as_str().parse().ok()?,
                    month,
                    captures.name("day")?.as_str().parse().ok()?,
                )?
            };
            YEARS.contains(&date.year()).then_some(FoundDate {
                range: whole.range(),
                date,
            })
        })
        .collect()
}

/// `text` ends with a birth label that is a word of its own.
fn ends_with_birth_label(text: &str) -> bool {
    let lowered = text.trim_end_matches([':', ' ', '\t']).to_ascii_lowercase();
    BIRTH_LABELS.iter().any(|label| {
        lowered.strip_suffix(label).is_some_and(|rest| {
            rest.chars().next_back().is_none_or(|c| !c.is_alphabetic())
        })
    })
}

/// The date at `start` of `line` is labelled as a birth date: on the same line
/// in front of it, or alone on its line under a line that ends with the label.
fn is_birth_date(line: &str, start: usize, previous: Option<&str>) -> bool {
    let before = line.get(..start).unwrap_or_default();
    if before.trim().is_empty() {
        return previous.is_some_and(ends_with_birth_label);
    }
    let reach_start = before
        .char_indices()
        .rev()
        .nth(BIRTH_LABEL_REACH)
        .map_or(0, |(index, _)| index);
    let near = before.get(reach_start..).unwrap_or(before);
    // The label may be followed by a word such as `am`.
    let words: Vec<&str> = near.split_whitespace().collect();
    let tail_from = words.len().saturating_sub(3);
    (0..=words.len() - tail_from).any(|dropped| {
        let kept = words.len() - dropped;
        kept > 0 && ends_with_birth_label(&words[..kept].join(" "))
    })
}

/// A line that is only a date or `<place>, <date>`.
fn header_date(line: &str, previous: Option<&str>) -> Option<NaiveDate> {
    let [found] = dates_in(line).try_into().ok()?;
    if is_birth_date(line, found.range.start, previous) {
        return None;
    }
    let before = line.get(..found.range.start)?.trim();
    let after = line.get(found.range.end..)?.trim();
    let place = before.strip_suffix(',').map(str::trim);
    let is_place = place.is_some_and(|name| {
        !name.is_empty()
            && name.chars().count() <= MAX_PLACE_CHARS
            && !name.chars().any(|c| c.is_ascii_digit())
    });
    (after.is_empty() && (before.is_empty() || is_place)).then_some(found.date)
}

/// The date right behind a label such as `Datum:` or `Rechnungsdatum `.
fn labelled_date(line: &str, previous: Option<&str>) -> Option<NaiveDate> {
    // ASCII lowercase keeps every byte offset of the line.
    let lowered = line.to_ascii_lowercase();
    DATE_LABELS.iter().find_map(|label| {
        lowered.match_indices(label).find_map(|(start, _)| {
            let is_word_start = line
                .get(..start)
                .and_then(|before| before.chars().next_back())
                .is_none_or(|c| !c.is_alphabetic());
            if !is_word_start {
                return None;
            }
            let rest = line.get(start + label.len()..)?;
            let value = rest.trim_start_matches([':', ' ', '\t']);
            let found = dates_in(value).into_iter().next()?;
            let at_label = found.range.start == 0;
            let from_line_start = line.len() - value.len();
            (at_label && !is_birth_date(line, from_line_start, previous)).then_some(found.date)
        })
    })
}

/// The date of the letter, `None` when the text names no plausible one.
pub fn detect_letter_date(text: &str) -> Option<NaiveDate> {
    let mut previous_line = None;
    let lines: Vec<(&str, Option<&str>)> = text
        .lines()
        .map(|line| {
            let previous = previous_line;
            if !line.trim().is_empty() {
                previous_line = Some(line);
            }
            (line, previous)
        })
        .collect();
    let header_len = lines.len().div_ceil(3).max(MIN_HEADER_LINES);
    let header = || lines.iter().take(header_len).copied();
    header()
        .find_map(|(line, previous)| header_date(line, previous))
        .or_else(|| {
            lines
                .iter()
                .find_map(|&(line, previous)| labelled_date(line, previous))
        })
        .or_else(|| {
            header()
                .flat_map(|(line, previous)| {
                    dates_in(line)
                        .into_iter()
                        .filter(move |found| !is_birth_date(line, found.range.start, previous))
                        .map(|found| found.date)
                })
                .max()
        })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::test_archive::{test_archive, TestFile};

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn the_date_line_of_the_head_beats_a_labelled_date_and_newer_dates() {
        let text = "Muster GmbH\n\nChemnitz, 28.01.2019\n\nRechnung\nRechnungsdatum: 01.02.2019\nZahlbar bis 15.03.2019\n";
        assert_eq!(detect_letter_date(text), Some(date(2019, 1, 28)));
        assert_eq!(
            detect_letter_date("12.05.2020\nHallo"),
            Some(date(2020, 5, 12))
        );
    }

    #[test]
    fn a_labelled_date_beats_the_newest_date() {
        let mut text =
            String::from("Muster GmbH\nRechnung vom Vertrag 01.01.2022 bis 31.12.2023\n");
        text.push_str(&"Zeile\n".repeat(20));
        text.push_str("Rechnungsdatum: 14.03.2021\n");
        assert_eq!(detect_letter_date(&text), Some(date(2021, 3, 14)));
        assert_eq!(
            detect_letter_date("Invoice date 2020-02-29\n"),
            Some(date(2020, 2, 29))
        );
        assert_eq!(
            detect_letter_date("Lieferdatum: 01.01.2020\nBelegdatum 02.01.2020\n"),
            Some(date(2020, 1, 2))
        );
    }

    #[test]
    fn without_header_or_label_the_newest_date_near_the_top_wins() {
        let text = "Vertrag\nlaufzeit von 01.01.2020 bis 31.12.2022 verlaengert\n";
        assert_eq!(detect_letter_date(text), Some(date(2022, 12, 31)));
    }

    #[test]
    fn german_long_forms_are_read() {
        assert_eq!(
            detect_letter_date("Berlin, 3. März 2024\n"),
            Some(date(2024, 3, 3))
        );
        assert_eq!(
            detect_letter_date("03. Maerz 2024\n"),
            Some(date(2024, 3, 3))
        );
        for (month, number) in GERMAN_MONTHS {
            let text = format!("Ort, 5. {} 2020\n", month.to_uppercase());
            assert_eq!(detect_letter_date(&text), Some(date(2020, number, 5)));
        }
    }

    #[test]
    fn impossible_and_implausible_dates_are_ignored() {
        assert_eq!(detect_letter_date("Ort, 31.02.2020\n"), None);
        assert_eq!(detect_letter_date("Ort, 01.01.1850\n"), None);
        assert_eq!(detect_letter_date("Ort, 01.01.2150\n"), None);
        assert_eq!(
            detect_letter_date("Ort, 31.02.2020\nDatum: 05.06.2020\n"),
            Some(date(2020, 6, 5))
        );
    }

    #[test]
    fn a_birth_date_is_never_the_letter_date() {
        let text = "Max Muster, geb. 21.10.1978\nBerlin, 03.05.2021\nBetreff\n";
        assert_eq!(detect_letter_date(text), Some(date(2021, 5, 3)));
        let labelled = "Geburtsdatum: 21.10.1978\nRechnungsdatum: 14.03.2021\n";
        assert_eq!(detect_letter_date(labelled), Some(date(2021, 3, 14)));
        let newest = "Vertrag\nGeboren am 21.10.1978, laufzeit ab 01.01.2020\n";
        assert_eq!(detect_letter_date(newest), Some(date(2020, 1, 1)));
        assert_eq!(detect_letter_date("Date of birth: 1978-10-21\n"), None);
        assert_eq!(detect_letter_date("geb. 21.10.1978\n"), None);
        assert_eq!(detect_letter_date("Name\nGeburtsdatum\n21.10.1978\n"), None);
        assert_eq!(detect_letter_date("Geb.-Datum: 21.10.1978\n"), None);
        assert_eq!(
            detect_letter_date("Reborn GmbH\n21.10.2020\n"),
            Some(date(2020, 10, 21)),
            "a label inside another word is no label"
        );
    }

    #[test]
    fn a_text_without_a_date_has_none() {
        assert_eq!(detect_letter_date(""), None);
        assert_eq!(detect_letter_date("Guten Tag\nKundennummer 12.34\n"), None);
    }

    fn check_all(files: &[TestFile]) {
        let mut checked = 0;
        for file in files {
            let Some(text) = file.text() else { continue };
            assert_eq!(
                detect_letter_date(text),
                file.date,
                "{}",
                file.relative_path.display()
            );
            checked += 1;
        }
        assert!(checked > 0);
    }

    #[test]
    fn the_dates_of_the_test_archive_letters_are_found() {
        let archive = test_archive();
        check_all(&archive.archive);
        check_all(&archive.inbox);
    }
}
