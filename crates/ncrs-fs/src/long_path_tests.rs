//! Paths past the 260 characters of MAX_PATH on Windows, through the trash and
//! the permanent delete.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code: a failing assertion is the signal"
)]

use std::path::PathBuf;

use super::{remove_permanently, system_trash};

const BEYOND_MAX_PATH: usize = 260;

fn file_beyond_max_path(root: &std::path::Path, name: &str) -> PathBuf {
    let mut folder = root.to_path_buf();
    let mut index = 0;
    while folder.join(name).as_os_str().len() <= BEYOND_MAX_PATH {
        folder.push(format!("folder-with-a-long-name-{index:02}"));
        index += 1;
    }
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join(name);
    std::fs::write(&file, "content").unwrap();
    file
}

#[test]
fn a_file_beyond_max_path_goes_to_the_recycle_bin() {
    let dir = tempfile::tempdir().unwrap();
    let file = file_beyond_max_path(dir.path(), "ncrs-long-path-trash.txt");
    assert!(file.as_os_str().len() > BEYOND_MAX_PATH);

    system_trash(None).trash(&file).unwrap();

    assert!(!file.exists());
}

#[test]
fn a_file_beyond_max_path_is_deleted_permanently() {
    let dir = tempfile::tempdir().unwrap();
    let file = file_beyond_max_path(dir.path(), "ncrs-long-path-delete.txt");
    assert!(file.as_os_str().len() > BEYOND_MAX_PATH);

    remove_permanently(&file).unwrap();

    assert!(!file.exists());
}
