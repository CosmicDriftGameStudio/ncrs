//! Does a pattern write the paths the files already have? The same check
//! tells the tests whether a recognized pattern is right and tells Hermes when
//! to ask: a pattern is certain when this list is empty.

use ncrs_index::{DocumentFields, Kinds};
use ncrs_intake::render_pattern_in;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    /// The path the file has.
    pub file: String,
    /// What the pattern writes for it, or `None` when it cannot write one.
    pub written: Option<String>,
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
    #![allow(clippy::indexing_slicing, clippy::shadow_unrelated)]

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
