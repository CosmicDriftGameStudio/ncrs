use std::fs::File;
use std::time::Duration;

use ncrs_os_probe::{folder_in_use, running_processes, volume_space, FolderUse, UnknownWhy};

const GENEROUS: Duration = Duration::from_secs(60);

#[cfg(any(all(target_os = "macos", not(feature = "sandbox")), target_os = "linux", windows))]
#[test]
fn a_file_this_process_holds_open_makes_the_folder_held_by_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("open.txt");
    let _open = File::create(&path).unwrap();

    match folder_in_use(dir.path(), GENEROUS) {
        FolderUse::Held(holders) => {
            assert!(holders.iter().any(|holder| holder.pid == std::process::id()), "{holders:?}");
        }
        other => panic!("expected Held, got {other:?}"),
    }
}

#[cfg(any(all(target_os = "macos", not(feature = "sandbox")), target_os = "linux"))]
#[test]
fn a_process_with_its_working_directory_in_the_folder_holds_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = std::process::Command::new("sleep")
        .arg("60")
        .current_dir(dir.path())
        .spawn()
        .unwrap();

    let answer = folder_in_use(dir.path(), GENEROUS);
    child.kill().unwrap();
    child.wait().unwrap();

    match answer {
        FolderUse::Held(holders) => {
            assert!(holders.iter().any(|holder| holder.pid == child.id()), "{holders:?}");
        }
        other => panic!("expected Held, got {other:?}"),
    }
}

#[cfg(any(all(target_os = "macos", not(feature = "sandbox")), target_os = "linux", windows))]
#[test]
fn a_folder_nobody_uses_is_free() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("closed.txt"), "x").unwrap();

    assert_eq!(folder_in_use(dir.path(), GENEROUS), FolderUse::Free);
}

#[cfg(any(all(target_os = "macos", not(feature = "sandbox")), target_os = "linux", windows))]
#[test]
fn a_zero_limit_ends_as_timed_out_or_with_the_real_answer_never_as_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let _open = File::create(dir.path().join("open.txt")).unwrap();

    match folder_in_use(dir.path(), Duration::ZERO) {
        FolderUse::Unknown(UnknownWhy::TimedOut) | FolderUse::Held(_) => {}
        other => panic!("unexpected {other:?}"),
    }
}

#[cfg(all(target_os = "macos", feature = "sandbox"))]
#[test]
fn the_store_build_cannot_look() {
    let dir = tempfile::tempdir().unwrap();

    assert_eq!(
        folder_in_use(dir.path(), GENEROUS),
        FolderUse::Unknown(UnknownWhy::NotAllowed)
    );
    assert!(running_processes().is_none());
}

#[cfg(not(feature = "sandbox"))]
#[test]
fn the_process_list_holds_this_process() {
    let processes = running_processes().unwrap();

    assert!(processes.iter().any(|holder| holder.pid == std::process::id()));
}

#[test]
fn a_volume_has_total_at_least_available_and_some_room() {
    let dir = tempfile::tempdir().unwrap();

    let space = volume_space(dir.path()).unwrap();

    assert!(space.total >= space.available, "{space:?}");
    assert!(space.available > 0, "{space:?}");
}
