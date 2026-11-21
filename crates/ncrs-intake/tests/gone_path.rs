#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use ncrs_index::{
        BatchKind, ContentHash, DocumentId, InMemoryKeyStore, Index, JournalState, NewJournalItem,
    };
    use ncrs_intake::{
        scan_location, settle_gone_path, Location, LocationMode, StabilityTracker, STABLE_AFTER,
    };
    use tempfile::TempDir;

    fn t0() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_800_000_000)
    }

    /// A scanned archive with one document, whose file then leaves for `elsewhere`.
    struct Moved {
        _archive_dir: TempDir,
        _elsewhere: TempDir,
        _index_dir: TempDir,
        index: Index,
        location: Location,
        id: DocumentId,
        from: std::path::PathBuf,
        to: std::path::PathBuf,
    }

    fn moved_out() -> Moved {
        let index_dir = TempDir::new().unwrap();
        let (mut index, _recovery_key) = Index::create_encrypted(
            &index_dir.path().join("index.db"),
            &InMemoryKeyStore::empty(),
        )
        .unwrap();
        let archive_dir = TempDir::new().unwrap();
        let elsewhere = TempDir::new().unwrap();
        let location = Location::new(archive_dir.path(), LocationMode::Archive, false).unwrap();
        let from = location.path.join("invoice.pdf");
        fs::write(&from, b"Invoice 4711 from Company A").unwrap();
        fs::write(
            location.path.join("stays.pdf"),
            b"Reminder 12 from Agency B",
        )
        .unwrap();
        let mut tracker = StabilityTracker::new();
        scan_location(&mut index, &location, &mut tracker, t0()).unwrap();
        scan_location(&mut index, &location, &mut tracker, t0() + STABLE_AFTER).unwrap();
        let id = index.document_by_path(&from).unwrap().unwrap().id;
        let to = elsewhere.path().join("invoice.pdf");
        fs::rename(&from, &to).unwrap();
        Moved {
            _archive_dir: archive_dir,
            _elsewhere: elsewhere,
            _index_dir: index_dir,
            index,
            location,
            id,
            from,
            to,
        }
    }

    fn is_missing(world: &Moved) -> bool {
        world
            .index
            .document(world.id)
            .unwrap()
            .unwrap()
            .missing_since
            .is_some()
    }

    #[test]
    fn a_file_that_vanished_without_a_move_under_way_is_marked_missing() {
        let world = moved_out();

        settle_gone_path(&world.index, &world.location, &world.from).unwrap();

        assert!(is_missing(&world));
    }

    #[test]
    fn a_file_the_app_is_moving_is_not_marked_missing_before_the_index_is_repointed() {
        let world = moved_out();
        let batch = world.index.begin_batch(BatchKind::Filing).unwrap();
        world
            .index
            .add_journal_item(
                batch,
                &NewJournalItem {
                    from: world.from.clone(),
                    to: world.to.clone(),
                    hash: ContentHash([1; 32]),
                    document_id: None,
                    duplicate: false,
                    state: JournalState::Pending,
                    reason: None,
                },
            )
            .unwrap();

        settle_gone_path(&world.index, &world.location, &world.from).unwrap();

        assert!(!is_missing(&world));
    }
}
