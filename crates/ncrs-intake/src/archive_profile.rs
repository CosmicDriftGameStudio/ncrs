//! Learns how an archive location is organised from the relative paths of its
//! documents, so a new document gets a target that fits. Pure: nothing here
//! touches a file or the index, and no AI is involved.
//!
//! A learned pattern uses the placeholder syntax of the filing rules:
//! `{year}`, `{month}`, `{date}`, `{number}`, plus `{folder}` for a folder
//! level that varies (a sender or category) and `{name}` for the free part of
//! a file name. Everything else is a constant. File extensions are not part of
//! a pattern; the proposal adds one.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, PathBuf};
use std::sync::OnceLock;

use chrono::{Datelike as _, NaiveDate};
use ncrs_index::{DocumentFields, SubtreePattern};
use regex::Regex;

use crate::extract::parse_date;
use crate::filing_plan::render_pattern;
use crate::sanitise::is_plain_literal;

/// A folder name must occur this often at one place before it counts as a
/// constant rather than a varying sender, and a word of a file name before it
/// counts as a recurring part rather than free text.
const MIN_RECURRING: usize = 3;
const FIRST_YEAR: i32 = 1900;
const LAST_YEAR: i32 = 2099;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveDocument {
    /// Below the root of the archive location.
    pub relative_path: PathBuf,
    /// The date field of the document, where known.
    pub date: Option<NaiveDate>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum FolderToken {
    Year,
    Month,
    Folder,
    Literal(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NameToken {
    Date,
    Number,
    Separator(char),
    Word(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    Date,
    Number,
    Name,
    Separator(char),
    Recurring(String),
}

#[derive(Clone)]
struct Parsed {
    folders: Vec<String>,
    name: Vec<NameToken>,
    date: Option<NaiveDate>,
}

/// One pattern per subtree, sorted by subtree. A subtree is named by the
/// constant folders of its documents' paths, empty for those without any. The
/// pattern of a subtree is the one most of its documents follow; a document
/// whose year folder disagrees with its known date does not follow it.
pub fn learn_archive(documents: &[ArchiveDocument]) -> Vec<SubtreePattern> {
    let parsed: Vec<Parsed> = documents.iter().filter_map(parse_document).collect();
    let folder_tokens = abstract_folders(&parsed);

    let mut groups: BTreeMap<(&[FolderToken], String), Vec<usize>> = BTreeMap::new();
    for (index, (document, folders)) in parsed.iter().zip(&folder_tokens).enumerate() {
        groups
            .entry((folders.as_slice(), shape(&document.name)))
            .or_default()
            .push(index);
    }

    // Per subtree: pattern -> (documents that follow it, whether it has structure).
    let mut subtrees: BTreeMap<String, SubtreeTally> = BTreeMap::new();
    for ((folders, _shape), members) in &groups {
        let pieces = name_pieces(&parsed, members);
        let structured = folders
            .iter()
            .any(|token| !matches!(token, FolderToken::Literal(_)))
            || pieces
                .iter()
                .any(|piece| !matches!(piece, Piece::Name | Piece::Separator(_)));
        let pattern = pattern_text(folders, &pieces);
        let followers = members
            .iter()
            .filter(|&&member| parsed.get(member).is_some_and(follows_dates))
            .count();
        let tally = subtrees.entry(subtree_of(folders)).or_default();
        tally.total += members.len();
        let entry = tally.patterns.entry(pattern).or_insert((0, structured));
        entry.0 += followers;
    }

    subtrees
        .into_iter()
        .map(|(subtree, tally)| {
            let dominant = tally
                .patterns
                .into_iter()
                .max_by_key(|(pattern, (followers, _))| (*followers, Reverse(pattern.clone())));
            let (pattern, following) = match dominant {
                Some((pattern, (followers, true))) if followers > 0 => (Some(pattern), followers),
                _ => (None, 0),
            };
            SubtreePattern {
                subtree,
                pattern,
                documents: u32::try_from(following).unwrap_or(u32::MAX),
                total: u32::try_from(tally.total).unwrap_or(u32::MAX),
            }
        })
        .collect()
}

/// The subtree each document belongs to, in the order of `documents`; `None`
/// for a path that cannot be a document of the location. The same assignment
/// `learn_archive` counts with.
pub fn subtree_per_document(documents: &[ArchiveDocument]) -> Vec<Option<String>> {
    let parsed: Vec<Option<Parsed>> = documents.iter().map(parse_document).collect();
    let present: Vec<Parsed> = parsed.iter().flatten().cloned().collect();
    let mut folders = abstract_folders(&present).into_iter();
    parsed
        .iter()
        .map(|document| {
            document
                .as_ref()
                .and_then(|_| folders.next())
                .map(|tokens| subtree_of(&tokens))
        })
        .collect()
}

#[derive(Default)]
struct SubtreeTally {
    total: usize,
    patterns: BTreeMap<String, (usize, bool)>,
}

/// The path a learned subtree gives a document with these fields, below the
/// root of the location. Values are sanitised like in a filing rule. `None`
/// when the subtree has no pattern, a field the pattern needs is missing, or
/// `extension` is not plain letters and digits.
pub fn propose_target(
    subtree: &SubtreePattern,
    fields: &DocumentFields,
    extension: &str,
) -> Option<PathBuf> {
    let pattern = subtree.pattern.as_deref()?;
    let plain_extension = !extension.is_empty()
        && extension
            .chars()
            .all(|character| character.is_ascii_alphanumeric());
    if !plain_extension {
        return None;
    }
    render_pattern(&format!("{pattern}.{extension}"), fields)
}

fn parse_document(document: &ArchiveDocument) -> Option<Parsed> {
    let mut folders = Vec::new();
    let mut components = document.relative_path.components().peekable();
    let mut file_stem = None;
    while let Some(component) = components.next() {
        let Component::Normal(part) = component else {
            return None;
        };
        let text = part.to_str()?;
        if components.peek().is_some() {
            folders.push(text.to_owned());
        } else {
            file_stem = Some(
                document
                    .relative_path
                    .file_stem()
                    .and_then(|stem_os| stem_os.to_str())
                    .unwrap_or(text)
                    .to_owned(),
            );
        }
    }
    Some(Parsed {
        folders,
        name: name_tokens(&file_stem?),
        date: document.date,
    })
}

/// A document follows the year folders of its path when its known date is in
/// that year.
fn follows_dates(document: &Parsed) -> bool {
    let Some(date) = document.date else {
        return true;
    };
    document
        .folders
        .iter()
        .filter_map(|folder| year_of(folder))
        .all(|year| year == date.year())
}

fn year_of(folder: &str) -> Option<i32> {
    if folder.len() != 4 || !folder.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    folder
        .parse()
        .ok()
        .filter(|year| (FIRST_YEAR..=LAST_YEAR).contains(year))
}

fn is_month(folder: &str) -> bool {
    folder.len() == 2
        && folder
            .parse::<u32>()
            .is_ok_and(|month| (1..=12).contains(&month))
}

/// A folder name that cannot be a constant piece of a pattern.
fn is_pattern_safe(name: &str) -> bool {
    is_plain_literal(name) && !name.contains(['{', '}'])
}

/// Folder levels as years, months, constants and varying names. Level by
/// level, so that a varying level merges the paths below it.
fn abstract_folders(parsed: &[Parsed]) -> Vec<Vec<FolderToken>> {
    let mut tokens: Vec<Vec<FolderToken>> = vec![Vec::new(); parsed.len()];
    let depth = parsed
        .iter()
        .map(|doc| doc.folders.len())
        .max()
        .unwrap_or(0);
    for level in 0..depth {
        let mut names_at: BTreeMap<Vec<FolderToken>, BTreeSet<&str>> = BTreeMap::new();
        for (document, prefix) in parsed.iter().zip(&tokens) {
            if let Some(name) = document.folders.get(level) {
                if classify_date_folder(name, prefix.last()).is_none() {
                    names_at.entry(prefix.clone()).or_default().insert(name);
                }
            }
        }
        for (document, prefix) in parsed.iter().zip(tokens.iter_mut()) {
            let Some(name) = document.folders.get(level) else {
                continue;
            };
            let token = classify_date_folder(name, prefix.last()).unwrap_or_else(|| {
                let varies = names_at
                    .get(prefix)
                    .is_some_and(|names| names.len() >= MIN_RECURRING);
                if varies || !is_pattern_safe(name) {
                    FolderToken::Folder
                } else {
                    FolderToken::Literal(name.clone())
                }
            });
            prefix.push(token);
        }
    }
    tokens
}

/// A month is only a month right below a year.
fn classify_date_folder(name: &str, previous: Option<&FolderToken>) -> Option<FolderToken> {
    if year_of(name).is_some() {
        Some(FolderToken::Year)
    } else if previous == Some(&FolderToken::Year) && is_month(name) {
        Some(FolderToken::Month)
    } else {
        None
    }
}

/// The constant folders of the path, joined by `/`; years, months and varying
/// levels are skipped. A constant path is a subtree of its own: if sibling
/// folders (`Insurance/Company A`, `Insurance/Agency B`) shared one, their
/// patterns would compete and the smaller folder's documents would look like
/// outliers.
fn subtree_of(folders: &[FolderToken]) -> String {
    folders
        .iter()
        .filter_map(|token| match token {
            FolderToken::Literal(name) => Some(name.as_str()),
            FolderToken::Year | FolderToken::Month | FolderToken::Folder => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn date_regex() -> Option<&'static Regex> {
    static DATE: OnceLock<Option<Regex>> = OnceLock::new();
    DATE.get_or_init(|| Regex::new(r"\d{4}-\d{2}-\d{2}|\d{2}\.\d{2}\.\d{4}|\d{8}").ok())
        .as_ref()
}

/// `2024-03-01`, `01.03.2024` or `20240301` as a date.
fn date_in(stem: &str) -> Option<(usize, usize)> {
    let bytes = stem.as_bytes();
    date_regex()?.find_iter(stem).find_map(|found| {
        let digit_before = found
            .start()
            .checked_sub(1)
            .and_then(|at| bytes.get(at))
            .is_some_and(u8::is_ascii_digit);
        let digit_after = bytes.get(found.end()).is_some_and(u8::is_ascii_digit);
        let text = found.as_str();
        let normalised = match (text.get(..4), text.get(4..6), text.get(6..8)) {
            (Some(year), Some(month), Some(day)) if text.len() == 8 => {
                format!("{year}-{month}-{day}")
            }
            _ => text.to_owned(),
        };
        (!digit_before && !digit_after && parse_date(&normalised).is_some())
            .then_some((found.start(), found.end()))
    })
}

fn name_tokens(stem: &str) -> Vec<NameToken> {
    let (before, date, after) = match date_in(stem) {
        Some((start, end)) => (
            stem.get(..start).unwrap_or_default(),
            true,
            stem.get(end..).unwrap_or_default(),
        ),
        None => (stem, false, ""),
    };
    let mut tokens = words_of(before);
    if date {
        tokens.push(NameToken::Date);
    }
    tokens.extend(words_of(after));
    tokens
}

fn words_of(text: &str) -> Vec<NameToken> {
    let mut tokens = Vec::new();
    let mut word = String::new();
    let flush = |pending: &mut String, found: &mut Vec<NameToken>| {
        if pending.is_empty() {
            return;
        }
        let finished = std::mem::take(pending);
        found.push(if finished.bytes().all(|byte| byte.is_ascii_digit()) {
            NameToken::Number
        } else {
            NameToken::Word(finished)
        });
    };
    for character in text.chars() {
        if matches!(character, '_' | '-' | ' ') {
            flush(&mut word, &mut tokens);
            tokens.push(NameToken::Separator(character));
        } else {
            word.push(character);
        }
    }
    flush(&mut word, &mut tokens);
    tokens
}

/// The structure of a file name with the words masked.
fn shape(tokens: &[NameToken]) -> String {
    tokens
        .iter()
        .map(|token| match token {
            NameToken::Date => "\u{1}".to_owned(),
            NameToken::Number => "\u{2}".to_owned(),
            NameToken::Word(_) => "\u{3}".to_owned(),
            NameToken::Separator(character) => character.to_string(),
        })
        .collect()
}

/// The name pattern of documents with the same shape. A word that is the same
/// in all of them, and there are enough of them, stays a constant.
fn name_pieces(parsed: &[Parsed], members: &[usize]) -> Vec<Piece> {
    let Some(first) = members.first().and_then(|&member| parsed.get(member)) else {
        return Vec::new();
    };
    let pieces = first
        .name
        .iter()
        .enumerate()
        .map(|(position, token)| match token {
            NameToken::Date => Piece::Date,
            NameToken::Number => Piece::Number,
            NameToken::Separator(character) => Piece::Separator(*character),
            NameToken::Word(word) => {
                let same_everywhere = members.iter().all(|&member| {
                    parsed
                        .get(member)
                        .and_then(|doc| doc.name.get(position))
                        .is_some_and(|other| *other == NameToken::Word(word.clone()))
                });
                if members.len() >= MIN_RECURRING && same_everywhere && is_pattern_safe(word) {
                    Piece::Recurring(word.clone())
                } else {
                    Piece::Name
                }
            }
        })
        .collect();
    merge_names(pieces)
}

/// Free words next to each other, however they are separated, are one name.
fn merge_names(pieces: Vec<Piece>) -> Vec<Piece> {
    let mut merged: Vec<Piece> = Vec::new();
    for piece in pieces {
        if piece == Piece::Name {
            let separators = merged
                .iter()
                .rev()
                .take_while(|earlier| matches!(earlier, Piece::Separator(_)))
                .count();
            let before = merged.len().saturating_sub(separators);
            let after_name = before
                .checked_sub(1)
                .and_then(|at| merged.get(at))
                .is_some_and(|earlier| *earlier == Piece::Name);
            if after_name {
                merged.truncate(before);
                continue;
            }
        }
        merged.push(piece);
    }
    merged
}

fn pattern_text(folders: &[FolderToken], pieces: &[Piece]) -> String {
    let mut parts: Vec<String> = folders
        .iter()
        .map(|token| match token {
            FolderToken::Year => "{year}".to_owned(),
            FolderToken::Month => "{month}".to_owned(),
            FolderToken::Folder => "{folder}".to_owned(),
            FolderToken::Literal(name) => name.clone(),
        })
        .collect();
    let file_name: String = pieces
        .iter()
        .map(|piece| match piece {
            Piece::Date => "{date}".to_owned(),
            Piece::Number => "{number}".to_owned(),
            Piece::Name => "{name}".to_owned(),
            Piece::Separator(character) => character.to_string(),
            Piece::Recurring(word) => word.clone(),
        })
        .collect();
    parts.push(if file_name.is_empty() {
        "{name}".to_owned()
    } else {
        file_name
    });
    parts.join("/")
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
    use crate::sanitise::stays_below;

    fn document(path: &str) -> ArchiveDocument {
        ArchiveDocument {
            relative_path: PathBuf::from(path),
            date: None,
        }
    }

    fn dated(path: &str, year: i32, month: u32, day: u32) -> ArchiveDocument {
        ArchiveDocument {
            date: NaiveDate::from_ymd_opt(year, month, day),
            ..document(path)
        }
    }

    fn documents(paths: &[&str]) -> Vec<ArchiveDocument> {
        paths.iter().map(|path| document(path)).collect()
    }

    fn only(learned: &[SubtreePattern], subtree: &str) -> SubtreePattern {
        learned
            .iter()
            .find(|entry| entry.subtree == subtree)
            .unwrap_or_else(|| panic!("no subtree {subtree:?} in {learned:?}"))
            .clone()
    }

    fn year_category_tree() -> Vec<ArchiveDocument> {
        let mut tree = Vec::new();
        for year in [2023, 2024] {
            for sender in ["Company A", "Agency B", "Company C"] {
                for (day, topic) in [(1, "premium"), (2, "renewal")] {
                    tree.push(document(&format!(
                        "{year}/Insurance/{sender}/{year}-03-0{day}_{topic}.pdf"
                    )));
                }
            }
        }
        tree
    }

    #[test]
    fn a_year_and_category_tree_yields_the_pattern_and_the_count() {
        let learned = learn_archive(&year_category_tree());

        assert_eq!(
            learned,
            [SubtreePattern {
                subtree: "Insurance".to_owned(),
                pattern: Some("{year}/Insurance/{folder}/{date}_{name}".to_owned()),
                documents: 12,
                total: 12,
            }]
        );
    }

    #[test]
    fn sibling_sender_folders_are_subtrees_of_their_own() {
        let mut tree = Vec::new();
        for year in [2023, 2024] {
            for (sender, topics) in [
                ("Company A", ["premium", "renewal"]),
                ("Agency B", ["notice", "claim"]),
            ] {
                for (day, topic) in topics.iter().enumerate() {
                    tree.push(document(&format!(
                        "{year}/Insurance/{sender}/{year}-03-0{}_{topic}.pdf",
                        day + 1
                    )));
                }
            }
        }

        let learned = learn_archive(&tree);

        let subtrees: Vec<_> = learned.iter().map(|entry| entry.subtree.as_str()).collect();
        assert_eq!(subtrees, ["Insurance/Agency B", "Insurance/Company A"]);
        for entry in &learned {
            assert_eq!(
                entry.pattern.as_deref(),
                Some(format!("{{year}}/{}/{{date}}_{{name}}", entry.subtree).as_str())
            );
            assert_eq!((entry.documents, entry.total), (4, 4));
        }
    }

    #[test]
    fn month_folders_below_a_year_are_months() {
        let learned = learn_archive(&documents(&[
            "2024/01/Invoice_1001.pdf",
            "2024/02/Invoice_1002.pdf",
            "2023/12/Invoice_1003.pdf",
        ]));

        assert_eq!(
            only(&learned, "").pattern.as_deref(),
            Some("{year}/{month}/Invoice_{number}")
        );
    }

    #[test]
    fn mixed_date_formats_are_recognised() {
        let learned = learn_archive(&documents(&[
            "Taxes/2024-03-01_receipt.pdf",
            "Taxes/20240302_letter.pdf",
            "Taxes/03.03.2024_notice.pdf",
        ]));

        let taxes = only(&learned, "Taxes");
        assert_eq!(taxes.pattern.as_deref(), Some("Taxes/{date}_{name}"));
        assert_eq!((taxes.documents, taxes.total), (3, 3));
    }

    #[test]
    fn a_number_that_is_no_date_is_not_read_as_one() {
        let learned = learn_archive(&documents(&[
            "Taxes/20241345_a.pdf",
            "Taxes/20241346_b.pdf",
            "Taxes/20241347_c.pdf",
        ]));

        assert_eq!(
            only(&learned, "Taxes").pattern.as_deref(),
            Some("Taxes/{number}_{name}")
        );
    }

    #[test]
    fn a_recurring_word_stays_and_a_free_word_becomes_a_name() {
        let learned = learn_archive(&documents(&[
            "Insurance/Policy_1001_house.pdf",
            "Insurance/Policy_1002_car.pdf",
            "Insurance/Policy_1003_boat.pdf",
        ]));

        assert_eq!(
            only(&learned, "Insurance").pattern.as_deref(),
            Some("Insurance/Policy_{number}_{name}")
        );
    }

    #[test]
    fn a_subtree_without_a_common_structure_is_reported_without_a_pattern() {
        let mut tree = year_category_tree();
        tree.extend(documents(&[
            "Misc/holiday.pdf",
            "Misc/notes.pdf",
            "Misc/recipe.pdf",
        ]));

        let learned = learn_archive(&tree);

        assert_eq!(
            only(&learned, "Misc"),
            SubtreePattern {
                subtree: "Misc".to_owned(),
                pattern: None,
                documents: 0,
                total: 3,
            }
        );
        assert!(only(&learned, "Insurance").pattern.is_some());
    }

    #[test]
    fn a_document_in_the_wrong_year_folder_does_not_follow_the_pattern() {
        let mut tree = vec![
            dated(
                "2024/Insurance/Company A/2024-03-01_premium.pdf",
                2024,
                3,
                1,
            ),
            dated(
                "2024/Insurance/Company A/2024-04-01_renewal.pdf",
                2024,
                4,
                1,
            ),
            dated("2024/Insurance/Company A/2024-05-01_notice.pdf", 2024, 5, 1),
        ];
        tree.push(dated(
            "2024/Insurance/Company A/2023-05-01_letter.pdf",
            2023,
            5,
            1,
        ));

        let insurance = only(&learn_archive(&tree), "Insurance/Company A");

        assert_eq!((insurance.documents, insurance.total), (3, 4));
    }

    #[test]
    fn paths_that_could_leave_the_location_are_left_out() {
        let learned = learn_archive(&documents(&["../outside/2024-01-01_a.pdf", "/abs/b.pdf"]));
        assert!(learned.is_empty());
    }

    fn insurance_pattern() -> SubtreePattern {
        only(&learn_archive(&year_category_tree()), "Insurance")
    }

    fn fields(sender: &str, kind: &str) -> DocumentFields {
        DocumentFields {
            sender: Some(sender.to_owned()),
            kind: Some(kind.to_owned()),
            date: NaiveDate::from_ymd_opt(2025, 6, 9),
            ..DocumentFields::default()
        }
    }

    #[test]
    fn the_target_for_a_new_document_follows_the_pattern() {
        let target = propose_target(&insurance_pattern(), &fields("Agency D", "premium"), "pdf");

        assert_eq!(
            target,
            Some(PathBuf::from(
                "2025/Insurance/Agency D/2025-06-09_premium.pdf"
            ))
        );
    }

    #[test]
    fn no_target_without_a_pattern_a_needed_field_or_a_plain_extension() {
        let pattern = insurance_pattern();
        let no_pattern = SubtreePattern {
            pattern: None,
            ..pattern.clone()
        };
        let no_date = DocumentFields {
            date: None,
            ..fields("Agency D", "premium")
        };

        assert_eq!(
            propose_target(&no_pattern, &fields("Agency D", "premium"), "pdf"),
            None
        );
        assert_eq!(propose_target(&pattern, &no_date, "pdf"), None);
        assert_eq!(
            propose_target(&pattern, &fields("Agency D", "premium"), "pdf/../x"),
            None
        );
    }

    #[test]
    fn a_value_with_separators_or_parent_references_cannot_leave_the_location() {
        let pattern = insurance_pattern();
        for hostile in ["../../etc", "a/../../b", "..\\..\\c", "/", "..", "x/y"] {
            let target = propose_target(&pattern, &fields(hostile, hostile), "pdf");
            if let Some(path) = target {
                assert!(stays_below(&path), "{hostile}: {path:?}");
                assert!(path.starts_with("2025/Insurance"), "{hostile}: {path:?}");
                assert_eq!(path.components().count(), 4, "{hostile}: {path:?}");
            }
        }
        assert_eq!(
            propose_target(&pattern, &fields("..", "/"), "pdf"),
            None,
            "values of only dots or separators count as missing"
        );
    }
}
