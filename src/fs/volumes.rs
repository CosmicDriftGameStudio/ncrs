//! The places the drive menu offers: the same ones the Finder sidebar lists
//! under "Locations".
//!
//! Listing must never touch a mount: a network share that went away hangs
//! `stat` for as long as the kernel retries, and the menu opens synchronously.
//! So this reads names only (`read_dir`, `/proc/mounts`, `read_link`) and checks
//! nothing about what it finds. Whether a volume can be entered is found out by
//! the directory read the choice starts, which reports an error like any other.

use std::path::{Path, PathBuf};

use super::home_dir;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    pub name: String,
    pub path: PathBuf,
}

impl Volume {
    fn new(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
        }
    }

    /// A volume named after the last component of its path.
    #[cfg(any(target_os = "linux", test))]
    fn named_by_path(path: PathBuf) -> Self {
        let name = last_component(&path).unwrap_or_else(|| path.to_string_lossy().into_owned());
        Self { name, path }
    }
}

fn last_component(path: &Path) -> Option<String> {
    path.file_name().map(|n| n.to_string_lossy().into_owned())
}

fn home_volume() -> Volume {
    let home = home_dir();
    let name = last_component(&home).unwrap_or_else(|| "Home".to_string());
    Volume::new(name, home)
}

/// The volumes to offer: the system root first, then home, then the mounted
/// ones by name. Each path appears once.
pub fn list_volumes() -> Vec<Volume> {
    let mut volumes = platform_volumes();
    let mut seen = std::collections::HashSet::new();
    volumes.retain(|volume| seen.insert(volume.path.clone()));
    volumes
}

#[cfg(target_os = "macos")]
fn platform_volumes() -> Vec<Volume> {
    let (scanned, root_name) = scan_volumes_dir(Path::new("/Volumes"));
    let mounted = mount_output()
        .and_then(|output| browsable_volumes(&output, root_name.as_deref()))
        .unwrap_or(scanned);
    let mut volumes = vec![Volume::new(
        root_name.unwrap_or_else(|| "Macintosh HD".to_string()),
        "/",
    )];
    volumes.push(home_volume());
    let icloud = home_dir().join("Library/Mobile Documents/com~apple~CloudDocs");
    if icloud.exists() {
        volumes.push(Volume::new("iCloud Drive", icloud));
    }
    volumes.extend(mounted);
    volumes
}

#[cfg(target_os = "linux")]
fn platform_volumes() -> Vec<Volume> {
    let mounts = std::fs::read(MOUNTS_FILE)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    let mut volumes = vec![Volume::new("/", "/"), home_volume()];
    volumes.extend(mounted_volumes(&mounts));
    volumes
}

#[cfg(windows)]
fn platform_volumes() -> Vec<Volume> {
    let mut volumes: Vec<Volume> = ('A'..='Z')
        .map(|letter| format!("{letter}:\\"))
        .filter(|root| Path::new(root).exists())
        .map(|root| Volume::new(root.clone(), root))
        .collect();
    volumes.push(home_volume());
    volumes
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn platform_volumes() -> Vec<Volume> {
    vec![Volume::new("/", "/"), home_volume()]
}

/// What `/Volumes` holds, sorted by name, plus the name of the entry that is
/// only a link to the system volume. That link is the system volume's name, and
/// it is listed as `/` instead of twice.
///
/// `read_dir` and `file_type` only: `file_type` comes from the directory entry
/// itself, and the link is judged by its text, never by following it.
#[cfg(any(target_os = "macos", test))]
fn scan_volumes_dir(dir: &Path) -> (Vec<Volume>, Option<String>) {
    let mut volumes = Vec::new();
    let mut root_name = None;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (volumes, root_name);
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Some(name) = last_component(&path) else {
            continue;
        };
        let is_symlink = entry.file_type().is_ok_and(|kind| kind.is_symlink());
        if is_symlink && std::fs::read_link(&path).is_ok_and(|target| target == Path::new("/")) {
            root_name = Some(name);
        } else {
            volumes.push(Volume::new(name, path));
        }
    }
    volumes.sort_by_cached_key(|volume| volume.name.to_lowercase());
    (volumes, root_name)
}

/// One line of `mount` output, as far as the drive menu needs it.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug)]
struct MountEntry {
    mount_point: PathBuf,
    fs_type: String,
    from: String,
    hidden: bool,
}

/// `<from> on <mount point> (<fstype>, <option>, ...)`. The option list is cut
/// off from the right, because a mount point may contain spaces and parentheses;
/// the source is whatever precedes the first ` on `. `nobrowse` among the
/// options is what the Finder honours to leave a mount out of its sidebar.
#[cfg(any(target_os = "macos", test))]
fn parse_mount_line(line: &str) -> Option<MountEntry> {
    let without_paren = line.trim_end().strip_suffix(')')?;
    let (head, option_list) = without_paren.rsplit_once(" (")?;
    let (from, mount_point) = head.split_once(" on ")?;
    let mut fields = option_list.split(',').map(str::trim);
    let fs_type = fields.next()?.to_string();
    let hidden = fields.any(|option| option == "nobrowse");
    Some(MountEntry {
        mount_point: PathBuf::from(mount_point),
        fs_type,
        from: from.to_string(),
        hidden,
    })
}

/// A macFUSE volume is named after its mount point, which for Cryptomator is a
/// random string. Its source reads `Cryptomator@macfuse0`: the part before the
/// `@` says what mounted it.
#[cfg(any(target_os = "macos", test))]
fn display_name(entry: &MountEntry, name: String) -> String {
    match entry.from.split_once('@') {
        Some((origin, _)) if entry.fs_type == "macfuse" && !origin.is_empty() => {
            format!("{name} ({origin})")
        }
        _ => name,
    }
}

/// The volumes among the lines of `mount`'s output that the Finder would show:
/// under `/Volumes`, not nobrowse, and not named like the link to the system
/// volume. Sorted by name. `None` if no line could be read at all.
#[cfg(any(target_os = "macos", test))]
fn browsable_volumes(mount_output: &str, system_link: Option<&str>) -> Option<Vec<Volume>> {
    let entries: Vec<MountEntry> = mount_output.lines().filter_map(parse_mount_line).collect();
    if entries.is_empty() {
        return None;
    }
    let mut volumes: Vec<Volume> = entries
        .into_iter()
        .filter(|entry| !entry.hidden)
        .filter(|entry| {
            entry.mount_point.starts_with("/Volumes") && entry.mount_point != Path::new("/Volumes")
        })
        .filter_map(|entry| {
            let name = last_component(&entry.mount_point)?;
            (system_link != Some(name.as_str())).then(|| {
                let shown = display_name(&entry, name);
                Volume::new(shown, entry.mount_point)
            })
        })
        .collect();
    volumes.sort_by_cached_key(|volume| volume.name.to_lowercase());
    Some(volumes)
}

/// The output of `/sbin/mount` without arguments. It lists the kernel's mount
/// table through `getfsstat(MNT_NOWAIT)`: cached data, no file system is asked,
/// so a dead share cannot hang it. Full path and no shell, so nothing on `PATH`
/// is run instead. `None` if it did not run or failed.
#[cfg(target_os = "macos")]
fn mount_output() -> Option<String> {
    let output = std::process::Command::new("/sbin/mount").output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(target_os = "linux")]
const MOUNTS_FILE: &str = "/proc/mounts";

/// Mount points of removable media and manual mounts.
#[cfg(any(target_os = "linux", test))]
const MOUNT_PARENTS: [&str; 3] = ["/media", "/run/media", "/mnt"];

/// The volumes among the lines of `/proc/mounts`: mount points under
/// [`MOUNT_PARENTS`], sorted by name. Other mounts (`/proc`, `/sys`, bind
/// mounts of system paths) are not places a user goes to.
#[cfg(any(target_os = "linux", test))]
fn mounted_volumes(mounts: &str) -> Vec<Volume> {
    let mut volumes: Vec<Volume> = mounts
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .map(|field| PathBuf::from(unescape_mount_field(field)))
        .filter(|path| MOUNT_PARENTS.iter().any(|parent| path.starts_with(parent)))
        .map(Volume::named_by_path)
        .collect();
    volumes.sort_by_cached_key(|volume| volume.name.to_lowercase());
    volumes
}

/// `/proc/mounts` writes a space, tab, newline or backslash in a path as a
/// backslash and three octal digits (`\040`).
#[cfg(any(target_os = "linux", test))]
fn unescape_mount_field(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        let octal = bytes
            .get(index + 1..index + 4)
            .filter(|_| byte == b'\\')
            .filter(|digits| digits.iter().all(|d| (b'0'..=b'7').contains(d)))
            .and_then(|digits| std::str::from_utf8(digits).ok())
            .and_then(|digits| u8::from_str_radix(digits, 8).ok());
        match octal {
            Some(value) => {
                out.push(value);
                index += 4;
            }
            None => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(volumes: &[Volume]) -> Vec<(&str, &str)> {
        volumes
            .iter()
            .map(|v| (v.name.as_str(), v.path.to_str().unwrap()))
            .collect()
    }

    #[test]
    fn only_removable_and_manual_mounts_are_volumes() {
        let mounts = "\
sysfs /sys sysfs rw,nosuid 0 0
/dev/nvme0n1p2 / ext4 rw 0 0
/dev/sda1 /run/media/marc/STICK vfat rw 0 0
server:/share /mnt/nas nfs rw 0 0
/dev/sdb1 /media/disk\\040two ext4 rw 0 0
tmpfs /run/user/1000 tmpfs rw 0 0
/dev/sdc1 /mnt2 ext4 rw 0 0
";
        assert_eq!(
            pairs(&mounted_volumes(mounts)),
            [
                ("disk two", "/media/disk two"),
                ("nas", "/mnt/nas"),
                ("STICK", "/run/media/marc/STICK"),
            ]
        );
    }

    #[test]
    fn octal_escapes_are_decoded() {
        assert_eq!(unescape_mount_field("/mnt/a\\040b"), "/mnt/a b");
        assert_eq!(unescape_mount_field("/mnt/t\\011ab\\012"), "/mnt/t\tab\n");
        assert_eq!(
            unescape_mount_field("/mnt/back\\134slash"),
            "/mnt/back\\slash"
        );
        // Not an escape: too short, or not octal.
        assert_eq!(unescape_mount_field("/mnt/a\\04"), "/mnt/a\\04");
        assert_eq!(unescape_mount_field("/mnt/a\\089"), "/mnt/a\\089");
        // Multi-byte UTF-8 is written as plain bytes, escapes are decoded as bytes.
        assert_eq!(unescape_mount_field("/mnt/Gr\u{fc}n"), "/mnt/Gr\u{fc}n");
        assert_eq!(unescape_mount_field("/mnt/Gr\\303\\274n"), "/mnt/Gr\u{fc}n");
    }

    #[test]
    fn mounts_without_a_second_field_are_skipped() {
        assert!(mounted_volumes("garbage\n\n").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn the_link_to_the_system_volume_gives_its_name_and_is_not_listed() {
        let dir = tempfile::Builder::new()
            .prefix("ncrs-volumes-")
            .tempdir()
            .unwrap();
        std::os::unix::fs::symlink("/", dir.path().join("Macintosh HD")).unwrap();
        std::fs::create_dir(dir.path().join("usb")).unwrap();
        std::fs::create_dir(dir.path().join("Backup")).unwrap();
        // A link elsewhere is a volume of its own.
        std::os::unix::fs::symlink(dir.path().join("usb"), dir.path().join("alias")).unwrap();

        let (volumes, root_name) = scan_volumes_dir(dir.path());
        assert_eq!(root_name.as_deref(), Some("Macintosh HD"));
        let names: Vec<_> = volumes.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, ["alias", "Backup", "usb"]);
        assert_eq!(volumes[2].path, dir.path().join("usb"));
    }

    const MOUNT_OUTPUT: &str = "\
/dev/disk3s1s1 on / (apfs, sealed, local, read-only, journaled)
/dev/disk3s5 on /System/Volumes/Data (apfs, local, journaled, nobrowse, protect, root data)
map auto_home on /System/Volumes/Data/home (autofs, automounted, nobrowse)
Cryptomator@macfuse0 on /Volumes/UzeN1wWoFdWU (macfuse, nodev, nosuid, synchronous, mounted by marc)
/dev/disk10s1 on /Volumes/dmg.tLR3n9 (hfs, local, nodev, nosuid, noowners, nobrowse, mounted by marc)
//me@nas.local/share on /Volumes/share (smbfs, nodev, nosuid, mounted by marc)
/dev/disk4s1 on /Volumes/My (Backup) on disk (apfs, local, nodev, nosuid, journaled)
/dev/disk5s1 on /Volumes/Macintosh HD (apfs, local)
";

    #[test]
    fn hidden_and_foreign_mounts_are_not_volumes() {
        let volumes = browsable_volumes(MOUNT_OUTPUT, Some("Macintosh HD")).unwrap();
        assert_eq!(
            pairs(&volumes),
            [
                ("My (Backup) on disk", "/Volumes/My (Backup) on disk"),
                ("share", "/Volumes/share"),
                ("UzeN1wWoFdWU (Cryptomator)", "/Volumes/UzeN1wWoFdWU"),
            ]
        );
    }

    #[test]
    fn a_mount_point_with_spaces_and_parentheses_is_kept_whole() {
        let entry = parse_mount_line("/dev/d on /Volumes/A (b) c (apfs, local)").unwrap();
        assert_eq!(entry.mount_point, Path::new("/Volumes/A (b) c"));
        assert_eq!(entry.fs_type, "apfs");
        assert!(!entry.hidden);
    }

    #[test]
    fn only_macfuse_sources_with_an_origin_extend_the_name() {
        let named = |line: &str| {
            let volumes = browsable_volumes(line, None).unwrap();
            volumes[0].name.clone()
        };
        assert_eq!(named("@macfuse1 on /Volumes/bare (macfuse, local)"), "bare");
        assert_eq!(
            named("macfuse2 on /Volumes/plain (macfuse, local)"),
            "plain"
        );
        assert_eq!(named("a@b on /Volumes/other (smbfs, local)"), "other");
    }

    #[test]
    fn garbage_output_is_no_answer() {
        assert!(browsable_volumes("", None).is_none());
        assert!(browsable_volumes("not a mount line\n", None).is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_real_mount_output_has_the_root() {
        let output = mount_output().expect("mount failed");
        assert!(output
            .lines()
            .filter_map(parse_mount_line)
            .any(|entry| entry.mount_point == Path::new("/")));
        assert!(browsable_volumes(&output, None).is_some());
    }

    #[test]
    fn a_missing_volumes_directory_is_an_empty_list() {
        let (volumes, root_name) = scan_volumes_dir(Path::new("/definitely/not/here"));
        assert!(volumes.is_empty());
        assert_eq!(root_name, None);
    }

    #[test]
    fn the_real_list_has_the_root_or_a_drive_and_no_duplicates() {
        let volumes = list_volumes();
        assert!(!volumes.is_empty());
        let mut paths: Vec<_> = volumes.iter().map(|v| v.path.clone()).collect();
        let before = paths.len();
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), before, "{volumes:?}");
    }
}
