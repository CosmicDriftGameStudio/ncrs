//! Which folders the catalog lists on each system.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod common;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use common::write;
use ncrs_space::{catalog, CatalogId, Os, Place, SpaceSettings};
use tempfile::TempDir;

fn no_env(_: &str) -> Option<OsString> {
    None
}

fn ids(places: &[Place]) -> Vec<CatalogId> {
    places.iter().map(|place| place.id).collect()
}

fn project(root: &Path, name: &str, with_manifest: bool) -> PathBuf {
    let folder = root.join(name);
    write(&folder.join("target/debug/out"), 10);
    if with_manifest {
        write(&folder.join("Cargo.toml"), 1);
    }
    folder
}

#[test]
fn a_target_folder_is_found_only_beside_a_cargo_toml() {
    let home = TempDir::new().unwrap();
    let rust = project(home.path(), "rust-app", true);
    let other = project(home.path(), "not-rust", false);

    let places = catalog(Os::Linux, home.path(), &no_env, &[rust.clone(), other], &SpaceSettings::default());

    assert_eq!(ids(&places), [CatalogId::CargoTarget]);
    assert_eq!(places[0].path, rust.join("target"));
    assert!(places[0].tools.contains(&"cargo"));
}

#[test]
fn only_what_exists_is_listed_and_a_symlink_is_not() {
    let home = TempDir::new().unwrap();
    assert!(catalog(Os::Linux, home.path(), &no_env, &[], &SpaceSettings::default()).is_empty());

    write(&home.path().join(".bun/install/cache/a"), 5);
    let places = catalog(Os::Linux, home.path(), &no_env, &[], &SpaceSettings::default());
    assert_eq!(ids(&places), [CatalogId::Bun]);

    #[cfg(unix)]
    {
        let elsewhere = TempDir::new().unwrap();
        std::fs::create_dir_all(home.path().join(".gradle")).unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), home.path().join(".gradle/caches")).unwrap();
        let places = catalog(Os::Linux, home.path(), &no_env, &[], &SpaceSettings::default());
        assert_eq!(ids(&places), [CatalogId::Bun]);
    }
}

#[test]
fn a_worktree_of_a_listed_repo_gets_its_target_too() {
    let home = TempDir::new().unwrap();
    let repo = project(home.path(), "repo", true);
    let linked = project(home.path(), "linked", true);
    let admin = repo.join(".git/worktrees/linked");
    std::fs::create_dir_all(&admin).unwrap();
    std::fs::write(admin.join("gitdir"), format!("{}\n", linked.join(".git").display())).unwrap();

    let places = catalog(Os::Linux, home.path(), &no_env, &[repo.clone()], &SpaceSettings::default());

    let paths: Vec<_> = places.iter().map(|place| place.path.clone()).collect();
    assert_eq!(paths, [repo.join("target"), linked.join("target")]);
}

#[test]
fn catalog_off_and_never_touch_remove_entries() {
    let home = TempDir::new().unwrap();
    let rust = project(home.path(), "rust-app", true);
    write(&home.path().join(".bun/install/cache/a"), 5);
    write(&home.path().join(".npm/_cacache/a"), 5);
    let settings = SpaceSettings {
        catalog_off: vec!["bun".to_owned()],
        never_touch: vec![home.path().join("rust-app")],
        ..SpaceSettings::default()
    };

    let places = catalog(Os::Linux, home.path(), &no_env, &[rust], &settings);

    assert_eq!(ids(&places), [CatalogId::Npm]);
}

#[test]
fn an_own_regenerable_folder_is_added_unless_it_is_never_touched() {
    let home = TempDir::new().unwrap();
    let own = home.path().join("renders");
    write(&own.join("a"), 5);
    let mut settings = SpaceSettings { own_regenerable: vec![own.clone()], ..SpaceSettings::default() };
    let places = catalog(Os::Linux, home.path(), &no_env, &[], &settings);
    assert_eq!(ids(&places), [CatalogId::OwnRegenerable]);

    settings.never_touch = vec![home.path().to_path_buf()];
    assert!(catalog(Os::Linux, home.path(), &no_env, &[], &settings).is_empty());
}

#[test]
fn known_app_caches_come_from_the_system_cache_folder_by_name() {
    let home = TempDir::new().unwrap();
    write(&home.path().join("Library/Caches/ms-playwright/a"), 5);
    write(&home.path().join("Library/Caches/SomeOtherApp/a"), 5);
    write(&home.path().join("Library/Caches/Homebrew/a"), 5);

    let places = catalog(Os::MacOs, home.path(), &no_env, &[], &SpaceSettings::default());

    let mut found = ids(&places);
    found.sort_by_key(|id| id.name());
    assert_eq!(found, [CatalogId::Homebrew, CatalogId::AppCache("ms-playwright")]);
}

#[test]
fn windows_reads_local_app_data_and_takes_only_week_old_temp_files() {
    let home = TempDir::new().unwrap();
    let local = home.path().join("AppData/Local");
    let temp = home.path().join("Temp");
    write(&local.join("pip/Cache/a"), 5);
    write(&temp.join("old.tmp"), 5);
    write(&temp.join("new.tmp"), 5);
    let old = std::fs::File::options().write(true).open(temp.join("old.tmp")).unwrap();
    old.set_modified(SystemTime::now() - Duration::from_secs(8 * 24 * 60 * 60)).unwrap();
    let local_value = local.clone();
    let temp_value = temp.clone();
    let env = move |name: &str| match name {
        "LOCALAPPDATA" => Some(local_value.clone().into_os_string()),
        "TEMP" => Some(temp_value.clone().into_os_string()),
        _ => None,
    };

    let places = catalog(Os::Windows, home.path(), &env, &[], &SpaceSettings::default());

    assert_eq!(ids(&places), [CatalogId::Pip, CatalogId::WinTempOld]);
    assert_eq!(places[1].path, temp.join("old.tmp"));
}
