#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::Receiver;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use chrono::{DateTime, Utc};
    use ncrs_index::{ContentHash, DocumentSource, DocumentStatus, InMemoryKeyStore, Index};
    use ncrs_intake::{
        scan_location, take_in, Location, LocationMode, LocationWatcher, Outcome, ScanReport, Skip,
        StabilityTracker, WatchEvent, STABLE_AFTER,
    };
    use sha2::{Digest as _, Sha256};
    use tempfile::TempDir;
    use walkdir::WalkDir;

    const INVOICE: &[u8] = b"Invoice 4711 from Company A";
    const REMINDER: &[u8] = b"Reminder 12 from Agency B";

    fn t0() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_800_000_000)
    }

    struct Fixture {
        _index_dir: TempDir,
        index: Index,
        tracker: StabilityTracker,
    }

    impl Fixture {
        fn new() -> Self {
            let index_dir = TempDir::new().unwrap();
            let (index, _recovery_key) = Index::create_encrypted(
                &index_dir.path().join("index.db"),
                &InMemoryKeyStore::empty(),
            )
            .unwrap();
            Self {
                _index_dir: index_dir,
                index,
                tracker: StabilityTracker::new(),
            }
        }

        fn scan(&mut self, location: &Location, now: SystemTime) -> ScanReport {
            scan_location(&mut self.index, location, &mut self.tracker, now).unwrap()
        }

        /// Scans once to see the files, and again after they have held still for
        /// `STABLE_AFTER`. Returns the second report.
        fn scan_settled(&mut self, location: &Location) -> ScanReport {
            self.scan(location, t0());
            self.scan(location, t0() + STABLE_AFTER)
        }
    }

    fn location(dir: &TempDir, mode: LocationMode, images: bool) -> Location {
        Location::new(dir.path(), mode, images).unwrap()
    }

    fn put(location: &Location, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = location.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, bytes).unwrap();
        path
    }

    fn hash_of(bytes: &[u8]) -> ContentHash {
        ContentHash(Sha256::digest(bytes).into())
    }

    fn seconds_of(time: SystemTime) -> i64 {
        DateTime::<Utc>::from(time).timestamp()
    }

    #[derive(Debug, PartialEq)]
    struct Entry {
        path: PathBuf,
        is_dir: bool,
        size: u64,
        mtime: SystemTime,
        content: Vec<u8>,
    }

    fn snapshot(root: &Path) -> Vec<Entry> {
        WalkDir::new(root)
            .sort_by_file_name()
            .into_iter()
            .map(|step| {
                let entry = step.unwrap();
                // Not `entry.metadata()`: on Windows that is the directory
                // listing's copy, which NTFS updates late.
                let metadata = fs::symlink_metadata(entry.path()).unwrap();
                Entry {
                    path: entry.path().to_path_buf(),
                    is_dir: metadata.is_dir(),
                    size: metadata.len(),
                    mtime: metadata.modified().unwrap(),
                    content: if metadata.is_file() {
                        fs::read(entry.path()).unwrap()
                    } else {
                        Vec::new()
                    },
                }
            })
            .collect()
    }

    #[test]
    fn a_location_must_exist_and_is_stored_canonical() {
        let dir = TempDir::new().unwrap();

        let missing = Location::new(&dir.path().join("absent"), LocationMode::Inbox, false);
        let found = Location::new(dir.path(), LocationMode::Inbox, false).unwrap();

        missing.unwrap_err();
        assert_eq!(found.path, fs::canonicalize(dir.path()).unwrap());
    }

    #[test]
    fn backlog_scan_of_an_inbox_and_an_archive_in_one_index() {
        let mut fixture = Fixture::new();
        let inbox_dir = TempDir::new().unwrap();
        let archive_dir = TempDir::new().unwrap();
        let inbox = location(&inbox_dir, LocationMode::Inbox, false);
        let archive = location(&archive_dir, LocationMode::Archive, false);
        let letter = put(&inbox, "letter.pdf", INVOICE);
        put(&inbox, "notes/todo.txt", b"call Agency B");
        put(&inbox, "photo.jpg", b"jpeg");
        put(&inbox, ".secret.pdf", b"hidden");
        put(&inbox, "~$draft.docx", b"lock");
        put(&inbox, "movie.mp4", b"video");
        let filed = put(&archive, "2025/offer.pdf", REMINDER);
        put(&archive, "readme.md", b"# Archive");

        let first = fixture.scan(&inbox, t0());
        let inbox_report = fixture.scan(&inbox, t0() + STABLE_AFTER);
        fixture.scan(&archive, t0());
        let archive_report = fixture.scan(&archive, t0() + STABLE_AFTER);

        assert_eq!(first.new, 0);
        assert_eq!(first.not_stable.len(), 2);
        assert_eq!(inbox_report.new, 2);
        assert!(inbox_report.not_stable.is_empty());
        assert_eq!(inbox_report.skipped.get(&Skip::ImagesOff), Some(&1));
        assert_eq!(inbox_report.skipped.get(&Skip::Hidden), Some(&1));
        assert_eq!(inbox_report.skipped.get(&Skip::Temporary), Some(&1));
        assert_eq!(inbox_report.skipped.get(&Skip::Unsupported), Some(&1));
        assert_eq!(archive_report.new, 2);
        assert_eq!(archive_report.error_total, 0);

        let received = fixture.index.document_by_path(&letter).unwrap().unwrap();
        assert_eq!(received.source, DocumentSource::Inbox);
        assert_eq!(received.status, DocumentStatus::New);
        assert_eq!(
            received.received_at.timestamp(),
            seconds_of(t0() + STABLE_AFTER)
        );
        let indexed = fixture.index.document_by_path(&filed).unwrap().unwrap();
        assert_eq!(indexed.source, DocumentSource::Archive);
        assert_eq!(indexed.status, DocumentStatus::Filed);
        assert_eq!(indexed.hash, hash_of(REMINDER));
        let mtime = fs::metadata(&filed).unwrap().modified().unwrap();
        assert_eq!(indexed.received_at.timestamp(), seconds_of(mtime));
    }

    #[test]
    fn images_are_taken_in_only_where_the_location_enables_them() {
        let mut fixture = Fixture::new();
        let off_dir = TempDir::new().unwrap();
        let on_dir = TempDir::new().unwrap();
        let off = location(&off_dir, LocationMode::Inbox, false);
        let on = location(&on_dir, LocationMode::Inbox, true);
        put(&off, "scan.PNG", b"png one");
        put(&on, "scan.PNG", b"png two");

        let without = fixture.scan_settled(&off);
        let with = fixture.scan_settled(&on);

        assert_eq!(
            (without.new, without.skipped.get(&Skip::ImagesOff)),
            (0, Some(&1))
        );
        assert_eq!((with.new, with.skipped.get(&Skip::ImagesOff)), (1, None));
    }

    #[test]
    fn a_scan_leaves_an_archive_location_as_it_was() {
        let mut fixture = Fixture::new();
        let dir = TempDir::new().unwrap();
        let archive = location(&dir, LocationMode::Archive, true);
        put(&archive, "a.pdf", INVOICE);
        put(&archive, "copy/a.pdf", INVOICE);
        put(&archive, "deep/er/b.txt", b"text");
        put(&archive, "photo.jpg", b"jpeg");
        put(&archive, ".hidden/c.pdf", b"hidden");
        put(&archive, "partial.pdf.part", b"half");
        fs::create_dir_all(archive.path.join("empty")).unwrap();
        let before = snapshot(&archive.path);

        fixture.scan_settled(&archive);
        fixture.scan(&archive, t0() + STABLE_AFTER + STABLE_AFTER);

        assert_eq!(snapshot(&archive.path), before);
    }

    #[test]
    fn the_same_bytes_twice_in_an_archive_are_one_document_and_a_recorded_copy() {
        let mut fixture = Fixture::new();
        let dir = TempDir::new().unwrap();
        let archive = location(&dir, LocationMode::Archive, false);
        let original = put(&archive, "a.pdf", INVOICE);
        let copy = put(&archive, "b.pdf", INVOICE);

        let report = fixture.scan_settled(&archive);

        assert_eq!((report.new, report.copies), (1, 1));
        let document = fixture.index.document_by_path(&original).unwrap().unwrap();
        assert_eq!(
            fixture.index.duplicate_paths(document.id).unwrap(),
            vec![copy.clone()]
        );

        let again = fixture.scan(&archive, t0() + STABLE_AFTER * 2);
        assert_eq!((again.unchanged, again.copies, again.new), (2, 0, 0));

        fs::remove_file(&copy).unwrap();
        let after_delete = fixture.scan(&archive, t0() + STABLE_AFTER * 3);
        assert_eq!(after_delete.removed_copies, 1);
        assert!(fixture
            .index
            .duplicate_paths(document.id)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn deleting_the_original_of_a_copy_pair_makes_the_copy_the_document() {
        let mut fixture = Fixture::new();
        let dir = TempDir::new().unwrap();
        let archive = location(&dir, LocationMode::Archive, false);
        let original = put(&archive, "a.pdf", INVOICE);
        let copy = put(&archive, "b.pdf", INVOICE);
        fixture.scan_settled(&archive);
        let id = fixture
            .index
            .document_by_path(&original)
            .unwrap()
            .unwrap()
            .id;
        fs::remove_file(&original).unwrap();

        let report = fixture.scan(&archive, t0() + STABLE_AFTER * 2);

        assert_eq!(
            (report.promoted_copies, report.missing, report.copies),
            (1, 0, 0)
        );
        assert_eq!(fixture.index.document(id).unwrap().unwrap().path, copy);
        assert!(fixture.index.duplicate_paths(id).unwrap().is_empty());
    }

    #[test]
    fn a_deleted_file_without_a_copy_leaves_a_missing_document_that_comes_back() {
        let mut fixture = Fixture::new();
        let dir = TempDir::new().unwrap();
        let archive = location(&dir, LocationMode::Archive, false);
        let path = put(&archive, "a.pdf", INVOICE);
        fixture.scan_settled(&archive);
        let id = fixture.index.document_by_path(&path).unwrap().unwrap().id;
        fs::remove_file(&path).unwrap();

        let report = fixture.scan(&archive, t0() + STABLE_AFTER * 2);

        assert_eq!((report.missing, report.promoted_copies), (1, 0));
        assert!(fixture.index.document_by_path(&path).unwrap().is_none());
        assert!(fixture
            .index
            .document(id)
            .unwrap()
            .unwrap()
            .missing_since
            .is_some());

        let elsewhere = put(&archive, "back/a.pdf", INVOICE);
        let back = fixture.scan_settled(&archive);

        assert_eq!((back.moved, back.new), (1, 0));
        let document = fixture.index.document(id).unwrap().unwrap();
        assert_eq!(document.path, elsewhere);
        assert!(document.missing_since.is_none());
        let again = fixture.scan(&archive, t0() + STABLE_AFTER * 5);
        assert_eq!((again.missing, again.unchanged_without_hash), (0, 1));
    }

    #[test]
    fn an_original_changed_in_place_while_a_copy_exists_leaves_the_old_document_with_the_copy() {
        let mut fixture = Fixture::new();
        let dir = TempDir::new().unwrap();
        let archive = location(&dir, LocationMode::Archive, false);
        let original = put(&archive, "a.pdf", INVOICE);
        let copy = put(&archive, "b.pdf", INVOICE);
        fixture.scan_settled(&archive);
        let old = fixture.index.document_by_path(&original).unwrap().unwrap();
        fs::write(&original, b"a different, longer text").unwrap();

        let report = fixture.scan_settled(&archive);

        assert_eq!(report.new, 1);
        let kept = fixture.index.document(old.id).unwrap().unwrap();
        assert_eq!((kept.path, kept.hash), (copy, hash_of(INVOICE)));
        let changed = fixture
            .index
            .document_by_hash(&hash_of(b"a different, longer text"))
            .unwrap()
            .unwrap();
        assert_eq!(changed.path, original);
        assert_ne!(changed.id, old.id);
        assert!(fixture.index.duplicate_paths(old.id).unwrap().is_empty());
    }

    #[test]
    fn a_file_overwritten_with_another_documents_bytes_becomes_a_copy_of_it() {
        let mut fixture = Fixture::new();
        let dir = TempDir::new().unwrap();
        let archive = location(&dir, LocationMode::Archive, false);
        let first = put(&archive, "a.pdf", INVOICE);
        let second = put(&archive, "b.pdf", REMINDER);
        fixture.scan_settled(&archive);
        let id_first = fixture.index.document_by_path(&first).unwrap().unwrap().id;
        let id_second = fixture.index.document_by_path(&second).unwrap().unwrap().id;
        fs::write(&first, REMINDER).unwrap();

        let report = fixture.scan_settled(&archive);

        assert_eq!(
            (report.copies, report.content_changed, report.missing),
            (1, 0, 0)
        );
        assert!(fixture
            .index
            .document(id_first)
            .unwrap()
            .unwrap()
            .missing_since
            .is_some());
        assert_eq!(
            fixture.index.duplicate_paths(id_second).unwrap(),
            vec![first]
        );

        let again = fixture.scan(&archive, t0() + STABLE_AFTER * 5);
        assert_eq!(again.unchanged_without_hash, 2);
        assert_eq!(again.unchanged, 2);
    }

    #[test]
    fn a_second_scan_of_an_unchanged_archive_hashes_nothing() {
        let mut fixture = Fixture::new();
        let dir = TempDir::new().unwrap();
        let archive = location(&dir, LocationMode::Archive, false);
        put(&archive, "a.pdf", INVOICE);
        put(&archive, "b.pdf", INVOICE);
        put(&archive, "sub/c.txt", b"text");
        fixture.scan_settled(&archive);

        let report = fixture.scan(&archive, t0() + STABLE_AFTER * 5);

        assert_eq!(report.unchanged, 3);
        assert_eq!(report.unchanged_without_hash, 3);
        assert!(report.not_stable.is_empty());
        assert!(fixture.tracker.pending().is_empty());
    }

    #[test]
    fn a_case_only_rename_is_a_move_and_never_a_copy() {
        let mut fixture = Fixture::new();
        let dir = TempDir::new().unwrap();
        let archive = location(&dir, LocationMode::Archive, false);
        let before = put(&archive, "Invoice.pdf", INVOICE);
        fixture.scan_settled(&archive);
        let id = fixture.index.document_by_path(&before).unwrap().unwrap().id;
        let after = archive.path.join("invoice.pdf");
        fs::rename(&before, &after).unwrap();

        let report = fixture.scan_settled(&archive);

        assert_eq!((report.moved, report.copies), (1, 0));
        assert_eq!(fixture.index.document(id).unwrap().unwrap().path, after);
        assert!(fixture.index.duplicate_paths(id).unwrap().is_empty());
    }

    #[test]
    fn a_duplicate_within_the_inbox_is_a_copy_and_not_a_second_document() {
        let mut fixture = Fixture::new();
        let dir = TempDir::new().unwrap();
        let inbox = location(&dir, LocationMode::Inbox, false);
        let first = put(&inbox, "a.pdf", INVOICE);
        let second = put(&inbox, "b.pdf", INVOICE);

        let report = fixture.scan_settled(&inbox);

        assert_eq!((report.new, report.copies), (1, 1));
        let document = fixture
            .index
            .document_by_hash(&hash_of(INVOICE))
            .unwrap()
            .unwrap();
        assert_eq!(document.path, first);
        assert_eq!(
            fixture.index.duplicate_paths(document.id).unwrap(),
            vec![second]
        );
    }

    #[test]
    fn a_file_moved_within_the_archive_keeps_its_document() {
        let mut fixture = Fixture::new();
        let dir = TempDir::new().unwrap();
        let archive = location(&dir, LocationMode::Archive, false);
        let before = put(&archive, "2025/a.pdf", INVOICE);
        fixture.scan_settled(&archive);
        let id = fixture.index.document_by_path(&before).unwrap().unwrap().id;
        let after = archive.path.join("2026/Company A/a.pdf");
        fs::create_dir_all(after.parent().unwrap()).unwrap();
        fs::rename(&before, &after).unwrap();

        let report = fixture.scan_settled(&archive);

        assert_eq!((report.moved, report.copies, report.new), (1, 0, 0));
        assert_eq!(fixture.index.document(id).unwrap().unwrap().path, after);
    }

    #[test]
    fn a_file_moved_from_the_inbox_into_the_archive_becomes_filed() {
        let mut fixture = Fixture::new();
        let inbox_dir = TempDir::new().unwrap();
        let archive_dir = TempDir::new().unwrap();
        let inbox = location(&inbox_dir, LocationMode::Inbox, false);
        let archive = location(&archive_dir, LocationMode::Archive, false);
        let before = put(&inbox, "a.pdf", INVOICE);
        fixture.scan_settled(&inbox);
        let id = fixture.index.document_by_path(&before).unwrap().unwrap().id;
        let after = archive.path.join("a.pdf");
        fs::rename(&before, &after).unwrap();

        let report = fixture.scan_settled(&archive);

        assert_eq!(report.moved, 1);
        let document = fixture.index.document(id).unwrap().unwrap();
        assert_eq!(document.path, after);
        assert_eq!(document.status, DocumentStatus::Filed);
    }

    #[test]
    fn an_inbox_file_that_is_already_filed_stays_and_nothing_is_recorded() {
        let mut fixture = Fixture::new();
        let inbox_dir = TempDir::new().unwrap();
        let archive_dir = TempDir::new().unwrap();
        let inbox = location(&inbox_dir, LocationMode::Inbox, false);
        let archive = location(&archive_dir, LocationMode::Archive, false);
        put(&archive, "a.pdf", INVOICE);
        fixture.scan_settled(&archive);
        let in_inbox = put(&inbox, "again.pdf", INVOICE);

        let report = fixture.scan_settled(&inbox);

        assert_eq!((report.already_filed, report.copies, report.new), (1, 0, 0));
        assert!(in_inbox.exists());
        let document = fixture
            .index
            .document_by_hash(&hash_of(INVOICE))
            .unwrap()
            .unwrap();
        assert!(fixture
            .index
            .duplicate_paths(document.id)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn an_inbox_document_that_was_filed_also_counts_as_already_filed() {
        let mut fixture = Fixture::new();
        let dir = TempDir::new().unwrap();
        let inbox = location(&dir, LocationMode::Inbox, false);
        let first = put(&inbox, "a.pdf", INVOICE);
        fixture.scan_settled(&inbox);
        let document = fixture.index.document_by_path(&first).unwrap().unwrap();
        fixture
            .index
            .set_document_status(document.id, DocumentStatus::Filed)
            .unwrap();
        let second = put(&inbox, "b.pdf", INVOICE);

        let report = fixture.scan_settled(&inbox);

        assert_eq!((report.already_filed, report.copies), (1, 0));
        assert!(second.exists());
        assert_eq!(document.source, DocumentSource::Inbox);
        assert!(fixture
            .index
            .duplicate_paths(document.id)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_text_file_changed_in_place_gets_its_new_hash_and_loses_its_text() {
        let mut fixture = Fixture::new();
        let dir = TempDir::new().unwrap();
        let inbox = location(&dir, LocationMode::Inbox, false);
        let path = put(&inbox, "note.txt", b"first version");
        fixture.scan_settled(&inbox);
        let id = fixture.index.document_by_path(&path).unwrap().unwrap().id;
        fixture
            .index
            .set_document_text(id, "first version")
            .unwrap();
        fs::write(&path, b"second, longer version").unwrap();

        let report = fixture.scan_settled(&inbox);

        assert_eq!(report.content_changed, 1);
        let document = fixture.index.document(id).unwrap().unwrap();
        assert_eq!(document.hash, hash_of(b"second, longer version"));
        assert!(fixture.index.search_text("first", 10).unwrap().is_empty());
    }

    #[test]
    fn a_growing_file_is_taken_in_only_once_it_holds_still() {
        let mut fixture = Fixture::new();
        let dir = TempDir::new().unwrap();
        let inbox = location(&dir, LocationMode::Inbox, false);
        let path = put(&inbox, "download.pdf", b"first part");

        let first = fixture.scan(&inbox, t0());
        assert_eq!(
            (first.new, first.not_stable.clone()),
            (0, vec![path.clone()])
        );
        assert_eq!(fixture.tracker.pending(), vec![path.clone()]);

        fs::write(&path, b"first part and the second part").unwrap();
        let grown = fixture.scan(&inbox, t0() + STABLE_AFTER);
        assert_eq!(
            (grown.new, grown.not_stable.clone()),
            (0, vec![path.clone()])
        );

        let settled = fixture.scan(&inbox, t0() + STABLE_AFTER * 2);
        assert_eq!((settled.new, settled.not_stable.len()), (1, 0));
        assert!(fixture.tracker.pending().is_empty());
    }

    #[test]
    fn take_in_reports_what_happened_to_one_file() {
        let mut fixture = Fixture::new();
        let dir = TempDir::new().unwrap();
        let inbox = location(&dir, LocationMode::Inbox, false);
        let path = put(&inbox, "a.pdf", INVOICE);
        let hidden = put(&inbox, ".a.pdf", INVOICE);
        let mut outcome = |target: &Path, now| {
            take_in(
                &mut fixture.index,
                &inbox,
                target,
                &mut fixture.tracker,
                now,
            )
            .unwrap()
        };

        assert_eq!(outcome(&hidden, t0()), Outcome::Skipped(Skip::Hidden));
        assert_eq!(outcome(&path, t0()), Outcome::NotStableYet);
        let Outcome::New(id) = outcome(&path, t0() + STABLE_AFTER) else {
            panic!("expected a new document");
        };
        assert_eq!(outcome(&path, t0() + STABLE_AFTER), Outcome::Unchanged(id));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_location_is_neither_followed_nor_taken_in() {
        use std::os::unix::fs::symlink;

        let mut fixture = Fixture::new();
        let outside_dir = TempDir::new().unwrap();
        let outside = outside_dir.path().join("secret.pdf");
        fs::write(&outside, REMINDER).unwrap();
        fs::write(outside_dir.path().join("inside.pdf"), INVOICE).unwrap();
        let dir = TempDir::new().unwrap();
        let archive = location(&dir, LocationMode::Archive, false);
        put(&archive, "real.pdf", b"real");
        symlink(&outside, archive.path.join("link.pdf")).unwrap();
        symlink(outside_dir.path(), archive.path.join("linked-folder")).unwrap();

        let report = fixture.scan_settled(&archive);

        assert_eq!(report.new, 1);
        assert_eq!(report.skipped.get(&Skip::Symlink), Some(&1));
        assert_eq!(
            fixture.index.document_by_hash(&hash_of(REMINDER)).unwrap(),
            None
        );
        assert_eq!(
            fixture.index.document_by_hash(&hash_of(INVOICE)).unwrap(),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_file_below_a_symlinked_folder_is_not_taken_in() {
        use std::os::unix::fs::symlink;

        let mut fixture = Fixture::new();
        let outside_dir = TempDir::new().unwrap();
        fs::write(outside_dir.path().join("x.pdf"), REMINDER).unwrap();
        let dir = TempDir::new().unwrap();
        let inbox = location(&dir, LocationMode::Inbox, false);
        symlink(outside_dir.path(), inbox.path.join("link")).unwrap();
        let through_link = inbox.path.join("link/x.pdf");

        let outcome = take_in(
            &mut fixture.index,
            &inbox,
            &through_link,
            &mut fixture.tracker,
            t0() + STABLE_AFTER,
        )
        .unwrap();

        assert_eq!(outcome, Outcome::Skipped(Skip::Symlink));
        assert_eq!(
            fixture.index.document_by_hash(&hash_of(REMINDER)).unwrap(),
            None
        );
    }

    #[test]
    fn a_path_outside_the_location_is_refused() {
        let mut fixture = Fixture::new();
        let elsewhere = TempDir::new().unwrap();
        let outside = elsewhere.path().join("x.pdf");
        fs::write(&outside, REMINDER).unwrap();
        let dir = TempDir::new().unwrap();
        let inbox = location(&dir, LocationMode::Inbox, false);
        let dotdot = inbox.path.join("../x.pdf");

        for path in [&outside, &dotdot] {
            let outcome = take_in(
                &mut fixture.index,
                &inbox,
                path,
                &mut fixture.tracker,
                t0() + STABLE_AFTER,
            )
            .unwrap();
            assert_eq!(outcome, Outcome::Skipped(Skip::OutsideLocation));
        }
        assert_eq!(
            fixture.index.document_by_hash(&hash_of(REMINDER)).unwrap(),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_named_like_a_document_is_skipped_and_does_not_block() {
        let mut fixture = Fixture::new();
        let dir = TempDir::new().unwrap();
        let inbox = location(&dir, LocationMode::Inbox, false);
        let fifo = inbox.path.join("x.pdf");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(made.success());

        let report = fixture.scan_settled(&inbox);

        assert_eq!(report.new, 0);
        assert_eq!(report.skipped.get(&Skip::Unsupported), Some(&1));
        assert_eq!(report.error_total, 0);
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_folder_is_counted_and_does_not_stop_the_scan() {
        use std::os::unix::fs::PermissionsExt as _;

        let mut fixture = Fixture::new();
        let dir = TempDir::new().unwrap();
        let archive = location(&dir, LocationMode::Archive, false);
        put(&archive, "a.pdf", INVOICE);
        put(&archive, "locked/b.pdf", REMINDER);
        let locked = archive.path.join("locked");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

        let report = fixture.scan_settled(&archive);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(report.new, 1);
        assert_eq!(report.error_total, 1);
        assert_eq!(report.error_paths, vec![locked]);
    }

    fn receive_until(receiver: &Receiver<WatchEvent>, wanted: &Path) -> Vec<PathBuf> {
        let mut seen = Vec::new();
        loop {
            let event = receiver
                .recv_timeout(Duration::from_secs(10))
                .unwrap_or_else(|_| panic!("no event for {} after {seen:?}", wanted.display()));
            let WatchEvent::File(path) = event else {
                panic!("unexpected {event:?}");
            };
            let reached = path == wanted;
            seen.push(path);
            if reached {
                return seen;
            }
        }
    }

    #[test]
    fn the_watcher_forwards_new_files_and_not_hidden_ones() {
        let dir = TempDir::new().unwrap();
        let inbox = location(&dir, LocationMode::Inbox, false);
        let (_watcher, receiver, failed) = LocationWatcher::start(std::slice::from_ref(&inbox));
        assert!(failed.is_empty());

        let first = put(&inbox, "first.pdf", INVOICE);
        let seen = receive_until(&receiver, &first);
        assert!(seen.iter().all(|path| *path == first), "{seen:?}");

        let hidden = put(&inbox, ".hidden.pdf", REMINDER);
        let barrier = put(&inbox, "sub/barrier.pdf", b"barrier");
        let after_hidden = receive_until(&receiver, &barrier);
        assert!(!after_hidden.contains(&hidden), "{after_hidden:?}");
    }

    #[test]
    fn the_watcher_forwards_the_files_of_a_folder_moved_in() {
        let outside = TempDir::new().unwrap();
        let folder = outside.path().join("batch");
        fs::create_dir_all(folder.join("nested")).unwrap();
        fs::write(folder.join("a.pdf"), INVOICE).unwrap();
        fs::write(folder.join("nested/b.pdf"), REMINDER).unwrap();
        let dir = TempDir::new().unwrap();
        let inbox = location(&dir, LocationMode::Inbox, false);
        let (_watcher, receiver, failed) = LocationWatcher::start(std::slice::from_ref(&inbox));
        assert!(failed.is_empty());

        fs::rename(&folder, inbox.path.join("batch")).unwrap();

        let first = inbox.path.join("batch/a.pdf");
        let nested = inbox.path.join("batch/nested/b.pdf");
        let mut seen = receive_until(&receiver, &nested);
        if !seen.contains(&first) {
            seen.extend(receive_until(&receiver, &first));
        }
        assert!(seen.contains(&first) && seen.contains(&nested), "{seen:?}");
    }

    #[test]
    fn a_location_that_cannot_be_watched_is_reported_and_the_others_carry_on() {
        let gone_dir = TempDir::new().unwrap();
        let gone = location(&gone_dir, LocationMode::Archive, false);
        let gone_path = gone.path.clone();
        drop(gone_dir);
        let dir = TempDir::new().unwrap();
        let inbox = location(&dir, LocationMode::Inbox, false);

        let (_watcher, receiver, failed) = LocationWatcher::start(&[gone, inbox.clone()]);

        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].0, gone_path);
        let file = put(&inbox, "a.pdf", INVOICE);
        receive_until(&receiver, &file);
    }
}
