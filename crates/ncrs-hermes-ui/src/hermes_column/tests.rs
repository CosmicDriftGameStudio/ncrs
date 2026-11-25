fn p(path: &str) -> PathBuf {
    PathBuf::from(path.replace('/', std::path::MAIN_SEPARATOR_STR))
}

use ncrs_config::config::{SourceSetting, TargetSetting};
use ncrs_filing::column_reading::{read_column, FolderReading};
use ncrs_index::{Index, ReviewReason, StatusCounts, TargetId};
use ncrs_intake::{LocationTransfer, OnNoText, OnSeveralTargets, SortSource, SortTarget};
use ncrs_pipeline::{
    Busy, ChangeOutcome, ChangedPath, ErrorEntry, WorkerSnapshot, MAX_CHANGED_PATHS,
};
use std::time::UNIX_EPOCH;

use super::*;

fn source(path: &str) -> SourceSetting {
    SourceSetting {
        source: SortSource {
            path: p(path),
            include: String::new(),
            exclude: String::new(),
            targets: Vec::new(),
            watch: true,
            transfer: LocationTransfer::default(),
            no_text: OnNoText::default(),
            several: OnSeveralTargets::default(),
            own_volume: None,
        },
        paused: None,
    }
}

fn target(path: &str) -> TargetSetting {
    TargetSetting {
        target: SortTarget {
            id: TargetId::main(),
            name: "Docs".to_owned(),
            path: p(path),
            own_volume: None,
        },
        paused: None,
    }
}

fn states(directory: &str, files: &[(&str, DocumentStatus)]) -> DirectoryStates {
    DirectoryStates {
        files: files
            .iter()
            .map(|(name, status)| {
                let path = p(directory).join(name);
                (
                    path.clone(),
                    PathState {
                        path,
                        status: *status,
                        review: None,
                        tags: FileTags::default(),
                    },
                )
            })
            .collect(),
        ..DirectoryStates::default()
    }
}

fn listing(directory: &str, files: &[(&str, DocumentStatus)]) -> ColumnReading {
    ColumnReading {
        generation: 1,
        epoch: 0,
        directory: p(directory),
        stage: ColumnStage::Listing,
        states: Some(states(directory, files)),
        folders: Vec::new(),
    }
}

fn counted(path: &str) -> FolderReading {
    let counts = StatusCounts {
        new: 1,
        ..StatusCounts::default()
    };
    FolderReading {
        path: p(path),
        excluded: false,
        direct: Some(counts),
        deep: Some(counts),
    }
}

fn snapshot_with(changes: &[(u64, &str)], latest: u64, floor: u64) -> Snapshot {
    Snapshot {
        changed_paths: changes
            .iter()
            .map(|(sequence, path)| ChangedPath {
                sequence: *sequence,
                path: p(path),
                outcome: ChangeOutcome::Finished,
            })
            .collect(),
        changed_sequence: latest,
        changed_sequence_floor: floor,
        ..Snapshot::off()
    }
}

fn loaded() -> HermesColumn {
    let mut column = HermesColumn::default();
    for directory in ["/a", "/b"] {
        column.apply(listing(directory, &[("x.pdf", DocumentStatus::New)]));
    }
    column.apply(ColumnReading {
        folders: vec![counted("/a"), counted("/a/sub"), counted("/b")],
        ..listing("/", &[])
    });
    column
}

fn directories(names: &[&str]) -> Vec<PathBuf> {
    names.iter().map(|name| p(name)).collect()
}

fn column_with_inquiry() -> HermesColumn {
    let path = p("/in/a.pdf");
    let mut column = HermesColumn::default();
    column.apply(ColumnReading {
        states: Some(DirectoryStates::sample(
            vec![PathState {
                path: path.clone(),
                status: DocumentStatus::InReview,
                review: Some(ReviewReason::NoMatchingRule),
                tags: FileTags::default(),
            }],
            Vec::new(),
        )),
        ..listing("/in", &[])
    });
    column
}

#[test]
fn the_status_line_of_an_open_question_says_that_it_waits_and_why() {
    let column = column_with_inquiry();

    let line = column
        .status_line(&p("/in/a.pdf"), Language::English)
        .unwrap();

    assert!(line.starts_with("Waiting for you: "), "{line}");
    assert!(column
        .status_line(&p("/in/none.pdf"), Language::English)
        .is_none());
}

#[test]
fn a_filed_file_is_called_sorted_in() {
    let mut column = HermesColumn::default();
    column.apply(listing("/d", &[("a.pdf", DocumentStatus::Filed)]));
    assert_eq!(
        column
            .status_line(&p("/d/a.pdf"), Language::German)
            .as_deref(),
        Some("Eingeordnet")
    );
}

#[test]
fn an_error_line_carries_the_shortened_message() {
    let mut column = column_with_inquiry();
    let long = "x".repeat(300);
    column.mark_for_test(
        Vec::new(),
        vec![(p("/in/a.pdf"), format!("{long}\nsecond line"))],
    );

    let line = column
        .status_line(&p("/in/a.pdf"), Language::English)
        .unwrap();

    assert!(line.starts_with("Error: xxx"), "{line}");
    assert!(line.chars().count() < 100, "{line}");
    assert!(!line.contains("second line"), "{line}");
}

#[test]
fn sources_and_targets_have_a_role_at_their_exact_path_and_the_target_wins() {
    let filing = Filing {
        sources: vec![source("/in"), source("/both")],
        targets: vec![target("/target"), target("/both")],
        ..Filing::default()
    };
    assert_eq!(role_of(&p("/in"), &filing), Some(FolderRole::Inbox));
    assert_eq!(
        role_of(&p("/target"), &filing),
        Some(FolderRole::FilingLocation)
    );
    assert_eq!(
        role_of(&p("/both"), &filing),
        Some(FolderRole::FilingLocation)
    );
    assert_eq!(role_of(&p("/target/2024"), &filing), None);
    assert_eq!(role_of(&p("/other"), &filing), None);
}

#[test]
fn a_folder_hermes_only_reads_has_no_role() {
    let filing = Filing {
        locations: vec![ncrs_config::config::FilingLocation {
            path: p("/old"),
            mode: ncrs_intake::LocationMode::Archive,
            images: false,
            is_target: false,
            transfer: LocationTransfer::default(),
            continuous: false,
        }],
        ..Filing::default()
    };
    assert_eq!(role_of(&p("/old"), &filing), None);
}

#[test]
fn the_index_status_maps_to_a_state() {
    let mut column = HermesColumn::default();
    column.apply(listing(
        "/d",
        &[
            ("new.pdf", DocumentStatus::New),
            ("planned.pdf", DocumentStatus::Planned),
            ("review.pdf", DocumentStatus::InReview),
            ("filed.pdf", DocumentStatus::Filed),
        ],
    ));
    let state = |name: &str| column.state_of(&p("/d").join(name));
    assert_eq!(state("new.pdf"), Some(HermesFileState::Unread));
    assert_eq!(state("planned.pdf"), Some(HermesFileState::Planned));
    assert_eq!(state("review.pdf"), Some(HermesFileState::Inquiry));
    assert_eq!(state("filed.pdf"), Some(HermesFileState::Filed));
    assert_eq!(state("unknown.pdf"), None);
    assert_eq!(column.state_of(&p("/elsewhere/a.pdf")), None);
}

#[test]
fn excluded_beats_reading_beats_error_beats_the_index_status() {
    let mut column = HermesColumn::default();
    let mut reading = listing(
        "/d",
        &[
            ("a.pdf", DocumentStatus::Filed),
            ("b.pdf", DocumentStatus::Filed),
            ("c.pdf", DocumentStatus::Filed),
        ],
    );
    reading.states.as_mut().unwrap().excluded_below = directories(&["/d/a.pdf"]);
    column.apply(reading);
    let mut snapshot = Snapshot::off();
    let on = |file: &str| WorkerSnapshot {
        id: 0,
        busy: Some(Busy {
            file: p(file),
            task: Task::Step("text"),
            started_at: UNIX_EPOCH,
        }),
    };
    snapshot.workers = vec![on("/d/a.pdf"), on("/d/b.pdf")];
    snapshot.errors = ["/d/a.pdf", "/d/b.pdf", "/d/c.pdf"]
        .iter()
        .map(|file| ErrorEntry {
            file: p(file),
            task: Task::Step("text"),
            message: String::new(),
            at: UNIX_EPOCH,
            sequence: 0,
        })
        .collect();

    column.set_snapshot(&snapshot);

    let state = |name: &str| column.state_of(&p("/d").join(name));
    assert_eq!(state("a.pdf"), Some(HermesFileState::Excluded));
    assert_eq!(state("b.pdf"), Some(HermesFileState::Reading));
    assert_eq!(state("c.pdf"), Some(HermesFileState::Error));
}

#[test]
fn an_error_ends_when_a_later_change_of_the_file_did_not_fail() {
    let mut column = HermesColumn::default();
    column.apply(listing("/d", &[("a.pdf", DocumentStatus::Filed)]));
    let failure = |sequence| ChangedPath {
        sequence,
        path: p("/d/a.pdf"),
        outcome: ChangeOutcome::Failed,
    };
    let mut snapshot = Snapshot::off();
    snapshot.changed_paths = vec![failure(1)];
    snapshot.changed_sequence = 1;
    snapshot.errors = vec![ErrorEntry {
        file: p("/d/a.pdf"),
        task: Task::Step("text"),
        message: "broken".to_owned(),
        at: UNIX_EPOCH,
        sequence: 1,
    }];
    column.set_snapshot(&snapshot);
    assert_eq!(
        column.state_of(&p("/d/a.pdf")),
        Some(HermesFileState::Error)
    );

    snapshot.changed_paths.push(ChangedPath {
        outcome: ChangeOutcome::Finished,
        ..failure(2)
    });
    snapshot.changed_sequence = 2;
    column.set_snapshot(&snapshot);

    assert_eq!(
        column.state_of(&p("/d/a.pdf")),
        Some(HermesFileState::Filed)
    );
}

#[test]
fn a_file_in_an_excluded_folder_is_excluded() {
    let mut column = HermesColumn::default();
    let mut reading = listing("/d/private", &[("a.pdf", DocumentStatus::New)]);
    reading.states.as_mut().unwrap().excluded_itself = true;
    column.apply(reading);
    assert_eq!(
        column.state_of(&p("/d/private/a.pdf")),
        Some(HermesFileState::Excluded)
    );
}

#[test]
fn a_scan_task_is_not_a_file_being_read() {
    let mut column = HermesColumn::default();
    let mut snapshot = Snapshot::off();
    snapshot.workers = vec![WorkerSnapshot {
        id: 0,
        busy: Some(Busy {
            file: p("/d/a.pdf"),
            task: Task::Scan,
            started_at: UNIX_EPOCH,
        }),
    }];
    column.set_snapshot(&snapshot);
    assert_eq!(column.state_of(&p("/d/a.pdf")), None);
}

#[test]
fn nothing_is_wanted_once_read_and_a_changed_file_makes_its_folder_and_ancestors_stale() {
    let mut column = loaded();
    let listed = directories(&["/a", "/b"]);
    let folders = directories(&["/a", "/a/sub", "/b"]);
    assert!(column.wanted_listings(&listed).is_empty());
    assert!(column
        .wanted_counts(&folders, ColumnStage::Listing)
        .is_empty());
    assert!(column.wanted_counts(&folders, ColumnStage::Deep).is_empty());

    column.consume_changes(&snapshot_with(&[(1, "/a/sub/x.pdf")], 1, 0));

    assert_eq!(column.wanted_listings(&listed), Vec::<PathBuf>::new());
    assert_eq!(
        column.wanted_counts(&folders, ColumnStage::Listing),
        directories(&["/a", "/a/sub"])
    );
    assert_eq!(
        column.wanted_counts(&folders, ColumnStage::Deep),
        directories(&["/a", "/a/sub"])
    );
    let stale_but_shown = column.summary_of(&p("/a")).unwrap();
    assert!(stale_but_shown.deep.is_some());
}

#[test]
fn a_changed_file_makes_only_the_listing_of_its_own_folder_stale() {
    let mut column = loaded();
    let listed = directories(&["/a", "/b"]);

    column.consume_changes(&snapshot_with(&[(1, "/a/x.pdf")], 1, 0));

    assert_eq!(column.wanted_listings(&listed), directories(&["/a"]));
    assert_eq!(
        column.state_of(&p("/a/x.pdf")),
        Some(HermesFileState::Unread)
    );
}

#[test]
fn only_changes_after_the_last_seen_sequence_count() {
    let mut column = loaded();
    column.consume_changes(&snapshot_with(&[(1, "/a/x.pdf")], 1, 0));
    let mut reading = listing("/a", &[("x.pdf", DocumentStatus::Filed)]);
    reading.epoch = column.epoch();
    assert!(column.apply(reading));

    column.consume_changes(&snapshot_with(&[(1, "/a/x.pdf"), (2, "/b/x.pdf")], 2, 0));

    assert_eq!(
        column.wanted_listings(&directories(&["/a", "/b"])),
        directories(&["/b"])
    );
}

#[test]
fn missed_changes_and_a_restarted_runner_drop_everything() {
    let mut column = loaded();
    column.consume_changes(&snapshot_with(&[], 3, 0));
    column.apply(listing("/a", &[]));
    let overflow = snapshot_with(
        &[(MAX_CHANGED_PATHS as u64 + 4, "/z/q.pdf")],
        MAX_CHANGED_PATHS as u64 + 4,
        4,
    );

    column.consume_changes(&overflow);

    assert_eq!(
        column.wanted_listings(&directories(&["/a", "/b"])),
        directories(&["/a", "/b"])
    );
    assert!(column.summary_of(&p("/a")).is_none());

    column.apply(listing("/a", &[]));
    column.consume_changes(&Snapshot::off());
    assert_eq!(
        column.wanted_listings(&directories(&["/a"])),
        directories(&["/a"])
    );
}

#[test]
fn invalidating_a_path_by_hand_reaches_a_folder_itself_and_its_ancestors() {
    let mut column = loaded();
    column.invalidate_path(&p("/a/sub"));
    assert_eq!(
        column.wanted_counts(&directories(&["/a", "/a/sub", "/b"]), ColumnStage::Listing),
        directories(&["/a", "/a/sub"])
    );
}

#[test]
fn a_reading_survives_a_change_elsewhere_and_not_one_in_its_own_directory() {
    let mut column = loaded();
    let asked_at = column.epoch();
    column.invalidate_path(&p("/other/x.pdf"));
    let mut elsewhere = listing("/d", &[("x.pdf", DocumentStatus::Filed)]);
    elsewhere.epoch = asked_at;
    assert!(column.apply(elsewhere));

    column.invalidate_path(&p("/d/x.pdf"));
    let mut inside = listing("/d", &[("x.pdf", DocumentStatus::New)]);
    inside.epoch = asked_at;
    assert!(!column.apply(inside));

    column.invalidate_all();
    let mut everywhere = listing("/d", &[]);
    everywhere.epoch = asked_at;
    assert!(!column.apply(everywhere));
}

#[test]
fn errors_under_a_folder_are_counted_per_folder_and_not_per_prefix() {
    let mut column = loaded();
    column.apply(ColumnReading {
        folders: vec![counted("/a/s")],
        ..listing("/a", &[])
    });
    let mut snapshot = Snapshot::off();
    snapshot.errors = ["/a/sub/x.pdf", "/a/y.pdf"]
        .iter()
        .map(|file| ErrorEntry {
            file: p(file),
            task: Task::Step("text"),
            message: String::new(),
            at: UNIX_EPOCH,
            sequence: 0,
        })
        .collect();

    column.set_snapshot(&snapshot);

    assert_eq!(column.summary_of(&p("/a")).unwrap().errors, 2);
    assert_eq!(column.summary_of(&p("/a/sub")).unwrap().errors, 1);
    assert_eq!(column.summary_of(&p("/a/s")).unwrap().errors, 0);
}

fn index_with_documents() -> (tempfile::TempDir, Index) {
    let dir = tempfile::TempDir::new().unwrap();
    let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
    for (seed, path) in [
        (1, "/d/Scan/a.pdf"),
        (2, "/d/Scan/sub/b.pdf"),
        (3, "/d/Scans/c.pdf"),
    ] {
        index
            .insert_document(&ncrs_index::NewDocument {
                hash: ncrs_index::ContentHash([seed; 32]),
                path: p(path),
                source: ncrs_index::DocumentSource::Inbox,
                received_at: chrono::DateTime::UNIX_EPOCH,
                stamp: None,
            })
            .unwrap();
    }
    (dir, index)
}

#[test]
fn a_reading_fills_the_cache_in_two_stages() {
    let (_dir, index) = index_with_documents();
    let request = |stage| ColumnRequest {
        generation: 7,
        epoch: 0,
        directory: p("/d/Scan"),
        subfolders: directories(&["/d/Scan/sub"]),
        stage,
    };
    let mut column = HermesColumn::default();

    let first = read_column(&index, &request(ColumnStage::Listing)).unwrap();
    assert_eq!(first.generation, 7);
    column.apply(first);

    assert_eq!(
        column.state_of(&p("/d/Scan/a.pdf")),
        Some(HermesFileState::Unread)
    );
    let sub = column.summary_of(&p("/d/Scan/sub")).unwrap();
    assert_eq!(sub.direct.unwrap().new, 1);
    assert_eq!(sub.deep, None);
    assert!(!sub.excluded);
    assert_eq!(
        column.wanted_counts(&directories(&["/d/Scan/sub"]), ColumnStage::Deep),
        directories(&["/d/Scan/sub"])
    );

    column.apply(read_column(&index, &request(ColumnStage::Deep)).unwrap());

    let counted_deep = column.summary_of(&p("/d/Scan/sub")).unwrap();
    assert_eq!(counted_deep.deep.unwrap().new, 1);
    assert_eq!(counted_deep.direct.unwrap().new, 1);
}

#[test]
fn an_excluded_folder_is_marked_and_holds_nothing_any_more() {
    let (_dir, index) = index_with_documents();
    index.exclude_path(&p("/d/Scan/sub")).unwrap();
    let mut column = HermesColumn::default();
    column.apply(
        read_column(
            &index,
            &ColumnRequest {
                generation: 7,
                epoch: 0,
                directory: p("/d/Scan"),
                subfolders: directories(&["/d/Scan/sub"]),
                stage: ColumnStage::Listing,
            },
        )
        .unwrap(),
    );

    let sub = column.summary_of(&p("/d/Scan/sub")).unwrap();
    assert!(sub.excluded);
    assert_eq!(sub.direct.unwrap().new, 0);
    assert_eq!(
        column.state_of(&p("/d/Scan/a.pdf")),
        Some(HermesFileState::Unread)
    );
}
