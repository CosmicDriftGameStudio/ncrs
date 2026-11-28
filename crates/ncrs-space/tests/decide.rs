#[cfg(test)]
mod decide_flow {
    use std::collections::HashMap;
    use std::fs::{self, File, FileTimes};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, SystemTime};

    use ncrs_space::decide::{
        app_name_of_installer, decide_entries, sort_downloads, DecideKind, InstalledApps,
    };
    use ncrs_space::{DecideReason, EntryKind, Group, SpaceSettings, VersionRelation};

    struct FakeApps(HashMap<String, PathBuf>);

    impl FakeApps {
        fn with(names: &[&str]) -> Self {
            Self(
                names
                    .iter()
                    .map(|name| {
                        (
                            name.to_lowercase(),
                            PathBuf::from(format!("/Applications/{name}.app")),
                        )
                    })
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
        file.set_times(FileTimes::new().set_accessed(then).set_modified(then))
            .unwrap();
    }

    fn names(items: &[ncrs_space::decide::DownloadItem]) -> Vec<String> {
        let mut names: Vec<String> = items
            .iter()
            .map(|item| {
                item.path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn an_installer_of_an_installed_app_is_grouped_and_one_of_an_unknown_app_is_not() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Firefox 126.0.dmg", "installer");
        write(dir.path(), "Strange-Tool-1.0.pkg", "other installer");

        let groups = sort_downloads(
            dir.path(),
            &FakeApps::with(&["Firefox"]),
            &AtomicBool::new(false),
        );

        assert_eq!(groups.installed.len(), 1);
        assert!(groups.installed[0].item.path.ends_with("Firefox 126.0.dmg"));
        assert_eq!(
            groups.installed[0].with,
            PathBuf::from("/Applications/Firefox.app")
        );
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

        let entries = decide_entries(
            home.path(),
            &SpaceSettings::default(),
            &AtomicBool::new(false),
        );

        assert_eq!(entries.len(), 2);
        assert!(entries
            .iter()
            .all(|entry| entry.group == Group::Decide && entry.held_by.is_empty()));
        let kind_of = |name: &str| {
            entries
                .iter()
                .find(|entry| entry.path.ends_with(name))
                .map(|entry| entry.kind.clone())
        };
        assert_eq!(kind_of("a.txt"), Some(EntryKind::Decide(DecideKind::Other)));
        assert_eq!(
            kind_of("b.txt"),
            Some(EntryKind::Decide(DecideKind::UntouchedYear))
        );
        assert!(entries
            .iter()
            .all(|entry| entry.modified.is_some() && entry.size.bytes > 0));
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

    struct VersionedApps;

    impl InstalledApps for VersionedApps {
        fn find(&self, name: &str) -> Option<PathBuf> {
            (name == "Xcode").then(|| PathBuf::from("/Applications/Xcode.app"))
        }

        fn version(&self, _app: &Path) -> Option<String> {
            Some("17.0".to_owned())
        }
    }

    #[test]
    fn an_installer_knows_the_app_its_version_and_how_it_compares() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Xcode_17.xip", "installer");
        write(dir.path(), "Xcode_16.2.xip", "older installer");

        let groups = sort_downloads(dir.path(), &VersionedApps, &AtomicBool::new(false));

        assert_eq!(groups.installed.len(), 2);
        let check_of = |name: &str| {
            groups
                .installed
                .iter()
                .find(|matched| matched.item.path.ends_with(name))
                .and_then(|matched| matched.installed.clone())
                .unwrap()
        };
        let same = check_of("Xcode_17.xip");
        assert_eq!(same.path, PathBuf::from("/Applications/Xcode.app"));
        assert_eq!(same.version.as_deref(), Some("17.0"));
        assert_eq!(same.installer_version.as_deref(), Some("17"));
        assert_eq!(same.relation, VersionRelation::Same);
        assert_eq!(
            check_of("Xcode_16.2.xip").relation,
            VersionRelation::InstalledNewer
        );
    }

    #[test]
    fn a_copy_outside_downloads_makes_the_download_redundant() {
        let home = tempfile::tempdir().unwrap();
        let downloads = home.path().join("Downloads");
        let documents = home.path().join("Documents/Taxes/2025");
        fs::create_dir_all(&downloads).unwrap();
        fs::create_dir_all(&documents).unwrap();
        let content = "x".repeat(5000);
        write(&downloads, "invoice.pdf", &content);
        write(&downloads, "other.pdf", &"y".repeat(5000));
        let kept = write(&documents, "invoice-final.pdf", &content);

        let groups = ncrs_space::decide::sort_downloads_with(
            &downloads,
            &FakeApps::with(&[]),
            &AtomicBool::new(false),
            SystemTime::now(),
            &[home.path().join("Documents")],
        );

        assert_eq!(groups.elsewhere.len(), 1);
        assert!(groups.elsewhere[0].item.path.ends_with("invoice.pdf"));
        assert_eq!(groups.elsewhere[0].with, kept);
        assert_eq!(names(&groups.rest), ["other.pdf"]);
    }

    #[test]
    fn a_decide_entry_for_a_copy_elsewhere_names_the_place_of_the_copy() {
        let home = tempfile::tempdir().unwrap();
        let downloads = home.path().join("Downloads");
        fs::create_dir_all(&downloads).unwrap();
        fs::create_dir_all(home.path().join("Documents")).unwrap();
        let content = "z".repeat(6000);
        write(&downloads, "a.bin", &content);
        let copy = write(&home.path().join("Documents"), "a-copy.bin", &content);

        let entries = decide_entries(
            home.path(),
            &SpaceSettings::default(),
            &AtomicBool::new(false),
        );

        let entry = entries
            .iter()
            .find(|entry| entry.path.ends_with("a.bin"))
            .unwrap();
        assert_eq!(entry.kind, EntryKind::Decide(DecideKind::Duplicate));
        assert_eq!(
            entry.facts.reason,
            Some(DecideReason::Duplicate {
                copy,
                in_downloads: false
            })
        );
        assert!(entry.facts.last_used.is_some());
    }

    #[test]
    fn a_duplicate_inside_downloads_names_the_kept_file() {
        let home = tempfile::tempdir().unwrap();
        let downloads = home.path().join("Downloads");
        fs::create_dir_all(&downloads).unwrap();
        let first = write(&downloads, "a.txt", "same content");
        age(&first, Duration::from_secs(3600));
        write(&downloads, "a (1).txt", "same content");

        let entries = decide_entries(
            home.path(),
            &SpaceSettings::default(),
            &AtomicBool::new(false),
        );

        let extra = entries
            .iter()
            .find(|entry| entry.kind == EntryKind::Decide(DecideKind::Duplicate))
            .unwrap();
        assert!(extra.path.ends_with("a (1).txt"));
        assert_eq!(
            extra.facts.reason,
            Some(DecideReason::Duplicate {
                copy: first,
                in_downloads: true
            })
        );
    }

    fn kinds_of(entries: &[ncrs_space::Entry]) -> Vec<(DecideKind, PathBuf)> {
        entries
            .iter()
            .filter_map(|entry| match &entry.kind {
                EntryKind::Decide(kind) => Some((kind.clone(), entry.path.clone())),
                EntryKind::Catalog(_) | EntryKind::Worktree(_) => None,
            })
            .collect()
    }

    fn fill(path: &Path, len: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![b'x'; len]).unwrap();
    }

    #[test]
    fn session_logs_backups_and_models_are_named_kinds_at_their_exact_places() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path();
        fill(&root.join(".claude-cosmic/projects/p/a.jsonl"), 100);
        fill(&root.join(".claude/projects/q/b.jsonl"), 100);
        fill(&root.join("Backups/disk/a"), 100);
        fill(
            &root.join("Library/Application Support/MobileSync/Backup/id/a"),
            100,
        );
        fill(&root.join(".cache/huggingface/hub/m.bin"), 100);
        fill(&root.join(".cache/ncrs-model-test/m.bin"), 100);
        fill(&root.join(".cache/other-tool/x"), 100);
        let settings = SpaceSettings {
            never_touch: Vec::new(),
            ..SpaceSettings::default()
        };

        let entries = decide_entries(root, &settings, &AtomicBool::new(false));
        let found = kinds_of(&entries);

        let of = |kind: DecideKind| -> Vec<PathBuf> {
            found
                .iter()
                .filter(|(kind_found, _)| *kind_found == kind)
                .map(|(_, path)| path.strip_prefix(root).unwrap().to_path_buf())
                .collect()
        };
        let mut logs = of(DecideKind::SessionLogs);
        logs.sort();
        assert_eq!(
            logs,
            [
                PathBuf::from(".claude/projects"),
                PathBuf::from(".claude-cosmic/projects")
            ]
        );
        let mut backups = of(DecideKind::Backups);
        backups.sort();
        assert_eq!(
            backups,
            [
                PathBuf::from("Backups"),
                PathBuf::from("Library/Application Support/MobileSync/Backup")
            ]
        );
        let mut models = of(DecideKind::ModelCache);
        models.sort();
        assert_eq!(
            models,
            [
                PathBuf::from(".cache/huggingface"),
                PathBuf::from(".cache/ncrs-model-test")
            ]
        );
        assert!(found
            .iter()
            .all(|(kind, _)| *kind != DecideKind::BigFolder && *kind != DecideKind::Simulators));
    }

    #[test]
    fn a_never_touch_folder_hides_the_named_kind_below_it() {
        let home = tempfile::tempdir().unwrap();
        fill(&home.path().join("Backups/disk/a"), 100);
        fill(&home.path().join(".claude/projects/q/b.jsonl"), 100);
        let settings = SpaceSettings {
            never_touch: vec![home.path().join("Backups")],
            ..SpaceSettings::default()
        };

        let found = kinds_of(&decide_entries(
            home.path(),
            &settings,
            &AtomicBool::new(false),
        ));

        assert!(found.iter().all(|(kind, _)| *kind != DecideKind::Backups));
        assert!(found
            .iter()
            .any(|(kind, _)| *kind == DecideKind::SessionLogs));
    }

    #[test]
    fn the_simulator_entry_may_not_go_to_the_trash_as_a_whole() {
        assert!(!DecideKind::Simulators.can_trash());
        assert!(DecideKind::Backups.can_trash());
        assert!(DecideKind::SessionLogs.can_trash());
    }
}
