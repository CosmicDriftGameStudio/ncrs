//! The checks right before a delete: one test per reason to leave a folder alone.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod common;

use common::{entry_for, write, Fake};
use ncrs_space::{check_before_delete, CatalogId, FolderUse, Holder, Skip, UnknownUse};
use tempfile::TempDir;

fn cache() -> (TempDir, std::path::PathBuf) {
    let home = TempDir::new().unwrap();
    let folder = home.path().join("target");
    write(&folder.join("debug/out"), 100);
    (home, folder)
}

#[test]
fn a_quiet_unchanged_folder_passes() {
    let (_home, folder) = cache();
    let entry = entry_for(&folder, CatalogId::CargoTarget);
    assert_eq!(
        check_before_delete(&entry, &Fake::quiet(), UnknownUse::Skip),
        Ok(())
    );
}

#[test]
fn a_running_tool_of_the_folder_stops_it_but_another_process_does_not() {
    let (_home, folder) = cache();
    let entry = entry_for(&folder, CatalogId::CargoTarget);
    let holder = Holder {
        pid: 42,
        name: "cargo".to_owned(),
    };
    assert_eq!(
        check_before_delete(&entry, &Fake::running("Cargo.exe"), UnknownUse::Skip),
        Err(Skip::ToolRunning(Holder {
            pid: 42,
            name: "Cargo.exe".to_owned()
        }))
    );
    assert_eq!(
        check_before_delete(&entry, &Fake::running("cargo"), UnknownUse::Skip),
        Err(Skip::ToolRunning(holder))
    );
    assert_eq!(
        check_before_delete(&entry, &Fake::running("firefox"), UnknownUse::Skip),
        Ok(())
    );
}

#[test]
fn an_open_file_in_the_folder_stops_it() {
    let (_home, folder) = cache();
    let entry = entry_for(&folder, CatalogId::CargoTarget);
    let holder = Holder {
        pid: 7,
        name: "editor".to_owned(),
    };
    let probes = Fake {
        folder: FolderUse::Held(vec![holder.clone()]),
        ..Fake::quiet()
    };
    assert_eq!(
        check_before_delete(&entry, &probes, UnknownUse::Skip),
        Err(Skip::FolderOpen(holder))
    );
}

#[test]
fn an_unknown_answer_counts_as_in_use_until_the_user_confirmed() {
    let (_home, folder) = cache();
    let entry = entry_for(&folder, CatalogId::CargoTarget);
    let no_list = Fake {
        processes: None,
        ..Fake::quiet()
    };
    let no_look = Fake {
        folder: FolderUse::Unknown(ncrs_space::UnknownWhy::TimedOut),
        ..Fake::quiet()
    };
    for probes in [&no_list, &no_look] {
        assert_eq!(
            check_before_delete(&entry, probes, UnknownUse::Skip),
            Err(Skip::UseUnknown)
        );
        assert_eq!(
            check_before_delete(&entry, probes, UnknownUse::AllowAfterConfirm),
            Ok(())
        );
    }
}

#[cfg(unix)]
#[test]
fn a_symlink_is_never_deleted() {
    let (home, folder) = cache();
    let link = home.path().join("link");
    std::os::unix::fs::symlink(&folder, &link).unwrap();
    let mut entry = entry_for(&folder, CatalogId::CargoTarget);
    entry.path = link;
    assert_eq!(
        check_before_delete(&entry, &Fake::quiet(), UnknownUse::Skip),
        Err(Skip::Symlink)
    );
}

#[cfg(unix)]
#[test]
fn a_folder_on_another_device_than_measured_is_left_alone() {
    let (_home, folder) = cache();
    let mut entry = entry_for(&folder, CatalogId::CargoTarget);
    entry.device = entry.device.map(|device| device.wrapping_add(1));
    assert_eq!(
        check_before_delete(&entry, &Fake::quiet(), UnknownUse::Skip),
        Err(Skip::OtherDrive)
    );
}

#[test]
fn a_changed_size_or_time_needs_a_new_measure() {
    let (_home, folder) = cache();
    let entry = entry_for(&folder, CatalogId::CargoTarget);

    write(&folder.join("debug/out"), 250);
    assert_eq!(
        check_before_delete(&entry, &Fake::quiet(), UnknownUse::Skip),
        Err(Skip::Changed)
    );

    let fresh = entry_for(&folder, CatalogId::CargoTarget);
    write(&folder.join("debug/more"), 1);
    assert_eq!(
        check_before_delete(&fresh, &Fake::quiet(), UnknownUse::Skip),
        Err(Skip::Changed)
    );
}

#[test]
fn a_folder_that_vanished_is_gone() {
    let (_home, folder) = cache();
    let entry = entry_for(&folder, CatalogId::CargoTarget);
    std::fs::remove_dir_all(&folder).unwrap();
    assert_eq!(
        check_before_delete(&entry, &Fake::quiet(), UnknownUse::Skip),
        Err(Skip::Gone)
    );
}

#[cfg(unix)]
#[test]
fn a_folder_the_app_may_not_look_into_is_not_granted() {
    use std::os::unix::fs::PermissionsExt as _;
    let (home, folder) = cache();
    let entry = entry_for(&folder, CatalogId::CargoTarget);
    let locked = std::fs::metadata(home.path()).unwrap().permissions();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o000)).unwrap();
    let outcome = check_before_delete(&entry, &Fake::quiet(), UnknownUse::Skip);
    std::fs::set_permissions(home.path(), locked).unwrap();
    assert_eq!(outcome, Err(Skip::NotGranted));
}
