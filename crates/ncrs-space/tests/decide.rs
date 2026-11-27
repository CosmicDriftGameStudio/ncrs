use std::collections::HashMap;
use std::fs::{self, File, FileTimes};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime};

use ncrs_space::decide::{
    app_name_of_installer, decide_entries, sort_downloads, DecideKind, InstalledApps,
};
use ncrs_space::{EntryKind, Group, SpaceSettings};

struct FakeApps(HashMap<String, PathBuf>);

impl FakeApps {
    fn with(names: &[&str]) -> Self {
        Self(
            names
                .iter()
                .map(|name| (name.to_lowercase(), PathBuf::from(format!("/Applications/{name}.app"))))
                .collect(),
        )
    }
}

impl InstalledApps for FakeApps {
    fn find(&self, name: &str) -> Option<PathBuf> {
        self.0.get(&name.to_lowercase()).cloned()
    }
}

fn write(folder: &Path, name: &str, content: &str) -> PathBuf {
    let path = folder.join(name);
    fs::write(&path, content).unwrap();
    path
}

fn age(path: &Path, by: Duration) {
    let then = SystemTime::now() - by;
    let file = File::options().write(true).open(path).unwrap();
    file.set_times(FileTimes::new().set_accessed(then).set_modified(then)).unwrap();
}

fn names(items: &[ncrs_space::decide::DownloadItem]) -> Vec<String> {
    let mut names: Vec<String> = items
        .iter()
        .map(|item| item.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn an_installer_of_an_installed_app_is_grouped_and_one_of_an_unknown_app_is_not() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "Firefox 126.0.dmg", "installer");
    write(dir.path(), "Strange-Tool-1.0.pkg", "other installer");

    let groups = sort_downloads(dir.path(), &FakeApps::with(&["Firefox"]), &AtomicBool::new(false));

    assert_eq!(groups.installed.len(), 1);
    assert!(groups.installed[0].item.path.ends_with("Firefox 126.0.dmg"));
    assert_eq!(groups.installed[0].with, PathBuf::from("/Applications/Firefox.app"));
    assert_eq!(names(&groups.rest), ["Strange-Tool-1.0.pkg"]);
}

#[test]
fn an_archive_with_its_unpacked_folder_beside_it_is_grouped() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "photos.zip", "zipped");
    fs::create_dir(dir.path().join("photos")).unwrap();
    write(&dir.path().join("photos"), "a.jpg", "image");
    write(dir.path(), "lonely.zip", "zipped alone");

    let groups = sort_downloads(dir.path(), &FakeApps::with(&[]), &AtomicBool::new(false));

    assert_eq!(groups.unpacked.len(), 1);
    assert!(groups.unpacked[0].item.path.ends_with("photos.zip"));
    assert!(groups.unpacked[0].with.ends_with("photos"));
    assert_eq!(names(&groups.rest), ["lonely.zip", "photos"]);
}

#[test]
fn equal_files_are_one_set_and_only_the_copies_are_offered() {
    let dir = tempfile::tempdir().unwrap();
    let first = write(dir.path(), "report.pdf", "same bytes");
    let second = write(dir.path(), "report (1).pdf", "same bytes");
    write(dir.path(), "other.pdf", "same size!");
    age(&first, Duration::from_secs(10 * 24 * 3600));

    let groups = sort_downloads(dir.path(), &FakeApps::with(&[]), &AtomicBool::new(false));

    assert_eq!(groups.duplicates.len(), 1);
    assert!(groups.duplicates[0].keep.path.ends_with("report.pdf"));
    assert_eq!(groups.duplicates[0].extra.len(), 1);
    assert_eq!(groups.duplicates[0].extra[0].path, second);
    assert_eq!(names(&groups.rest), ["other.pdf", "report.pdf"]);
}

#[test]
fn what_nobody_touched_for_a_year_is_its_own_group() {
    let dir = tempfile::tempdir().unwrap();
    let old = write(dir.path(), "old.txt", "ancient");
    write(dir.path(), "new.txt", "fresh");
    age(&old, Duration::from_secs(400 * 24 * 3600));

    let groups = sort_downloads(dir.path(), &FakeApps::with(&[]), &AtomicBool::new(false));

    assert_eq!(names(&groups.untouched_year), ["old.txt"]);
    assert_eq!(names(&groups.rest), ["new.txt"]);
}

#[test]
fn hidden_files_and_unfinished_downloads_are_left_out() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), ".DS_Store", "x");
    write(dir.path(), "movie.mp4.crdownload", "half");
    write(dir.path(), "done.txt", "done");

    let groups = sort_downloads(dir.path(), &FakeApps::with(&[]), &AtomicBool::new(false));

    assert_eq!(names(&groups.rest), ["done.txt"]);
}

#[test]
fn decide_entries_carry_the_group_the_kind_and_the_facts() {
    let home = tempfile::tempdir().unwrap();
    let downloads = home.path().join("Downloads");
    fs::create_dir(&downloads).unwrap();
    write(&downloads, "a.txt", "one");
    let old = write(&downloads, "b.txt", "two two");
    age(&old, Duration::from_secs(500 * 24 * 3600));

    let entries = decide_entries(home.path(), &SpaceSettings::default(), &AtomicBool::new(false));

    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|entry| entry.group == Group::Decide && entry.held_by.is_empty()));
    let kind_of = |name: &str| {
        entries
            .iter()
            .find(|entry| entry.path.ends_with(name))
            .map(|entry| entry.kind.clone())
    };
    assert_eq!(kind_of("a.txt"), Some(EntryKind::Decide(DecideKind::Other)));
    assert_eq!(kind_of("b.txt"), Some(EntryKind::Decide(DecideKind::UntouchedYear)));
    assert!(entries.iter().all(|entry| entry.modified.is_some() && entry.size.bytes > 0));
}

#[test]
fn the_app_name_stops_at_the_version_a_platform_word_or_a_copy_marker() {
    for (file, name) in [
        ("Firefox 126.0.dmg", Some("Firefox")),
        ("Slack-4.38.121-macOS.dmg", Some("Slack")),
        ("Obsidian-1.5.3.AppImage", Some("Obsidian")),
        ("Docker.dmg", Some("Docker")),
        ("1Password-8.10.pkg", Some("1Password")),
        ("Zoom_installer_x64.exe", Some("Zoom")),
        ("Visual Studio Code (1).dmg", Some("Visual Studio Code")),
        ("", None),
    ] {
        assert_eq!(app_name_of_installer(file).as_deref(), name, "{file}");
    }
}
