use super::*;
use ncrs_index::TargetId;
use ncrs_intake::{OnNoText, OnSeveralTargets, SortSource, SortTarget};

fn id(text: &str) -> TargetId {
    TargetId::new(text).unwrap()
}

fn config_in(dir: &tempfile::TempDir, text: &str) -> PathBuf {
    let path = dir.path().join("config.toml");
    std::fs::write(&path, text).unwrap();
    path
}

fn paths(base: &tempfile::TempDir) -> (PathBuf, PathBuf, PathBuf) {
    (
        base.path().join("inbox"),
        base.path().join("archive"),
        base.path().join("other"),
    )
}

fn source_at(path: &Path, targets: &[&str]) -> SortSource {
    SortSource {
        path: path.to_path_buf(),
        include: String::new(),
        exclude: String::new(),
        targets: targets.iter().map(|text| id(text)).collect(),
        watch: false,
        transfer: LocationTransfer::Move,
        no_text: OnNoText::Ask,
        several: OnSeveralTargets::Ask,
        own_volume: None,
    }
}

fn names_of(config: &Config) -> (Vec<String>, Vec<PathBuf>) {
    (
        config
            .filing
            .targets
            .iter()
            .map(|setting| setting.target.id.as_str().to_owned())
            .collect(),
        config
            .filing
            .sources
            .iter()
            .map(|setting| setting.source.path.clone())
            .collect(),
    )
}

fn assert_the_sorting_of_the_filing_fixtures(config: &Config) {
    let home = Path::new(FIXTURE_HOME);
    let targets = &config.filing.targets;
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].target.id, TargetId::main());
    assert_eq!(targets[0].target.name, "Archive");
    assert_eq!(targets[0].target.path, home.join("Documents/Archive"));
    assert_eq!(targets[0].paused, None);
    let sources = &config.filing.sources;
    assert_eq!(sources.len(), 1);
    let source = &sources[0].source;
    assert_eq!(source.path, home.join("Inbox"));
    assert_eq!(source.targets, [TargetId::main()]);
    assert_eq!(source.exclude, IMAGE_EXCLUDE);
    assert_eq!(source.include, "");
    assert!(!source.watch);
    assert_eq!(source.transfer, LocationTransfer::Move);
    assert_eq!(source.no_text, OnNoText::Ask);
    assert_eq!(source.several, OnSeveralTargets::Ask);
    assert_eq!(source.own_volume, None);
    assert_eq!(sources[0].paused, None);
}

#[test]
fn the_configs_of_0_5_0_and_0_7_0_give_one_target_and_one_source() {
    for text in [
        include_str!("../../../../../tests/fixtures/config/v0.5.0.toml"),
        include_str!("../../../../../tests/fixtures/config/v0.7.0.toml"),
    ] {
        assert_the_sorting_of_the_filing_fixtures(&load_fixture(text));
    }
}

#[test]
fn the_configs_of_the_1_x_releases_give_the_target_main_and_one_source() {
    for text in [
        include_str!("../../../../../tests/fixtures/config/v1.0.0.toml"),
        include_str!("../../../../../tests/fixtures/config/v1.2.0.toml"),
        include_str!("../../../../../tests/fixtures/config/v1.5.1.toml"),
    ] {
        let filing = load_fixture(text).filing;
        assert_eq!(filing.targets.len(), 1);
        assert_eq!(filing.targets[0].target.id, TargetId::main());
        assert_eq!(filing.sources.len(), 1);
        assert_eq!(filing.sources[0].source.targets, [TargetId::main()]);
        assert!(filing.targets.iter().all(|setting| setting.paused.is_none()));
        assert!(filing.sources.iter().all(|setting| setting.paused.is_none()));
    }
}

#[test]
fn the_old_flags_become_watch_transfer_and_the_image_exclusion() {
    let base = tempfile::tempdir().unwrap();
    let (inbox, archive, other) = paths(&base);
    let text = filing_text(
        true,
        &[
            location(
                &quoted(&inbox),
                "mode = \"inbox\"\nimages = true\ncontinuous = true\ntransfer = \"copy\"",
            ),
            location(&quoted(&other), "mode = \"inbox\"\nimages = false"),
            location(&quoted(&archive), "mode = \"archive\"\ntarget = true"),
        ],
    );

    let filing = load_filing(&text, None).unwrap().filing;

    let first = &filing.sources[0].source;
    assert!(first.watch);
    assert_eq!(first.transfer, LocationTransfer::Copy);
    assert_eq!(first.exclude, "");
    let second = &filing.sources[1].source;
    assert!(!second.watch);
    assert_eq!(second.exclude, IMAGE_EXCLUDE);
    assert_eq!(filing.locations.len(), 3);
}

#[test]
fn an_archive_without_the_target_flag_is_no_target_and_no_source() {
    let base = tempfile::tempdir().unwrap();
    let (_, archive, _) = paths(&base);
    let text = filing_text(true, &[location(&quoted(&archive), "mode = \"archive\"")]);

    let filing = load_filing(&text, None).unwrap().filing;

    assert!(filing.targets.is_empty() && filing.sources.is_empty());
    assert_eq!(filing.locations.len(), 1);
}

#[test]
fn an_inbox_without_a_target_waits_without_one() {
    let base = tempfile::tempdir().unwrap();
    let (inbox, _, _) = paths(&base);
    let text = filing_text(true, &[location(&quoted(&inbox), "mode = \"inbox\"")]);

    let filing = load_filing(&text, None).unwrap().filing;

    assert!(filing.targets.is_empty());
    assert!(filing.sources[0].source.targets.is_empty());
    assert_eq!(filing.sources[0].paused, None);
}

#[test]
fn old_files_with_nested_or_repeated_folders_and_two_targets_still_load() {
    let base = tempfile::tempdir().unwrap();
    let (_, archive, other) = paths(&base);
    let nested = archive.join("inside");
    let text = filing_text(
        true,
        &[
            location(&quoted(&archive), "mode = \"archive\"\ntarget = true"),
            location(&quoted(&other), "mode = \"archive\"\ntarget = true"),
            location(&quoted(&nested), "mode = \"archive\""),
        ],
    );

    let filing = load_filing(&text, None).unwrap().filing;

    let ids: Vec<_> = filing
        .targets
        .iter()
        .map(|setting| setting.target.id.as_str())
        .collect();
    assert_eq!(ids, ["main", "t2"]);
}

#[test]
fn once_sources_or_targets_are_in_the_file_the_old_locations_count_only_as_archives() {
    let base = tempfile::tempdir().unwrap();
    let (inbox, archive, _) = paths(&base);
    for key in ["sources = []", "targets = []"] {
        let text = format!(
            "[modules.filing]\nenabled = true\n{key}\n{}{}",
            location(&quoted(&inbox), "mode = \"inbox\"\ncontinuous = true"),
            location(&quoted(&archive), "mode = \"archive\"\nimages = true\ntarget = true"),
        );

        let filing = load_filing(&text, None).unwrap().filing;

        assert!(filing.sources.is_empty(), "{key}");
        assert!(filing.targets.is_empty(), "{key}");
        let kept: Vec<_> = filing
            .locations
            .iter()
            .map(|found| (found.path.clone(), found.mode, found.images, found.is_target))
            .collect();
        assert_eq!(kept, [(archive.clone(), LocationMode::Archive, true, false)], "{key}");
    }
}

fn new_form(base: &tempfile::TempDir) -> String {
    let (inbox, archive, other) = paths(base);
    format!(
        "[modules.filing]\nenabled = true\n\n\
         [[modules.filing.targets]]\nid = \"main\"\nname = \"Documents\"\npath = {}\nown_volume = true\n\n\
         [[modules.filing.targets]]\nid = \"t2\"\npath = {}\n\n\
         [[modules.filing.sources]]\npath = {}\ninclude = \"*.pdf\"\nexclude = \"private/\"\n\
         targets = [\"main\", \"t2\"]\nwhen = \"watch\"\nfiles = \"copy\"\nno_text = \"skip\"\nseveral = \"copy_to_all\"\nown_volume = true\n",
        quoted(&archive),
        quoted(&other),
        quoted(&inbox),
    )
}

#[test]
fn the_new_form_is_read_with_every_value() {
    let base = tempfile::tempdir().unwrap();
    let (inbox, archive, other) = paths(&base);

    let filing = load_filing(&new_form(&base), None).unwrap().filing;

    assert_eq!(
        filing.targets,
        [
            TargetSetting {
                target: SortTarget {
                    id: id("main"),
                    name: "Documents".to_owned(),
                    path: archive,
                    own_volume: Some(true),
                },
                paused: None,
            },
            TargetSetting {
                target: SortTarget {
                    id: id("t2"),
                    name: "other".to_owned(),
                    path: other,
                    own_volume: None,
                },
                paused: None,
            },
        ]
    );
    assert_eq!(
        filing.sources,
        [SourceSetting {
            source: SortSource {
                path: inbox,
                include: "*.pdf".to_owned(),
                exclude: "private/".to_owned(),
                targets: vec![id("main"), id("t2")],
                watch: true,
                transfer: LocationTransfer::Copy,
                no_text: OnNoText::Skip,
                several: OnSeveralTargets::CopyToAll,
                own_volume: Some(true),
            },
            paused: None,
        }]
    );
}

#[test]
fn an_unknown_target_pauses_only_the_source() {
    let base = tempfile::tempdir().unwrap();
    let text = new_form(&base).replace("[\"main\", \"t2\"]", "[\"main\", \"t9\"]");

    let filing = load_filing(&text, None).unwrap().filing;

    assert_eq!(
        filing.sources[0].paused,
        Some(PauseReason::UnknownTarget("t9".to_owned()))
    );
    assert_eq!(filing.sources[0].source.targets, [id("main")]);
    assert!(filing.targets.iter().all(|setting| setting.paused.is_none()));
}

#[test]
fn a_glob_that_does_not_compile_pauses_only_the_source() {
    let base = tempfile::tempdir().unwrap();
    let text = new_form(&base).replace("include = \"*.pdf\"", "include = \"[a\"");

    let filing = load_filing(&text, None).unwrap().filing;

    assert!(matches!(
        filing.sources[0].paused,
        Some(PauseReason::InvalidGlob(_))
    ));
    assert_eq!(filing.targets.len(), 2);
}

#[test]
fn an_unknown_word_pauses_only_the_source() {
    let base = tempfile::tempdir().unwrap();
    let text = new_form(&base).replace("when = \"watch\"", "when = \"daily\"");

    let filing = load_filing(&text, None).unwrap().filing;

    assert_eq!(
        filing.sources[0].paused,
        Some(PauseReason::UnknownValue("when = \"daily\"".to_owned()))
    );
}

#[test]
fn a_repeated_id_or_folder_pauses_the_later_target_and_source() {
    let base = tempfile::tempdir().unwrap();
    let (inbox, archive, other) = paths(&base);
    let text = format!(
        "[modules.filing]\n\
         [[modules.filing.targets]]\nid = \"main\"\npath = {a}\n\
         [[modules.filing.targets]]\nid = \"main\"\npath = {o}\n\
         [[modules.filing.targets]]\nid = \"t2\"\npath = {a}\n\
         [[modules.filing.sources]]\npath = {i}\ntargets = [\"main\"]\n\
         [[modules.filing.sources]]\npath = {i}\ntargets = [\"main\"]\n",
        a = quoted(&archive),
        o = quoted(&other),
        i = quoted(&inbox),
    );

    let filing = load_filing(&text, None).unwrap().filing;

    let paused: Vec<_> = filing.targets.iter().map(|setting| setting.paused.clone()).collect();
    assert_eq!(
        paused,
        [None, Some(PauseReason::DuplicateId), Some(PauseReason::SamePath)]
    );
    assert_eq!(filing.sources[0].paused, None);
    assert_eq!(filing.sources[1].paused, Some(PauseReason::SamePath));
}

#[test]
fn only_a_watched_source_in_a_target_is_refused() {
    let base = tempfile::tempdir().unwrap();
    let (_, archive, other) = paths(&base);
    let inside = archive.join("inbox");
    let inside_other = other.join("deep");
    let text = format!(
        "[modules.filing]\n\
         [[modules.filing.targets]]\nid = \"main\"\npath = {a}\n\
         [[modules.filing.targets]]\nid = \"t2\"\npath = {inside_other}\n\
         [[modules.filing.sources]]\npath = {inside}\ntargets = [\"main\"]\nwhen = \"watch\"\n\
         [[modules.filing.sources]]\npath = {a}\ntargets = [\"t2\"]\nwhen = \"once\"\n\
         [[modules.filing.sources]]\npath = {o}\ntargets = [\"main\"]\nwhen = \"watch\"\n",
        a = quoted(&archive),
        o = quoted(&other),
        inside = quoted(&inside),
        inside_other = quoted(&inside_other),
    );

    let filing = load_filing(&text, None).unwrap().filing;

    let paused: Vec<_> = filing.sources.iter().map(|setting| setting.paused.clone()).collect();
    // A target as a once source and a watched source that holds a target are fine.
    assert_eq!(paused, [Some(PauseReason::WatchedInTarget(id("main"))), None, None]);
    assert!(filing.targets.iter().all(|setting| setting.paused.is_none()));
}

#[cfg(windows)]
#[test]
fn a_target_written_with_the_verbatim_prefix_loads_without_it() {
    let text = "[modules.filing]\n[[modules.filing.targets]]\nid = \"main\"\npath = '\\\\?\\C:\\x'\n";

    let filing = load_filing(text, None).unwrap().filing;

    assert_eq!(filing.targets[0].target.path, PathBuf::from(r"C:\x"));
}

const LEGACY: &str = "# mine\n[hermes]\nenabled = true\n";

fn legacy_config(base: &tempfile::TempDir) -> String {
    let (inbox, archive, other) = paths(base);
    format!(
        "{LEGACY}\n[modules.filing]\nenabled = true\n\n{}{}{}",
        location(
            &quoted(&inbox),
            "mode = \"inbox\"\ncontinuous = true\ntransfer = \"copy\""
        ),
        location(&quoted(&archive), "mode = \"archive\"\nimages = true\ntarget = true"),
        location(&quoted(&other), "mode = \"archive\"\nimages = true"),
    )
}

fn backup_of(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(BACKUP_SUFFIX);
    PathBuf::from(name)
}

#[test]
fn the_first_write_keeps_a_copy_and_moves_the_old_folders_into_the_new_lists() {
    let base = tempfile::tempdir().unwrap();
    let (inbox, archive, other) = paths(&base);
    let original = legacy_config(&base);
    let path = config_in(&base, &original);

    let source = source_at(&inbox, &["main"]);
    let written = upsert_sort_source_with_home(&path, &source, None, None).unwrap();

    assert_eq!(std::fs::read_to_string(backup_of(&path)).unwrap(), original);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# mine"), "{text}");
    assert!(!text.contains("continuous") && !text.contains("target = true"), "{text}");
    assert_eq!(names_of(&written), (vec!["main".to_owned()], vec![inbox]));
    assert_eq!(written.filing.targets[0].target.path, archive);
    let kept: Vec<_> = written.filing.locations.iter().map(|l| l.path.clone()).collect();
    assert!(kept.contains(&archive) && kept.contains(&other));
    assert_eq!(load_with_home(&path, None).unwrap(), written);
}

#[test]
fn the_copy_is_made_once_and_never_overwritten() {
    let base = tempfile::tempdir().unwrap();
    let (inbox, _, other) = paths(&base);
    let original = legacy_config(&base);
    let path = config_in(&base, &original);

    upsert_sort_source_with_home(&path, &source_at(&inbox, &["main"]), None, None).unwrap();
    upsert_sort_source_with_home(&path, &source_at(&other, &["main"]), None, None).unwrap();

    assert_eq!(std::fs::read_to_string(backup_of(&path)).unwrap(), original);

    let second = tempfile::tempdir().unwrap();
    let path = config_in(&second, &legacy_config(&second));
    std::fs::write(backup_of(&path), "earlier copy").unwrap();
    upsert_sort_source_with_home(&path, &source_at(&paths(&second).0, &[]), None, None).unwrap();
    assert_eq!(std::fs::read_to_string(backup_of(&path)).unwrap(), "earlier copy");
}

#[cfg(unix)]
#[test]
fn the_copy_has_the_permissions_of_the_config() {
    use std::os::unix::fs::PermissionsExt;
    let base = tempfile::tempdir().unwrap();
    let path = config_in(&base, &legacy_config(&base));
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

    upsert_sort_source_with_home(&path, &source_at(&paths(&base).0, &[]), None, None).unwrap();

    let mode = |file: &Path| std::fs::metadata(file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&backup_of(&path)), 0o600);
    assert_eq!(mode(&path), 0o600);
}

#[test]
fn a_removed_source_does_not_come_back_from_the_old_locations() {
    let base = tempfile::tempdir().unwrap();
    let (inbox, _, _) = paths(&base);
    let path = config_in(&base, &legacy_config(&base));

    let removed = remove_sort_source_with_home(&path, &inbox, None).unwrap();

    assert!(removed.filing.sources.is_empty());
    assert!(removed.filing.locations.iter().all(|l| l.mode == LocationMode::Archive));
    let reloaded = load_with_home(&path, None).unwrap();
    assert!(reloaded.filing.sources.is_empty());
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("sources = []"), "{text}");
}

#[test]
fn a_changed_source_is_replaced_at_its_old_folder_and_applies_when_loaded_again() {
    let base = tempfile::tempdir().unwrap();
    let (inbox, _, other) = paths(&base);
    let path = config_in(&base, &new_form(&base));

    let moved = SortSource {
        include: "*.png".to_owned(),
        ..source_at(&other, &["t2"])
    };
    upsert_sort_source_with_home(&path, &moved, Some(&inbox), None).unwrap();

    let filing = load(&path).unwrap().filing;
    assert_eq!(filing.sources.len(), 1);
    assert_eq!(filing.sources[0].source, moved);
}

#[test]
fn target_ids_are_never_handed_out_twice() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    let folder = |name: &str| base.path().join(name);
    let mut handed_out = Vec::new();
    let mut add = |name: &str| {
        let (new_id, _) =
            add_sort_target_with_home(&path, name, &folder(name), None, None).unwrap();
        handed_out.push(new_id.as_str().to_owned());
        new_id
    };

    let main = add("a");
    let second = add("b");
    remove_sort_target_with_home(&path, &second, None).unwrap();
    add("c");
    remove_sort_target_with_home(&path, &main, None).unwrap();
    add("d");

    assert_eq!(handed_out, ["main", "t2", "t3", "t4"]);
}

#[test]
fn a_new_config_gets_main_for_the_first_target() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");

    let (first, config) =
        add_sort_target_with_home(&path, "Docs", &base.path().join("docs"), Some(false), None)
            .unwrap();

    assert_eq!(first, TargetId::main());
    assert_eq!(config.filing.targets[0].target.own_volume, Some(false));
    assert!(!backup_of(&path).exists());
}

#[test]
fn an_old_config_with_a_target_flag_hands_out_t2_after_main() {
    let base = tempfile::tempdir().unwrap();
    let path = config_in(&base, &legacy_config(&base));

    let (next, config) =
        add_sort_target_with_home(&path, "More", &base.path().join("more"), None, None).unwrap();

    assert_eq!(next.as_str(), "t2");
    assert_eq!(config.filing.targets.len(), 2);
}

#[test]
fn what_is_written_is_what_is_read() {
    let base = tempfile::tempdir().unwrap();
    let (inbox, archive, other) = paths(&base);
    let path = config_in(&base, "version = 1\n");
    let target = SortTarget {
        id: id("main"),
        name: "Docs".to_owned(),
        path: archive,
        own_volume: Some(true),
    };
    let source = SortSource {
        include: "*.pdf;*.docx".to_owned(),
        exclude: "private/".to_owned(),
        watch: true,
        transfer: LocationTransfer::Copy,
        no_text: OnNoText::Skip,
        several: OnSeveralTargets::CopyToAll,
        own_volume: Some(true),
        ..source_at(&inbox, &["main"])
    };
    upsert_sort_target_with_home(&path, &target, None).unwrap();
    upsert_sort_source_with_home(&path, &source, None, None).unwrap();
    let plain = source_at(&other, &["main"]);

    let written = upsert_sort_source_with_home(&path, &plain, None, None).unwrap();

    let reloaded = load_with_home(&path, None).unwrap();
    assert_eq!(reloaded, written);
    let sources: Vec<_> = reloaded.filing.sources.iter().map(|s| s.source.clone()).collect();
    assert_eq!(sources, [source, plain]);
    assert_eq!(reloaded.filing.targets[0].target, target);
    assert!(reloaded.filing.sources.iter().all(|s| s.paused.is_none()));
}

#[test]
fn removing_a_target_leaves_its_sources_paused_not_rewritten() {
    let base = tempfile::tempdir().unwrap();
    let path = config_in(&base, &new_form(&base));

    let config = remove_sort_target_with_home(&path, &id("t2"), None).unwrap();

    assert_eq!(config.filing.targets.len(), 1);
    assert_eq!(
        config.filing.sources[0].paused,
        Some(PauseReason::UnknownTarget("t2".to_owned()))
    );
}

#[test]
fn a_removed_target_is_no_read_folder_after_the_read_folders_were_written() {
    let base = tempfile::tempdir().unwrap();
    let (_, archive, other) = paths(&base);
    let path = config_in(&base, &new_form(&base));
    let read_only = base.path().join("read");
    let mut folders = load(&path).unwrap().filing.locations;
    assert!(folders.iter().any(|found| found.path == other));
    folders.push(FilingLocation {
        path: read_only.clone(),
        mode: LocationMode::Archive,
        images: false,
        is_target: false,
        transfer: LocationTransfer::default(),
        continuous: false,
    });

    let written = write_read_folders_with_home(&path, &folders, None).unwrap();
    assert_eq!(written.filing.targets.len(), 2);
    let config = remove_sort_target_with_home(&path, &id("t2"), None).unwrap();

    let kept: Vec<_> = config
        .filing
        .locations
        .iter()
        .filter(|found| found.mode == LocationMode::Archive)
        .map(|found| found.path.clone())
        .collect();
    assert_eq!(kept, [read_only, archive]);
}

#[test]
fn a_folder_the_file_lists_stays_a_read_folder_when_its_target_is_removed() {
    let base = tempfile::tempdir().unwrap();
    let (_, _, other) = paths(&base);
    let text = format!(
        "{}\n{}",
        new_form(&base),
        location(&quoted(&other), "mode = \"archive\"\nimages = true"),
    );
    let path = config_in(&base, &text);
    let folders = load(&path).unwrap().filing.locations;

    write_read_folders_with_home(&path, &folders, None).unwrap();
    let config = remove_sort_target_with_home(&path, &id("t2"), None).unwrap();

    assert!(config.filing.locations.iter().any(|found| found.path == other));
}
