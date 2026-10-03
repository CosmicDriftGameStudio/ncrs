//! The order of a directory listing.

use std::cmp::Ordering;
use std::ffi::OsString;

use super::FileEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortColumn {
    #[default]
    Name,
    Size,
    Modified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SortKey {
    pub column: SortColumn,
    pub descending: bool,
}

impl SortKey {
    /// The key after asking for `column`: the same column again reverses the
    /// direction, another column starts ascending.
    pub fn requested(self, column: SortColumn) -> Self {
        Self {
            column,
            descending: self.column == column && !self.descending,
        }
    }
}

/// The case fold is what makes `B.txt` sort next to `b.txt`. Lossy conversion
/// is only a display concern; for the comparison it is stable for a given byte
/// sequence.
fn fold_key(name: &OsString) -> String {
    name.to_string_lossy().to_lowercase()
}

/// Sorts `entries` by `key`. `..` stays first and directories stay before
/// files in either direction; the key orders within those groups. Directories
/// have no size and go by name under `Size`, entries without a date come last
/// under `Modified`, and the name breaks every tie.
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
    let by_name = || folded_a.cmp(folded_b).then_with(|| a.name.cmp(&b.name));
    b.is_parent
        .cmp(&a.is_parent)
        .then(b.is_dir.cmp(&a.is_dir))
        .then_with(|| match key.column {
            SortColumn::Name => directed(by_name()),
            SortColumn::Size if a.is_dir => by_name(),
            SortColumn::Size => directed(a.size.cmp(&b.size)).then_with(by_name),
            SortColumn::Modified => match (a.modified, b.modified) {
                (Some(x), Some(y)) => directed(x.cmp(&y)).then_with(by_name),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => by_name(),
            },
        })
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
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
        }
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
