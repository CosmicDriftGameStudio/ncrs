//! The order of a directory listing.

use std::cmp::Ordering;
use std::ffi::OsString;

pub use ncrs_core::{SortColumn, SortKey};

use super::FileEntry;

/// Latin letters with marks and the base letters they sort as. A letter that
/// is written precomposed (`ä`) and one written as `a` plus a combining mark
/// are the same here, so the same name sorts in one place whatever system
/// wrote it.
const BASE_LETTERS: &[(&str, &str)] = &[
    ("àáâãäåāăą", "a"),
    ("çćĉċč", "c"),
    ("ďđ", "d"),
    ("èéêëēĕėęě", "e"),
    ("ĝğġģ", "g"),
    ("ĥħ", "h"),
    ("ìíîïĩīĭįı", "i"),
    ("ĵ", "j"),
    ("ķ", "k"),
    ("ĺļľŀł", "l"),
    ("ñńņňŉ", "n"),
    ("òóôõöøōŏő", "o"),
    ("ŕŗř", "r"),
    ("śŝşš", "s"),
    ("ţťŧ", "t"),
    ("ùúûüũūŭůűų", "u"),
    ("ŵ", "w"),
    ("ýÿŷ", "y"),
    ("źżž", "z"),
    ("ß", "ss"),
    ("æ", "ae"),
    ("œ", "oe"),
];

fn is_combining_mark(c: char) -> bool {
    matches!(c, '\u{0300}'..='\u{036F}')
}

/// `text` in lower case with the marks taken off its Latin letters.
pub(crate) fn base_letters(text: &str) -> String {
    let mut folded = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        if is_combining_mark(c) {
            continue;
        }
        match BASE_LETTERS.iter().find(|(letters, _)| letters.contains(c)) {
            Some((_, base)) => folded.push_str(base),
            None => folded.push(c),
        }
    }
    folded
}

/// The key a name is ordered by: case and marks folded, and every run of
/// digits written as its length and then its digits, so that `9.txt` comes
/// before `10.txt`. Lossy conversion is only a display concern; for the
/// comparison it is stable for a given byte sequence.
fn fold_key(name: &OsString) -> String {
    let folded = base_letters(&name.to_string_lossy());
    let mut key = String::with_capacity(folded.len());
    let mut chars = folded.chars().peekable();
    while let Some(c) = chars.next() {
        if !c.is_ascii_digit() {
            key.push(c);
            continue;
        }
        let mut digits = String::from(c);
        while let Some(next) = chars.next_if(char::is_ascii_digit) {
            digits.push(next);
        }
        let significant = digits.trim_start_matches('0');
        key.push_str(&format!("{:0>4}{significant}", significant.len()));
    }
    key
}

/// Sorts `entries` by `key`. `..` stays first and directories stay before
/// files in either direction; the key orders within those groups. Directories
/// go by their computed size under `Size`, those without one after those with
/// one in either direction and by name; entries without a date come last under
/// `Modified`, and the name breaks every tie, then the full path (a list of search hits
/// holds equal names from different folders).
pub fn sort_entries(entries: &mut Vec<FileEntry>, key: SortKey) {
    // The folded name is computed once per entry, not once per comparison:
    // `to_lowercase` allocates, and a large directory compares O(n log n) times.
    let mut keyed: Vec<(String, FileEntry)> = std::mem::take(entries)
        .into_iter()
        .map(|entry| (fold_key(&entry.name), entry))
        .collect();
    keyed.sort_by(|(folded_a, a), (folded_b, b)| compare(a, folded_a, b, folded_b, key));
    entries.extend(keyed.into_iter().map(|(_, entry)| entry));
}

fn compare(a: &FileEntry, folded_a: &str, b: &FileEntry, folded_b: &str, key: SortKey) -> Ordering {
    let directed = |ordering: Ordering| {
        if key.descending {
            ordering.reverse()
        } else {
            ordering
        }
    };
    let by_name = || {
        folded_a
            .cmp(folded_b)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.path.cmp(&b.path))
    };
    b.is_parent
        .cmp(&a.is_parent)
        .then(b.is_dir.cmp(&a.is_dir))
        .then_with(|| match key.column {
            SortColumn::Name => directed(by_name()),
            SortColumn::Size if a.is_dir => match (a.folder_size, b.folder_size) {
                (Some(x), Some(y)) => directed(x.bytes.cmp(&y.bytes)).then_with(by_name),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => by_name(),
            },
            SortColumn::Size => directed(a.size.cmp(&b.size)).then_with(by_name),
            SortColumn::Modified => match (a.modified, b.modified) {
                (Some(x), Some(y)) => directed(x.cmp(&y)).then_with(by_name),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => by_name(),
            },
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};

    fn entry(name: &str, is_dir: bool, size: u64, age_secs: Option<u64>) -> FileEntry {
        FileEntry {
            name: name.into(),
            path: PathBuf::from(name),
            is_dir,
            is_symlink: false,
            is_parent: false,
            size,
            modified: age_secs.map(|secs| SystemTime::UNIX_EPOCH + Duration::from_secs(secs)),
            folder_size: None,
            details: Default::default(),
        }
    }

    fn computed_dir(name: &str, bytes: u64) -> FileEntry {
        FileEntry {
            folder_size: Some(crate::FolderSize {
                bytes,
                partial: false,
            }),
            ..entry(name, true, 0, None)
        }
    }

    fn dir_names_by_size(descending: bool) -> Vec<String> {
        let mut entries = vec![
            entry("a-unknown", true, 0, None),
            computed_dir("big", 900),
            entry("b-unknown", true, 0, None),
            computed_dir("small", 5),
            computed_dir("also-small", 5),
            entry("file.txt", false, 1, None),
        ];
        sort_entries(
            &mut entries,
            SortKey {
                column: SortColumn::Size,
                descending,
            },
        );
        entries
            .iter()
            .map(|e| e.name.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn computed_folders_sort_by_size_before_the_ones_without_a_value() {
        assert_eq!(
            dir_names_by_size(false),
            [
                "also-small",
                "small",
                "big",
                "a-unknown",
                "b-unknown",
                "file.txt"
            ]
        );
        assert_eq!(
            dir_names_by_size(true),
            [
                "big",
                "also-small",
                "small",
                "a-unknown",
                "b-unknown",
                "file.txt"
            ]
        );
    }

    fn listing() -> Vec<FileEntry> {
        vec![
            entry("b.txt", false, 30, Some(200)),
            entry("Zdir", true, 0, Some(100)),
            FileEntry::parent(Path::new("/")),
            entry("a.txt", false, 10, Some(300)),
            entry("adir", true, 0, Some(400)),
            entry("c.txt", false, 20, None),
        ]
    }

    fn sorted(column: SortColumn, descending: bool) -> Vec<String> {
        let mut entries = listing();
        sort_entries(&mut entries, SortKey { column, descending });
        entries
            .iter()
            .map(|e| e.name.to_string_lossy().into_owned())
            .collect()
    }

    fn names_in_order(names: &[&str]) -> Vec<String> {
        let mut entries: Vec<FileEntry> = names
            .iter()
            .map(|name| entry(name, false, 0, None))
            .collect();
        sort_entries(&mut entries, SortKey::default());
        entries
            .iter()
            .map(|e| e.name.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn numbers_are_ordered_by_value() {
        assert_eq!(
            names_in_order(&["10.txt", "9.txt", "100.txt", "a2", "a10"]),
            ["9.txt", "10.txt", "100.txt", "a2", "a10"]
        );
    }

    #[test]
    fn umlauts_sort_with_their_base_letter_in_either_spelling() {
        let precomposed = "\u{e4}rzte";
        let decomposed = "a\u{308}rzte";
        let order = names_in_order(&["zebra", "anna", precomposed, decomposed, "berta"]);
        assert_eq!(order.first().map(String::as_str), Some("anna"));
        assert_eq!(order.last().map(String::as_str), Some("zebra"));
        let pair: Vec<_> = order.iter().filter(|name| name.ends_with("rzte")).collect();
        assert_eq!(pair.len(), 2);
        assert_eq!(
            order.get(1).map(String::as_str),
            pair.first().map(|n| n.as_str())
        );
        assert_eq!(
            order.get(2).map(String::as_str),
            pair.get(1).map(|n| n.as_str())
        );
    }

    #[test]
    fn the_default_is_name_ascending_ignoring_case() {
        assert_eq!(SortKey::default().column, SortColumn::Name);
        assert!(!SortKey::default().descending);
        assert_eq!(
            sorted(SortColumn::Name, false),
            ["..", "adir", "Zdir", "a.txt", "b.txt", "c.txt"]
        );
    }

    #[test]
    fn name_descending_reverses_each_group_but_keeps_the_groups_in_place() {
        assert_eq!(
            sorted(SortColumn::Name, true),
            ["..", "Zdir", "adir", "c.txt", "b.txt", "a.txt"]
        );
    }

    #[test]
    fn size_orders_files_and_leaves_directories_by_name_in_both_directions() {
        assert_eq!(
            sorted(SortColumn::Size, false),
            ["..", "adir", "Zdir", "a.txt", "c.txt", "b.txt"]
        );
        assert_eq!(
            sorted(SortColumn::Size, true),
            ["..", "adir", "Zdir", "b.txt", "c.txt", "a.txt"]
        );
    }

    #[test]
    fn modified_puts_entries_without_a_date_last_in_both_directions() {
        assert_eq!(
            sorted(SortColumn::Modified, false),
            ["..", "Zdir", "adir", "b.txt", "a.txt", "c.txt"]
        );
        assert_eq!(
            sorted(SortColumn::Modified, true),
            ["..", "adir", "Zdir", "a.txt", "b.txt", "c.txt"]
        );
    }

    #[test]
    fn equal_values_fall_back_to_the_name() {
        let mut entries = vec![
            entry("b", false, 5, Some(1)),
            entry("A", false, 5, Some(1)),
            entry("a", false, 5, Some(1)),
        ];
        for column in [SortColumn::Size, SortColumn::Modified] {
            for descending in [false, true] {
                sort_entries(&mut entries, SortKey { column, descending });
                let names: Vec<_> = entries.iter().map(|e| e.name.to_string_lossy()).collect();
                assert_eq!(names, ["A", "a", "b"]);
            }
        }
    }

    #[test]
    fn requesting_the_same_column_again_flips_the_direction() {
        let key = SortKey::default();
        let size = key.requested(SortColumn::Size);
        assert_eq!((size.column, size.descending), (SortColumn::Size, false));
        let again = size.requested(SortColumn::Size);
        assert!(again.descending);
        assert!(!again.requested(SortColumn::Size).descending);
        assert!(!again.requested(SortColumn::Name).descending);
    }
}
