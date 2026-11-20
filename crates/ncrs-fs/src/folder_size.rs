//! Total size of a folder, measured on request.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use ncrs_core::FolderSize;

/// Hard links are counted once per walk, symlinks are never followed (a link
/// counts its own size). Folders on another device (mounts, `/proc`) are
/// skipped like `du -x` and make the result partial. `None` when `cancel` was
/// set. Blocking.
pub fn measure_folder(root: &Path, cancel: &AtomicBool) -> Option<FolderSize> {
    measure_folder_on(root, cancel, device_of)
}

/// How many folders below the root a walk keeps a size for. A walk over a
/// whole disk would otherwise hold one path per folder of it.
const KEPT_FOLDERS: usize = 20_000;

/// The size of a folder and of the folders the walk went through below it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeasuredTree {
    pub total: FolderSize,
    /// Every folder below the root the walk kept, parents before their
    /// children; a folder's size includes its subfolders.
    pub below: Vec<(PathBuf, FolderSize)>,
}

/// [`measure_folder`] that also keeps the size of the folders it walked
/// through, so entering one shows its size without another walk. A hard link
/// is counted in the folder the walk met it in first.
pub fn measure_folder_tree(root: &Path, cancel: &AtomicBool) -> Option<MeasuredTree> {
    measure_tree_on(root, cancel, device_of)
}

#[cfg(unix)]
fn device_of(_path: &Path, metadata: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt as _;
    Some(metadata.dev())
}

#[cfg(not(unix))]
fn device_of(_path: &Path, _metadata: &std::fs::Metadata) -> Option<u64> {
    None
}

fn measure_folder_on(
    root: &Path,
    cancel: &AtomicBool,
    device_of: impl Fn(&Path, &std::fs::Metadata) -> Option<u64>,
) -> Option<FolderSize> {
    measure_tree_on(root, cancel, device_of).map(|tree| tree.total)
}

fn measure_tree_on(
    root: &Path,
    cancel: &AtomicBool,
    device_of: impl Fn(&Path, &std::fs::Metadata) -> Option<u64>,
) -> Option<MeasuredTree> {
    let root_device = std::fs::symlink_metadata(root)
        .ok()
        .and_then(|metadata| device_of(root, &metadata));
    let mut seen = SeenFiles::default();
    let mut counted = vec![Counted {
        path: root.to_path_buf(),
        parent: None,
        bytes: 0,
        partial: false,
    }];
    // Each folder still to read, with the entry of `counted` its bytes go to:
    // its own, or its nearest ancestor's once the walk stopped keeping them.
    let mut pending: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((folder, owner)) = pending.pop() {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let Ok(children) = std::fs::read_dir(&folder) else {
            counted[owner].partial = true;
            continue;
        };
        for listed in children {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let Ok(child) = listed else {
                counted[owner].partial = true;
                continue;
            };
            let path = child.path();
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                counted[owner].partial = true;
                continue;
            };
            if metadata.is_dir() {
                if root_device.is_some() && device_of(&path, &metadata) != root_device {
                    counted[owner].partial = true;
                } else if counted.len() <= KEPT_FOLDERS {
                    counted.push(Counted {
                        path: path.clone(),
                        parent: Some(owner),
                        bytes: 0,
                        partial: false,
                    });
                    pending.push((path, counted.len() - 1));
                } else {
                    pending.push((path, owner));
                }
            } else if seen.first_time(&metadata) {
                counted[owner].bytes = counted[owner].bytes.saturating_add(metadata.len());
            }
        }
    }
    // A folder is pushed after its parent, so walking backwards adds every
    // folder to its parent once its own subfolders are in.
    for index in (1..counted.len()).rev() {
        if let Some(parent) = counted[index].parent {
            let (bytes, partial) = (counted[index].bytes, counted[index].partial);
            counted[parent].bytes = counted[parent].bytes.saturating_add(bytes);
            counted[parent].partial |= partial;
        }
    }
    let size_of = |entry: &Counted| FolderSize {
        bytes: entry.bytes,
        partial: entry.partial,
    };
    let total = size_of(&counted[0]);
    let below = counted
        .iter()
        .skip(1)
        .map(|entry| (entry.path.clone(), size_of(entry)))
        .collect();
    Some(MeasuredTree { total, below })
}

struct Counted {
    path: PathBuf,
    parent: Option<usize>,
    bytes: u64,
    partial: bool,
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

    #[test]
    fn every_folder_walked_through_keeps_its_own_size() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("tree/a.txt"), 10);
        write(&root.path().join("tree/one/b.txt"), 20);
        write(&root.path().join("tree/one/two/c.txt"), 5);

        let tree = measure_folder_tree(&root.path().join("tree"), &running()).unwrap();

        let size = |bytes| FolderSize {
            bytes,
            partial: false,
        };
        assert_eq!(tree.total, size(35));
        assert_eq!(
            tree.below,
            vec![
                (root.path().join("tree/one"), size(25)),
                (root.path().join("tree/one/two"), size(5)),
            ]
        );
    }

    #[test]
    fn a_folder_on_another_device_is_skipped_and_makes_the_total_partial() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("tree/a.txt"), 10);
        write(&root.path().join("tree/mount/big.bin"), 1000);
        let other_device =
            |path: &Path, _: &std::fs::Metadata| Some(if path.ends_with("mount") { 2 } else { 1 });

        let found = measure_folder_on(&root.path().join("tree"), &running(), other_device);

        assert_eq!(
            found,
            Some(FolderSize {
                bytes: 10,
                partial: true
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
