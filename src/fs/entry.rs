use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A single row in a directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
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
        let name = entry.file_name().to_string_lossy().into_owned();
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
            name: "..".to_string(),
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
    pub fn listing_order(a: &Self, b: &Self) -> Ordering {
        b.is_parent
            .cmp(&a.is_parent)
            .then(b.is_dir.cmp(&a.is_dir))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, is_dir: bool) -> FileEntry {
        FileEntry {
            name: name.to_string(),
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
