//! Putting a finished entry under its final name, and what a copy carries over
//! from its original. Every write path that stages first uses these two, so
//! none of them has its own half of the pattern.

use std::io;
use std::path::Path;

/// Renames `from` to `to` and fails with `AlreadyExists` when `to` is taken,
/// instead of replacing it. The kernel decides where it can (macOS and Linux);
/// elsewhere, and on a file system that cannot, it is a check followed by the
/// rename, which leaves a window for someone creating the name in between.
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        use rustix::fs::{renameat_with, RenameFlags, CWD};
        use rustix::io::Errno;
        match renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE) {
            Ok(()) => return Ok(()),
            Err(Errno::INVAL | Errno::NOSYS | Errno::NOTSUP) => {}
            Err(other) => return Err(other.into()),
        }
    }
    match std::fs::symlink_metadata(to) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => std::fs::rename(from, to),
        Err(e) => Err(e),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", to.display()),
        )),
    }
}

/// Sets the modification time of a folder, which cannot be opened like a file
/// on Windows.
pub fn set_folder_modified(folder: &Path, modified: std::time::SystemTime) -> io::Result<()> {
    #[cfg(windows)]
    let handle = {
        use std::os::windows::fs::OpenOptionsExt as _;
        // FILE_WRITE_ATTRIBUTES, and FILE_FLAG_BACKUP_SEMANTICS, without which
        // Windows does not open a folder at all.
        std::fs::OpenOptions::new()
            .access_mode(0x0100)
            .custom_flags(0x0200_0000)
            .open(folder)?
    };
    #[cfg(not(windows))]
    let handle = std::fs::File::open(folder)?;
    handle.set_modified(modified)
}

/// Copies the extended attributes of `source` to `target`: the download
/// quarantine on macOS, Finder tags, the resource fork. A file system that
/// cannot hold one (exFAT, a share) must not fail the copy, so the copy goes
/// on, and the names of the attributes the target refused for another reason
/// are returned for the caller to report. A refusal because the target holds no
/// attributes at all, and `com.apple.provenance`, which only the system sets,
/// are not reported.
pub fn copy_xattrs(source: &Path, target: &Path) -> Vec<String> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        use rustix::fs::{
            lgetxattr, lgetxattr_query, llistxattr, llistxattr_query, lsetxattr, XattrFlags,
        };
        use rustix::io::Errno;
        let mut refused = Vec::new();
        let size = match llistxattr_query(source) {
            Ok(size) => size,
            Err(Errno::NOTSUP) => return refused,
            Err(_) => {
                refused.push("(list)".to_owned());
                return refused;
            }
        };
        let mut listed = vec![0; size];
        let Ok(used) = llistxattr(source, &mut *listed) else {
            refused.push("(list)".to_owned());
            return refused;
        };
        for raw_name in listed
            .get(..used)
            .unwrap_or_default()
            .split(|byte| *byte == 0)
        {
            let Ok(name) = std::ffi::CString::new(raw_name) else {
                continue;
            };
            if name.as_bytes().is_empty() || name.as_bytes() == b"com.apple.provenance" {
                continue;
            }
            let shown = name.to_string_lossy().into_owned();
            let Ok(len) = lgetxattr_query(source, name.as_c_str()) else {
                refused.push(shown);
                continue;
            };
            let mut value = vec![0; len];
            let Ok(got) = lgetxattr(source, name.as_c_str(), &mut *value) else {
                refused.push(shown);
                continue;
            };
            match lsetxattr(
                target,
                name.as_c_str(),
                value.get(..got).unwrap_or_default(),
                XattrFlags::empty(),
            ) {
                Ok(()) | Err(Errno::NOTSUP) => {}
                Err(_) => refused.push(shown),
            }
        }
        refused
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (source, target);
        Vec::new()
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_taken_name_is_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from");
        let to = dir.path().join("to");
        std::fs::write(&from, b"new").unwrap();
        std::fs::write(&to, b"old").unwrap();

        let err = rename_no_replace(&from, &to).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&to).unwrap(), b"old");
        assert_eq!(std::fs::read(&from).unwrap(), b"new");
    }

    #[test]
    fn a_free_name_is_taken() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from");
        let to = dir.path().join("to");
        std::fs::write(&from, b"new").unwrap();

        rename_no_replace(&from, &to).unwrap();

        assert_eq!(std::fs::read(&to).unwrap(), b"new");
        assert!(!from.exists());
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn extended_attributes_follow_the_copy() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from");
        let to = dir.path().join("to");
        std::fs::write(&from, b"x").unwrap();
        std::fs::write(&to, b"x").unwrap();
        let name = if cfg!(target_os = "macos") {
            "com.apple.quarantine"
        } else {
            "user.ncrs-test"
        };
        if rustix::fs::lsetxattr(
            &from,
            name,
            b"0081;5f3a;Safari",
            rustix::fs::XattrFlags::empty(),
        )
        .is_err()
        {
            // This file system holds no attributes; nothing to carry over.
            return;
        }

        assert!(copy_xattrs(&from, &to).is_empty());

        let mut value = [0; 64];
        let got = rustix::fs::lgetxattr(&to, name, &mut value).unwrap();
        assert_eq!(value.get(..got), Some(&b"0081;5f3a;Safari"[..]));
    }
}
