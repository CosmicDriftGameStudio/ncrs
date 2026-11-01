//! Total size of a folder, measured on request.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use ncrs_core::FolderSize;

/// Hard links are counted once per walk, symlinks are never followed (a link
/// counts its own size). `None` when `cancel` was set. Blocking.
pub fn measure_folder(root: &Path, cancel: &AtomicBool) -> Option<FolderSize> {
    let mut bytes: u64 = 0;
    let mut partial = false;
    let mut seen = SeenFiles::default();
    let mut pending: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(folder) = pending.pop() {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let Ok(children) = std::fs::read_dir(&folder) else {
            partial = true;
            continue;
        };
        for listed in children {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let Ok(child) = listed else {
                partial = true;
                continue;
            };
            let path = child.path();
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                partial = true;
                continue;
            };
            if metadata.is_dir() {
                pending.push(path);
            } else if seen.first_time(&metadata) {
                bytes = bytes.saturating_add(metadata.len());
            }
        }
    }
    Some(FolderSize { bytes, partial })
}

#[derive(Default)]
struct SeenFiles {
    #[cfg(unix)]
    linked: std::collections::HashSet<(u64, u64)>,
}

impl SeenFiles {
    #[cfg(unix)]
    fn first_time(&mut self, metadata: &std::fs::Metadata) -> bool {
        use std::os::unix::fs::MetadataExt as _;
        metadata.nlink() <= 1 || self.linked.insert((metadata.dev(), metadata.ino()))
    }

    // std has no stable file id on other systems, so links count each time.
    #[cfg(not(unix))]
    fn first_time(&mut self, _metadata: &std::fs::Metadata) -> bool {
        true
    }
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

    fn write(path: &Path, len: usize) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![b'x'; len]).unwrap();
    }

    fn running() -> AtomicBool {
        AtomicBool::new(false)
    }

    #[test]
    fn nested_files_are_summed() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("tree/a.txt"), 10);
        write(&root.path().join("tree/deep/er/b.txt"), 25);

        let found = measure_folder(&root.path().join("tree"), &running());

        assert_eq!(
            found,
            Some(FolderSize {
                bytes: 35,
                partial: false
            })
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_counts_only_itself() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("big/huge.bin"), 100_000);
        write(&root.path().join("tree/small.txt"), 1);
        let link = root.path().join("tree/to-big");
        std::os::unix::fs::symlink(root.path().join("big"), &link).unwrap();
        let link_size = std::fs::symlink_metadata(&link).unwrap().len();

        let found = measure_folder(&root.path().join("tree"), &running()).unwrap();

        assert_eq!(found.bytes, 1 + link_size);
        assert!(!found.partial);
    }

    #[cfg(unix)]
    #[test]
    fn a_hard_link_pair_is_counted_once() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("tree/one.bin"), 500);
        std::fs::hard_link(
            root.path().join("tree/one.bin"),
            root.path().join("tree/two.bin"),
        )
        .unwrap();

        let found = measure_folder(&root.path().join("tree"), &running()).unwrap();

        assert_eq!(found.bytes, 500);
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_subfolder_makes_the_value_partial() {
        use std::os::unix::fs::PermissionsExt as _;
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("tree/open.txt"), 7);
        write(&root.path().join("tree/sealed/inside.txt"), 3);
        let sealed = root.path().join("tree/sealed");
        std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o000)).unwrap();
        // Whoever may read it anyway (root) cannot show the case.
        let can_still_read = std::fs::read_dir(&sealed).is_ok();

        let found = measure_folder(&root.path().join("tree"), &running());
        std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o755)).unwrap();

        if !can_still_read {
            assert_eq!(
                found,
                Some(FolderSize {
                    bytes: 7,
                    partial: true
                })
            );
        }
    }

    #[test]
    fn a_vanished_root_is_partial_and_empty() {
        let root = tempfile::tempdir().unwrap();

        let found = measure_folder(&root.path().join("gone"), &running());

        assert_eq!(
            found,
            Some(FolderSize {
                bytes: 0,
                partial: true
            })
        );
    }

    #[test]
    fn a_stop_flag_set_beforehand_gives_nothing() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("tree/a.txt"), 10);

        let found = measure_folder(&root.path().join("tree"), &AtomicBool::new(true));

        assert_eq!(found, None);
    }
}
