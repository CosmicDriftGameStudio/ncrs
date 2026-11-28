//! Total size of a folder, measured on request.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

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
    measure_tree_on(
        root,
        cancel,
        device_of,
        |_| {},
        Duration::MAX,
        Keep::default(),
    )
}

/// How often a running walk reports what it has counted so far.
pub const RUNNING_REPORT_INTERVAL: Duration = Duration::from_millis(100);

/// [`measure_folder_tree`] that also calls `on_running` with the bytes counted
/// so far, at most every `report_every`. The bytes only grow and the last
/// value is at most the final total (hard links and skipped folders are not
/// in it).
pub fn measure_folder_tree_reporting(
    root: &Path,
    cancel: &AtomicBool,
    report_every: Duration,
    on_running: impl FnMut(u64),
) -> Option<MeasuredTree> {
    measure_tree_on(
        root,
        cancel,
        device_of,
        on_running,
        report_every,
        Keep::default(),
    )
}

/// [`measure_folder_tree_reporting`] that keeps a size only for the folders down to `max_depth`
/// below the root (1 = its direct subfolders), without the cap on their number. Deeper
/// bytes roll up into the folder at that depth.
pub fn measure_folder_depth(
    root: &Path,
    cancel: &AtomicBool,
    max_depth: usize,
    report_every: Duration,
    on_running: impl FnMut(u64),
) -> Option<MeasuredTree> {
    let keep = Keep {
        max_folders: usize::MAX,
        max_depth,
    };
    measure_tree_on(root, cancel, device_of, on_running, report_every, keep)
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
    measure_tree_on(
        root,
        cancel,
        device_of,
        |_| {},
        Duration::MAX,
        Keep::default(),
    )
    .map(|tree| tree.total)
}

fn measure_tree_on(
    root: &Path,
    cancel: &AtomicBool,
    device_of: impl Fn(&Path, &std::fs::Metadata) -> Option<u64>,
    mut on_running: impl FnMut(u64),
    report_every: Duration,
    keep: Keep,
) -> Option<MeasuredTree> {
    let mut running: u64 = 0;
    let mut last_report = Instant::now();
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
    let mut pending: Vec<(PathBuf, usize, usize)> = vec![(root.to_path_buf(), 0, 0)];
    while let Some((folder, owner, depth)) = pending.pop() {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let Ok(children) = std::fs::read_dir(&folder) else {
            mark_partial(&mut counted, owner);
            continue;
        };
        for listed in children {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let Ok(child) = listed else {
                mark_partial(&mut counted, owner);
                continue;
            };
            let path = child.path();
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                mark_partial(&mut counted, owner);
                continue;
            };
            if metadata.is_dir() {
                if root_device.is_some() && device_of(&path, &metadata) != root_device {
                    mark_partial(&mut counted, owner);
                } else if counted.len() <= keep.max_folders && depth < keep.max_depth {
                    counted.push(Counted {
                        path: path.clone(),
                        parent: Some(owner),
                        bytes: 0,
                        partial: false,
                    });
                    pending.push((path, counted.len() - 1, depth + 1));
                } else {
                    pending.push((path, owner, depth + 1));
                }
            } else if seen.first_time(&metadata) {
                add_bytes(&mut counted, owner, metadata.len());
                running = running.saturating_add(metadata.len());
                if last_report.elapsed() >= report_every {
                    on_running(running);
                    last_report = Instant::now();
                }
            }
        }
    }
    // A folder is pushed after its parent, so walking backwards adds every
    // folder to its parent once its own subfolders are in.
    for index in (1..counted.len()).rev() {
        let Some((parent, bytes, partial)) = counted
            .get(index)
            .and_then(|entry| Some((entry.parent?, entry.bytes, entry.partial)))
        else {
            continue;
        };
        add_bytes(&mut counted, parent, bytes);
        if partial {
            mark_partial(&mut counted, parent);
        }
    }
    let size_of = |entry: &Counted| FolderSize {
        bytes: entry.bytes,
        partial: entry.partial,
    };
    let total = size_of(counted.first()?);
    let below = counted
        .iter()
        .skip(1)
        .map(|entry| (entry.path.clone(), size_of(entry)))
        .collect();
    Some(MeasuredTree { total, below })
}

fn mark_partial(counted: &mut [Counted], index: usize) {
    if let Some(entry) = counted.get_mut(index) {
        entry.partial = true;
    }
}

fn add_bytes(counted: &mut [Counted], index: usize, bytes: u64) {
    if let Some(entry) = counted.get_mut(index) {
        entry.bytes = entry.bytes.saturating_add(bytes);
    }
}

/// What a walk keeps a size for.
struct Keep {
    max_folders: usize,
    max_depth: usize,
}

impl Default for Keep {
    fn default() -> Self {
        Self {
            max_folders: KEPT_FOLDERS,
            max_depth: usize::MAX,
        }
    }
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
    fn a_depth_walk_keeps_folders_down_to_the_depth_and_counts_the_rest_in() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("tree/one/a.txt"), 10);
        write(&root.path().join("tree/one/two/b.txt"), 20);
        write(&root.path().join("tree/one/two/three/c.txt"), 5);

        let tree = measure_folder_depth(
            &root.path().join("tree"),
            &running(),
            2,
            Duration::MAX,
            |_| {},
        )
        .unwrap();

        let kept: Vec<_> = tree
            .below
            .iter()
            .map(|(path, size)| {
                (
                    path.strip_prefix(root.path()).unwrap().to_owned(),
                    size.bytes,
                )
            })
            .collect();
        assert_eq!(
            kept,
            [
                (PathBuf::from("tree/one"), 35),
                (PathBuf::from("tree/one/two"), 25)
            ]
        );
        assert_eq!(tree.total.bytes, 35);
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
    fn running_totals_arrive_before_the_result_and_only_grow() {
        let root = tempfile::tempdir().unwrap();
        for n in 0..6 {
            write(&root.path().join(format!("tree/sub/file-{n}.txt")), 10 + n);
        }
        write(&root.path().join("tree/top.txt"), 7);
        let mut reported = Vec::new();

        let tree = measure_tree_on(
            &root.path().join("tree"),
            &running(),
            device_of,
            |bytes| reported.push(bytes),
            Duration::ZERO,
            Keep::default(),
        )
        .unwrap();

        assert_eq!(reported.len(), 7, "one report per counted file");
        assert!(reported.is_sorted());
        assert!(reported.iter().all(|bytes| *bytes <= tree.total.bytes));
        assert_eq!(reported.last().copied(), Some(tree.total.bytes));
    }

    #[test]
    fn a_stop_flag_set_beforehand_gives_nothing() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("tree/a.txt"), 10);

        let found = measure_folder(&root.path().join("tree"), &AtomicBool::new(true));

        assert_eq!(found, None);
    }
}
