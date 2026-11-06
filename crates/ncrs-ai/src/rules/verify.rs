//! Does a pattern write the paths the files already have? The same check
//! tells the tests whether a recognized pattern is right and tells Hermes when
//! to ask: a pattern is certain when this list is empty.

use ncrs_index::{DocumentFields, Kinds};
use ncrs_intake::render_pattern_in;

use super::levels::Levels;

/// How many deviating files the repair note names.
const REPAIR_FILES: usize = 3;
/// Letters a shared run of the file names needs to count as a pattern.
const WORDY_RUN: usize = 3;
/// Said to the model when its answer left the file name automatic although
/// the files have names of their own.
const FILE_NAME_NOTE: &str =
    "The file names follow a pattern. Describe it in datei; datei null is not the answer here.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    /// The path the file has.
    pub file: String,
    /// What the pattern writes for it, or `None` when it cannot write one.
    pub written: Option<String>,
}

/// What the model is told when its pattern does not write the paths: the
/// pattern, and for some files what it writes against where they lie.
pub fn repair_note(pattern: &str, misfits: &[Mismatch]) -> String {
    let mut note = format!("Your answer {pattern} does not write these paths:");
    for misfit in misfits.iter().take(REPAIR_FILES) {
        match &misfit.written {
            Some(written) => note.push_str(&format!(
                "\n- the pattern gives \"{written}\", the file lies at \"{}\"",
                misfit.file
            )),
            None => note.push_str(&format!(
                "\n- the pattern cannot write a path for \"{}\"",
                misfit.file
            )),
        }
    }
    note.push_str("\nAnswer with a pattern that writes the paths as they are.");
    note
}

/// [`repair_note`] for the answer `previous`; an automatic file name is
/// pointed out as well, whether or not paths deviate.
pub fn repair_note_for(previous: &Levels, misfits: &[Mismatch]) -> String {
    let mut parts = Vec::new();
    if !misfits.is_empty() {
        parts.push(repair_note(&previous.pattern(), misfits));
    }
    if previous.file.is_none() {
        parts.push(FILE_NAME_NOTE.to_owned());
    }
    parts.join("\n")
}

/// The files are named after a pattern of their own: at least two different
/// names share a leading or trailing run with words in it, as
/// `Gehaltsabrechnung_` or `_Rechnung`. Only the structure is looked at, the
/// model writes the level.
pub fn file_names_share_structure(files: &[(String, DocumentFields)]) -> bool {
    let stems: Vec<&str> = files
        .iter()
        .map(|(path, _)| {
            let name = path.rsplit('/').next().unwrap_or(path);
            match name.rsplit_once('.') {
                Some((stem, extension)) if is_extension(&format!(".{extension}")) => stem,
                _ => name,
            }
        })
        .collect();
    let Some(first) = stems.first() else {
        return false;
    };
    if stems.iter().all(|stem| stem == first) {
        return false;
    }
    let lead: Vec<char> = first
        .chars()
        .enumerate()
        .take_while(|(index, ch)| {
            stems
                .iter()
                .all(|stem| stem.chars().nth(*index) == Some(*ch))
        })
        .map(|(_, ch)| ch)
        .collect();
    let tail: Vec<char> = first
        .chars()
        .rev()
        .enumerate()
        .take_while(|(index, ch)| {
            stems
                .iter()
                .all(|stem| stem.chars().rev().nth(*index) == Some(*ch))
        })
        .map(|(_, ch)| ch)
        .collect();
    [lead, tail]
        .iter()
        .any(|run| run.iter().filter(|ch| ch.is_alphabetic()).count() >= WORDY_RUN)
}

pub fn path_text(path: &std::path::Path) -> String {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// `rest` is a file extension: a dot and letters or digits.
fn is_extension(rest: &str) -> bool {
    rest.strip_prefix('.').is_some_and(|extension| {
        !extension.is_empty() && extension.chars().all(|ch| ch.is_ascii_alphanumeric())
    })
}

/// The file has the path the pattern writes, with or without its extension.
fn is_written_path(file: &str, written: &str) -> bool {
    file.strip_prefix(written)
        .is_some_and(|rest| rest.is_empty() || is_extension(rest))
}

/// The files the pattern does not write: `files` are the paths the files
/// have, with the fields of each document.
pub fn pattern_mismatches(
    pattern: &str,
    files: &[(String, DocumentFields)],
    kinds: &Kinds,
) -> Vec<Mismatch> {
    files
        .iter()
        .filter_map(|(file, fields)| {
            let written = render_pattern_in(pattern, fields, kinds).map(|path| path_text(&path));
            let fits = written
                .as_deref()
                .is_some_and(|text| is_written_path(file, text));
            (!fits).then(|| Mismatch {
                file: file.clone(),
                written,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::indexing_slicing,
        clippy::shadow_unrelated,
        clippy::unwrap_used
    )]

    use ncrs_index::KindLanguage;
    use ncrs_intake::parse_date;

    use super::*;

    fn files() -> (Vec<(String, DocumentFields)>, Kinds) {
        let fields = DocumentFields {
            sender: Some("Otto GmbH".to_owned()),
            number: Some("5512".to_owned()),
            date: parse_date("2021-04-02"),
            ..DocumentFields::default()
        };
        (
            vec![(
                "2021/Otto GmbH/2021-04-02_Otto-GmbH_5512.pdf".to_owned(),
                fields,
            )],
            Kinds::built_in(KindLanguage::German),
        )
    }

    #[test]
    fn a_pattern_that_writes_every_path_has_no_mismatch() {
        let (files, kinds) = files();
        assert!(pattern_mismatches("{year}/{sender}/", &files, &kinds).is_empty());
    }

    #[test]
    fn a_pattern_that_writes_something_else_lists_the_file() {
        let (files, kinds) = files();
        let found = pattern_mismatches("{year}/{recipient}/", &files, &kinds);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].written, None);
        let found = pattern_mismatches("{sender}/{year}/", &files, &kinds);
        assert_eq!(
            found[0]
                .written
                .as_deref()
                .map(|w| w.starts_with("Otto GmbH/2021")),
            Some(true)
        );
    }

    fn named(names: &[&str]) -> Vec<(String, DocumentFields)> {
        names
            .iter()
            .map(|name| ((*name).to_owned(), DocumentFields::default()))
            .collect()
    }

    #[test]
    fn names_with_a_shared_word_follow_a_pattern() {
        assert!(file_names_share_structure(&named(&[
            "2023/Gehaltsabrechnung_2023-05-25.pdf",
            "2024/Gehaltsabrechnung_2024-01-31.pdf",
        ])));
        assert!(file_names_share_structure(&named(&[
            "2023-05-25_Rechnung.pdf",
            "2024-01-31_Rechnung.pdf",
        ])));
    }

    #[test]
    fn names_of_dates_or_a_single_name_show_no_structure() {
        assert!(!file_names_share_structure(&named(&[
            "2020-01-10.pdf",
            "2020-03-11.pdf"
        ])));
        assert!(!file_names_share_structure(&named(&["Rechnung_1.pdf"])));
        assert!(!file_names_share_structure(&named(&[
            "a/Rechnung_1.pdf",
            "b/Rechnung_1.pdf"
        ])));
    }

    #[test]
    fn the_repair_note_points_out_an_automatic_file_name() {
        let automatic = Levels::from_pattern("{year}/").unwrap();
        let named_file = Levels::from_pattern("{year}/{date}").unwrap();
        assert!(repair_note_for(&automatic, &[]).contains("datei"));
        assert!(!repair_note_for(&named_file, &[]).contains("datei"));
        let misfit = Mismatch {
            file: "a.pdf".to_owned(),
            written: None,
        };
        let note = repair_note_for(&automatic, &[misfit]);
        assert!(note.contains("does not write these paths") && note.contains("datei"));
    }

    #[test]
    fn a_file_name_written_by_hand_may_have_dots() {
        let (mut files, kinds) = files();
        files[0].0 = "2021.04.pdf".to_owned();
        assert!(pattern_mismatches("{year}.{month}", &files, &kinds).is_empty());
        files[0].0 = "2021.04.02.pdf".to_owned();
        assert_eq!(
            pattern_mismatches("{year}.{month}", &files, &kinds).len(),
            1
        );
    }
}
