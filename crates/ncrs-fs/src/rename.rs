//! Renaming one entry inside its folder, here or on an SFTP server.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::remote::session::Pool;
use crate::remote::{transfer, RemotePath};

/// Renames the entry at `from` to `new_name` in the same folder and returns
/// the new path. Nothing is replaced: a name another entry already has fails
/// with `AlreadyExists`. Giving the entry's own name again is a no-op, and so
/// is a change of case only, which a case-insensitive file system reports as
/// taken because it is the entry itself.
pub async fn rename(pool: Arc<Pool>, from: PathBuf, new_name: String) -> io::Result<PathBuf> {
    tokio::task::spawn_blocking(move || rename_blocking(&pool, &from, &new_name))
        .await
        .map_err(io::Error::other)?
}

/// As [`rename`], on the calling thread. On a server path the thread must be
/// one of the runtime's blocking threads.
fn rename_blocking(pool: &Pool, from: &Path, new_name: &str) -> io::Result<PathBuf> {
    match RemotePath::parse(from) {
        Some(remote) => transfer::rename(pool, &remote, new_name).map(|to| to.to_path_buf()),
        None => rename_local(from, new_name),
    }
}

fn rename_local(from: &Path, new_name: &str) -> io::Result<PathBuf> {
    if new_name.is_empty()
        || new_name == "."
        || new_name == ".."
        || new_name.contains(['/', std::path::MAIN_SEPARATOR])
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a name is one component",
        ));
    }
    let to = from.with_file_name(new_name);
    if to == from {
        return Ok(to);
    }
    // The check and the rename are two steps; `rename` itself would replace a
    // file, and there is no portable rename that refuses to.
    if std::fs::symlink_metadata(&to).is_ok()
        && !(differs_in_case_only(from, &to) && is_same_entry(from, &to))
    {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "an entry with this name exists",
        ));
    }
    std::fs::rename(from, &to)?;
    Ok(to)
}

/// A second hard link to the same inode is the same entry too, but renaming
/// onto it would silently do nothing; only a change of case is the exception.
fn differs_in_case_only(from: &Path, to: &Path) -> bool {
    match (from.file_name(), to.file_name()) {
        (Some(old), Some(new)) => {
            old.to_string_lossy().to_lowercase() == new.to_string_lossy().to_lowercase()
        }
        _ => false,
    }
}

/// Whether both paths name one entry: a case-insensitive file system shows the
/// entry under either spelling. Compared without following links, so a second
/// link to the same target is a different entry.
#[cfg(unix)]
fn is_same_entry(first: &Path, second: &Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    match (
        std::fs::symlink_metadata(first),
        std::fs::symlink_metadata(second),
    ) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn is_same_entry(first: &Path, second: &Path) -> bool {
    same_file::is_same_file(first, second).unwrap_or(false)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn pool() -> Pool {
        Pool::over_tcp(crate::remote::KnownHostsFiles::default())
    }

    #[test]
    fn a_file_gets_its_new_name() {
        let scratch = tempfile::tempdir().unwrap();
        let from = scratch.path().join("old.txt");
        std::fs::write(&from, "content").unwrap();

        let to = rename_blocking(&pool(), &from, "new.txt").unwrap();

        assert_eq!(to, scratch.path().join("new.txt"));
        assert!(!from.exists());
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "content");
    }

    #[test]
    fn a_folder_keeps_its_content_under_the_new_name() {
        let scratch = tempfile::tempdir().unwrap();
        let from = scratch.path().join("old");
        std::fs::create_dir(&from).unwrap();
        std::fs::write(from.join("inside.txt"), "x").unwrap();

        let to = rename_blocking(&pool(), &from, "new").unwrap();

        assert!(!from.exists());
        assert!(to.join("inside.txt").is_file());
    }

    #[test]
    fn a_name_another_entry_has_is_refused_and_nothing_changes() {
        let scratch = tempfile::tempdir().unwrap();
        let from = scratch.path().join("a.txt");
        let taken = scratch.path().join("b.txt");
        std::fs::write(&from, "a").unwrap();
        std::fs::write(&taken, "b").unwrap();

        let error = rename_blocking(&pool(), &from, "b.txt").unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&from).unwrap(), "a");
        assert_eq!(std::fs::read_to_string(&taken).unwrap(), "b");
    }

    #[test]
    fn a_name_with_a_separator_is_refused() {
        let scratch = tempfile::tempdir().unwrap();
        let from = scratch.path().join("a.txt");
        std::fs::write(&from, "a").unwrap();

        for bad in ["sub/b.txt", "", ".", ".."] {
            let error = rename_blocking(&pool(), &from, bad).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{bad:?}");
        }
        assert!(from.exists());
    }

    #[test]
    fn the_same_name_changes_nothing() {
        let scratch = tempfile::tempdir().unwrap();
        let from = scratch.path().join("a.txt");
        std::fs::write(&from, "a").unwrap();

        assert_eq!(rename_blocking(&pool(), &from, "a.txt").unwrap(), from);
        assert!(from.exists());
    }

    #[test]
    fn a_change_of_case_only_is_allowed() {
        let scratch = tempfile::tempdir().unwrap();
        let from = scratch.path().join("note.txt");
        std::fs::write(&from, "n").unwrap();

        let to = rename_blocking(&pool(), &from, "Note.txt").unwrap();

        let names: Vec<_> = std::fs::read_dir(scratch.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["Note.txt"]);
        assert_eq!(to, scratch.path().join("Note.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn a_second_hard_link_to_the_same_file_is_in_the_way() {
        let scratch = tempfile::tempdir().unwrap();
        let from = scratch.path().join("a.txt");
        let link = scratch.path().join("b.txt");
        std::fs::write(&from, "a").unwrap();
        std::fs::hard_link(&from, &link).unwrap();

        let error = rename_blocking(&pool(), &from, "b.txt").unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(from.exists());
    }

    /// Two spellings are two entries on a case-sensitive file system, and the
    /// second one is then in the way.
    #[test]
    fn a_case_variant_is_taken_only_where_the_system_tells_cases_apart() {
        let scratch = tempfile::tempdir().unwrap();
        let from = scratch.path().join("a.txt");
        let other = scratch.path().join("A.TXT");
        std::fs::write(&from, "a").unwrap();
        std::fs::write(&other, "other").unwrap();
        let both_exist = std::fs::read_dir(scratch.path()).unwrap().count() == 2;
        if !both_exist {
            return;
        }

        let error = rename_blocking(&pool(), &from, "A.TXT").unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    }
}
