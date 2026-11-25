//! Whether a target can be filed into. A target folder is never made: a folder
//! that is not there may be a drive that is not plugged in.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ncrs_intake::{SortSource, SortTarget};

use super::access::{text, SortingAccess};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetState {
    Ready,
    Waiting(WaitReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitReason {
    /// The folder is not there, or no such target is configured.
    Missing,
    /// The volume the target was on is not there.
    NotMounted,
    /// The folder is there but cannot be read, for example without the grant
    /// of the store build.
    NotAccessible,
    /// Taken over from an older version, and none of the documents known below
    /// it is there.
    Unconfirmed,
    Paused,
}

pub(super) fn state_of(access: &SortingAccess, target: &SortTarget) -> TargetState {
    let system = system_folders();
    state_with(target, device_of, &system, || holds_known_file(access, &target.path))
}

/// Whether `path` is on a volume of its own, as the `own_volume` of a source or
/// target remembers it; `None` when that cannot be told.
pub fn is_on_own_volume(path: &Path) -> Option<bool> {
    differs_from_system(path, &device_of, &system_folders())
}

fn state_with(
    target: &SortTarget,
    device_of: impl Fn(&Path) -> Option<String>,
    system: &[PathBuf],
    holds_known_file: impl FnOnce() -> bool,
) -> TargetState {
    let on_own_volume = target.own_volume == Some(true);
    if let Err(err) = fs::read_dir(&target.path) {
        return TargetState::Waiting(match err.kind() {
            io::ErrorKind::NotFound if on_own_volume => WaitReason::NotMounted,
            io::ErrorKind::NotFound | io::ErrorKind::NotADirectory => WaitReason::Missing,
            _ => WaitReason::NotAccessible,
        });
    }
    if on_own_volume && !is_mounted(&target.path, &device_of, system) {
        return TargetState::Waiting(WaitReason::NotMounted);
    }
    if target.own_volume.is_none() && !holds_known_file() {
        return TargetState::Waiting(WaitReason::Unconfirmed);
    }
    TargetState::Ready
}

/// Where systems mount drives.
const MOUNT_FOLDERS: [&str; 4] = ["/Volumes", "/media", "/run/media", "/mnt"];

/// Whether the drive of a source is there; without it an empty folder may be
/// left where it was mounted. `root` is the folder as its files are spelled.
pub(super) fn source_is_mounted(source: &SortSource, root: &Path) -> bool {
    source_is_mounted_with(source, root, device_of, &system_folders())
}

/// A source that was never checked is not held to a known document as a target
/// is: an ordinary one often has none.
fn source_is_mounted_with(
    source: &SortSource,
    root: &Path,
    device_of: impl Fn(&Path) -> Option<String>,
    system: &[PathBuf],
) -> bool {
    let looks_mounted = MOUNT_FOLDERS
        .iter()
        .any(|folder| root != Path::new(folder) && root.starts_with(folder));
    !source.own_volume.unwrap_or(looks_mounted) || is_mounted(root, &device_of, system)
}

fn is_mounted(
    path: &Path,
    device_of: &impl Fn(&Path) -> Option<String>,
    system: &[PathBuf],
) -> bool {
    differs_from_system(path, device_of, system) == Some(true)
}

fn differs_from_system(
    path: &Path,
    device_of: &impl Fn(&Path) -> Option<String>,
    system: &[PathBuf],
) -> Option<bool> {
    let device = device_of(path)?;
    let system_devices: Vec<String> = system.iter().filter_map(|folder| device_of(folder)).collect();
    (!system_devices.is_empty()).then(|| !system_devices.contains(&device))
}

fn holds_known_file(access: &SortingAccess, folder: &Path) -> bool {
    let folder = ncrs_intake::canonical_path(folder).unwrap_or_else(|_| folder.to_path_buf());
    let known = access.with_index(|index| index.documents_under(&folder).map_err(text));
    known.is_ok_and(|documents| documents.iter().any(|(_, path)| path.is_file()))
}

/// The folders that are on the system disk whatever is plugged in. The home
/// folder counts: on a partition of its own it is still always there, and an
/// empty mount point inside it must not pass for the mounted drive.
fn system_folders() -> Vec<PathBuf> {
    let mut folders = Vec::new();
    #[cfg(unix)]
    folders.push(PathBuf::from("/"));
    #[cfg(target_os = "macos")]
    folders.push(PathBuf::from("/System/Volumes/Data"));
    #[cfg(unix)]
    folders.extend(std::env::var_os("HOME").map(PathBuf::from));
    #[cfg(windows)]
    folders.extend(std::env::var_os("SystemDrive").map(|drive| {
        let mut root = drive;
        root.push("\\");
        PathBuf::from(root)
    }));
    folders
}

#[cfg(unix)]
fn device_of(path: &Path) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).ok().map(|found| found.dev().to_string())
}

/// The drive or share of the path. A volume mounted into a folder of the
/// system drive counts as the system drive, so a target on one waits.
#[cfg(windows)]
fn device_of(path: &Path) -> Option<String> {
    let resolved = ncrs_intake::canonical_path(path).ok()?;
    match resolved.components().next()? {
        std::path::Component::Prefix(prefix) => {
            Some(prefix.as_os_str().to_string_lossy().to_ascii_uppercase())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ncrs_index::TargetId;
    use ncrs_intake::{LocationTransfer, OnNoText, OnSeveralTargets};

    fn target(path: &Path, own_volume: Option<bool>) -> SortTarget {
        SortTarget {
            id: TargetId::new("archive").unwrap(),
            name: "Archive".to_owned(),
            path: path.to_path_buf(),
            own_volume,
        }
    }

    /// The target folder is on `device`, everything else on the system disk.
    fn state(target: &SortTarget, device: Option<&str>, known: bool) -> TargetState {
        let own = target.path.clone();
        state_with(
            target,
            |path| {
                if path == own {
                    device.map(str::to_owned)
                } else {
                    Some("system".to_owned())
                }
            },
            &[PathBuf::from("/")],
            || known,
        )
    }

    #[test]
    fn a_target_on_a_drive_waits_while_the_drive_is_not_there() {
        let dir = tempfile::tempdir().unwrap();
        let on_drive = target(dir.path(), Some(true));
        assert_eq!(state(&on_drive, Some("usb"), false), TargetState::Ready);
        assert_eq!(
            state(&on_drive, Some("system"), true),
            TargetState::Waiting(WaitReason::NotMounted)
        );
        assert_eq!(
            state(&on_drive, None, true),
            TargetState::Waiting(WaitReason::NotMounted)
        );
        let gone = target(&dir.path().join("gone"), Some(true));
        assert_eq!(
            state(&gone, Some("usb"), true),
            TargetState::Waiting(WaitReason::NotMounted)
        );
    }

    #[test]
    fn a_target_on_the_system_disk_only_has_to_be_there() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            state(&target(dir.path(), Some(false)), Some("system"), false),
            TargetState::Ready
        );
        let gone = dir.path().join("gone");
        assert_eq!(
            state(&target(&gone, Some(false)), Some("system"), false),
            TargetState::Waiting(WaitReason::Missing)
        );
        assert!(!gone.exists());
    }

    #[test]
    fn a_target_that_was_taken_over_waits_until_a_known_document_is_seen() {
        let dir = tempfile::tempdir().unwrap();
        let adopted = target(dir.path(), None);
        assert_eq!(
            state(&adopted, Some("usb"), false),
            TargetState::Waiting(WaitReason::Unconfirmed)
        );
        assert_eq!(state(&adopted, Some("usb"), true), TargetState::Ready);
    }

    #[test]
    fn a_source_waits_for_the_drive_it_was_saved_on_or_looks_to_be_on() {
        let is_mounted = |root: &str, own_volume, device: Option<&str>| {
            let source = SortSource {
                path: PathBuf::from(root),
                include: String::new(),
                exclude: String::new(),
                targets: Vec::new(),
                watch: true,
                transfer: LocationTransfer::Move,
                no_text: OnNoText::Ask,
                several: OnSeveralTargets::Ask,
                own_volume,
            };
            let root = Path::new(root);
            let device_of = |path: &Path| {
                if path == root {
                    device.map(str::to_owned)
                } else {
                    Some("system".to_owned())
                }
            };
            source_is_mounted_with(&source, root, device_of, &[PathBuf::from("/")])
        };
        for place in ["/Users/me/Downloads", "/home/me/Downloads", "/Volumes", "/mnt2/x"] {
            assert!(is_mounted(place, None, Some("system")), "{place}");
            assert!(is_mounted(place, None, None), "{place}");
            assert!(is_mounted(place, Some(true), Some("usb")), "{place}");
            assert!(!is_mounted(place, Some(true), Some("system")), "{place}");
            assert!(!is_mounted(place, Some(true), None), "{place}");
        }
        for place in ["/Volumes/Stick/in", "/media/me/stick", "/run/media/me/stick", "/mnt/x"] {
            assert!(!is_mounted(place, None, Some("system")), "{place}");
            assert!(!is_mounted(place, None, None), "{place}");
            assert!(is_mounted(place, None, Some("usb")), "{place}");
            assert!(is_mounted(place, Some(false), Some("system")), "{place}");
        }
    }
}
