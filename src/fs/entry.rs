use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A single row in a directory listing.
/// Lowercase form used for the case-insensitive comparison.
///
/// `OsString::to_string_lossy` is only a display concern; the comparison needs
/// a stable ordering, and lossy conversion is stable for a given byte
/// sequence. The case fold is what makes `B.txt` sort next to `b.txt`.
fn fold_key(name: &std::ffi::OsString) -> String {
    name.to_string_lossy().to_lowercase()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// The name as the filesystem spells it. `OsString`, not `String`: a name
    /// that is not valid UTF-8 would become U+FFFD here, and two different
    /// files could then share one name — which matters once a tag is a name
    /// rather than a position.
    pub name: std::ffi::OsString,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
    /// `true` for the synthetic `..` entry.
    pub is_parent: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

impl FileEntry {
    /// Builds an entry from a `read_dir` item. Never fails: missing metadata
    /// simply results in zero size / unknown date.
    pub fn from_dir_entry(entry: &std::fs::DirEntry) -> Self {
        let path = entry.path();
        let name = entry.file_name();
        let is_symlink = entry.file_type().map(|t| t.is_symlink()).unwrap_or(false);
        // Follow symlinks so that links to directories are navigable.
        let metadata = std::fs::metadata(&path).or_else(|_| entry.metadata()).ok();

        let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let size = metadata
            .as_ref()
            .filter(|m| !m.is_dir())
            .map(|m| m.len())
            .unwrap_or(0);
        let modified = metadata.and_then(|m| m.modified().ok());

        Self {
            name,
            path,
            is_dir,
            is_symlink,
            is_parent: false,
            size,
            modified,
        }
    }

    /// The synthetic `..` entry pointing at `parent`.
    pub fn parent(parent: &Path) -> Self {
        Self {
            name: std::ffi::OsString::from(".."),
            path: parent.to_path_buf(),
            is_dir: true,
            is_symlink: false,
            is_parent: true,
            size: 0,
            modified: None,
        }
    }

    /// Sort order: `..` first, then directories, then files; names
    /// case-insensitively.
    ///
    /// No longer used to sort — `sort_key` is, because it computes the
    /// lowercase name once per entry instead of once per comparison. Kept as the
    /// readable statement of the rule, and because the test that pins `sort_key`
    /// to it would otherwise have nothing to compare against.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn listing_order(a: &Self, b: &Self) -> Ordering {
        b.is_parent
            .cmp(&a.is_parent)
            .then(b.is_dir.cmp(&a.is_dir))
            .then_with(|| fold_key(&a.name).cmp(&fold_key(&b.name)))
            .then_with(|| a.name.cmp(&b.name))
    }

    /// The key `listing_order` sorts by. Computed once per entry, not once per
    /// comparison: `to_lowercase` allocates, and doing that O(n log n) times is
    /// the whole cost of sorting a large directory.
    ///
    /// The flags are inverted, matching `listing_order`: that one compares
    /// `b.flag.cmp(&a.flag)` so `true` sorts first, and a key has to do the
    /// same. Sorting ascending on the raw flag would put `..` last.
    pub fn sort_key(&self) -> (bool, bool, String) {
        (!self.is_parent, !self.is_dir, fold_key(&self.name))
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
// reason: a test module belongs next to what it tests
#[allow(clippy::inline_modules)]
#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, is_dir: bool) -> FileEntry {
        FileEntry {
            name: name.into(),
            path: PathBuf::from(name),
            is_dir,
            is_symlink: false,
            is_parent: false,
            size: 0,
            modified: None,
        }
    }

    #[test]
    fn parent_entry_sorts_first() {
        let parent = FileEntry::parent(Path::new("/"));
        let file = entry("a.txt", false);
        let dir = entry("zdir", true);

        assert_eq!(FileEntry::listing_order(&parent, &file), Ordering::Less);
        assert_eq!(FileEntry::listing_order(&parent, &dir), Ordering::Less);
    }

    #[test]
    fn directories_sort_before_files() {
        let dir = entry("zzz", true);
        let file = entry("aaa.txt", false);

        assert_eq!(FileEntry::listing_order(&dir, &file), Ordering::Less);
    }

    #[test]
    fn names_compare_case_insensitively() {
        let upper = entry("B.txt", false);
        let lower = entry("a.txt", false);

        assert_eq!(FileEntry::listing_order(&lower, &upper), Ordering::Less);
    }

    #[test]
    fn equal_names_fall_back_to_byte_order() {
        let a = entry("A.txt", false);
        let b = entry("a.txt", false);

        assert_eq!(FileEntry::listing_order(&a, &b), Ordering::Less);
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
// reason: a test module belongs next to what it tests
#[allow(clippy::inline_modules)]
#[cfg(test)]
mod sort_key_tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::PathBuf;

    fn entry(name: &str, is_dir: bool, is_parent: bool) -> FileEntry {
        FileEntry {
            name: OsString::from(name),
            path: PathBuf::from(name),
            is_dir,
            is_symlink: false,
            is_parent,
            size: 0,
            modified: None,
        }
    }

    /// `sort_key` exists to replace `listing_order` in `sort_by_cached_key`, and
    /// the two have to agree. They did not: `listing_order` compares the
    /// flags inverted, the key did not, and `..` sorted to the bottom. The
    /// existing tests only exercised `listing_order`, so nothing noticed.
    #[test]
    fn the_key_sorts_like_the_comparator() {
        let entries = vec![
            entry("zeta.txt", false, false),
            entry("Alpha", true, false),
            entry("..", true, true),
            entry("beta.txt", false, false),
            entry("Beta", true, false),
        ];

        let mut by_key = entries.clone();
        by_key.sort_by_cached_key(|e| e.sort_key());

        let mut by_order = entries;
        by_order.sort_by(FileEntry::listing_order);

        let names = |v: &[FileEntry]| -> Vec<String> {
            v.iter()
                .map(|e| e.name.to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(names(&by_key), names(&by_order));
    }

    /// And the order itself, so "they agree" cannot be true of two wrong orders.
    #[test]
    fn the_key_puts_parent_first_then_dirs_then_names() {
        let entries = [
            entry("zeta.txt", false, false),
            entry("alpha.txt", false, false),
            entry("Beta", true, false),
            entry("..", true, true),
            entry("alpha", true, false),
        ];
        let mut sorted = entries.to_vec();
        sorted.sort_by_cached_key(|e| e.sort_key());

        let names: Vec<String> = sorted
            .iter()
            .map(|e| e.name.to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["..", "alpha", "Beta", "alpha.txt", "zeta.txt"]);
    }
}
