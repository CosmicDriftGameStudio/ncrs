//! Tagging and untagging by a pattern of file names: `*` for any run of
//! characters, `?` for one, several patterns separated by `;`, upper and lower
//! case alike.

use std::ffi::OsStr;

use ncrs_core::{FileEntry, SelectionSet};

use crate::name_match::{type_ahead_key, typed_key};

/// The patterns of `typed`, ready to compare with a lower-cased composed name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamePattern {
    patterns: Vec<Vec<char>>,
}

impl NamePattern {
    /// `None` when `typed` holds no pattern at all.
    pub fn parse(typed: &str) -> Option<Self> {
        let patterns: Vec<Vec<char>> = typed
            .split(';')
            .map(str::trim)
            .filter(|each| !each.is_empty())
            .map(|each| typed_key(each).chars().collect())
            .collect();
        (!patterns.is_empty()).then_some(Self { patterns })
    }

    pub fn matches(&self, name: &OsStr) -> bool {
        let letters: Vec<char> = type_ahead_key(name).chars().collect();
        self.patterns.iter().any(|pattern| {
            // A name without a dot is a name of `*.*` too, as in Total Commander.
            pattern == &['*', '.', '*'] || glob_matches(pattern, &letters)
        })
    }
}

fn glob_matches(pattern: &[char], name: &[char]) -> bool {
    let (mut p, mut n) = (0, 0);
    // Where the last `*` was and how much of the name it has taken so far.
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some((p, n));
                p += 1;
            }
            Some('?') => {
                p += 1;
                n += 1;
            }
            Some(&literal) if name.get(n) == Some(&literal) => {
                p += 1;
                n += 1;
            }
            _ => match star {
                Some((star_at, taken)) => {
                    p = star_at + 1;
                    n = taken + 1;
                    star = Some((star_at, taken + 1));
                }
                None => return false,
            },
        }
    }
    pattern.iter().skip(p).all(|&rest| rest == '*')
}

/// Tags (or, with `untag`, untags) the entries of `entries` whose name matches
/// `typed`. `..` is never tagged. Returns how many entries matched.
pub fn tag_matching(
    selection: &mut SelectionSet,
    entries: &[FileEntry],
    typed: &str,
    untag: bool,
) -> usize {
    let Some(pattern) = NamePattern::parse(typed) else {
        return 0;
    };
    let mut matched = 0;
    for entry in entries
        .iter()
        .filter(|entry| !entry.is_parent && pattern.matches(&entry.name))
    {
        matched += 1;
        if selection.is_tagged(&entry.path) == untag {
            selection.toggle(&entry.path);
        }
    }
    matched
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn matches(typed: &str, name: &str) -> bool {
        NamePattern::parse(typed).is_some_and(|pattern| pattern.matches(OsStr::new(name)))
    }

    fn entry(name: &str, is_parent: bool) -> FileEntry {
        FileEntry {
            name: name.into(),
            path: PathBuf::from("/d").join(name),
            is_dir: false,
            is_symlink: false,
            is_parent,
            size: 0,
            modified: None,
            folder_size: None,
            details: Default::default(),
        }
    }

    #[test]
    fn star_and_question_mark_match_runs_and_single_characters() {
        assert!(matches("*.pdf", "report.pdf"));
        assert!(!matches("*.pdf", "report.pdf.bak"));
        assert!(matches("rep*", "report.pdf"));
        assert!(matches("r?port.pdf", "report.pdf"));
        assert!(!matches("r?port.pdf", "rport.pdf"));
        assert!(matches("*a*b*", "xxaxxbxx"));
        assert!(!matches("*a*b*", "xxbxxaxx"));
        assert!(matches("*", ""));
    }

    #[test]
    fn case_does_not_matter_and_composed_forms_are_the_same() {
        assert!(matches("*.PDF", "report.pdf"));
        assert!(matches("\u{dc}bung*", "U\u{308}bung.txt"));
    }

    #[test]
    fn star_dot_star_takes_a_name_without_a_dot() {
        assert!(matches("*.*", "Makefile"));
        assert!(matches("*.*", "a.b"));
    }

    #[test]
    fn several_patterns_are_separated_by_semicolons() {
        assert!(matches("*.jpg; *.png", "a.png"));
        assert!(matches("*.jpg;*.png;", "a.jpg"));
        assert!(!matches("*.jpg;*.png", "a.gif"));
        assert_eq!(NamePattern::parse(" ; "), None);
    }

    #[test]
    fn tagging_skips_the_parent_and_untagging_only_removes_the_matches() {
        let entries = [
            entry("..", true),
            entry("a.pdf", false),
            entry("b.pdf", false),
            entry("c.txt", false),
        ];
        let mut selection = SelectionSet::new();
        assert_eq!(tag_matching(&mut selection, &entries, "*", false), 3);
        assert!(!selection.is_tagged(&entries[0].path));
        assert_eq!(selection.len(), 3);
        assert_eq!(tag_matching(&mut selection, &entries, "*.pdf", true), 2);
        assert!(selection.is_tagged(&entries[3].path));
        assert_eq!(selection.len(), 1);
        // Tagging again what is tagged does not untag it.
        assert_eq!(tag_matching(&mut selection, &entries, "c.*", false), 1);
        assert!(selection.is_tagged(&entries[3].path));
    }
}
