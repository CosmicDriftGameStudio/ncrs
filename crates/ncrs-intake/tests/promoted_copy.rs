use std::fs;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ncrs_index::{DocumentStatus, InMemoryKeyStore, Index};
use ncrs_intake::{scan_location, Location, LocationMode, StabilityTracker, STABLE_AFTER};
use tempfile::TempDir;

const INVOICE: &[u8] = b"Invoice 4711 from Company A";

fn t0() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_800_000_000)
}

struct World {
    _index_dir: TempDir,
    index: Index,
    tracker: StabilityTracker,
}

impl World {
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

    fn scan_settled(&mut self, location: &Location) {
        scan_location(&mut self.index, location, &mut self.tracker, t0()).unwrap();
        scan_location(
            &mut self.index,
            location,
            &mut self.tracker,
            t0() + STABLE_AFTER,
        )
        .unwrap();
    }
}

fn location(dir: &TempDir, mode: LocationMode) -> Location {
    Location::new(dir.path(), mode, false).unwrap()
}

#[test]
fn an_archive_copy_that_takes_over_from_an_inbox_file_makes_the_document_filed() {
    let mut world = World::new();
    let inbox_dir = TempDir::new().unwrap();
    let archive_dir = TempDir::new().unwrap();
    let inbox = location(&inbox_dir, LocationMode::Inbox);
    let archive = location(&archive_dir, LocationMode::Archive);
    let inbox_file = inbox.path.join("invoice.pdf");
    fs::write(&inbox_file, INVOICE).unwrap();
    fs::write(inbox.path.join("stays.pdf"), b"Reminder 12 from Agency B").unwrap();
    world.scan_settled(&inbox);
    let archive_file = archive.path.join("invoice.pdf");
    fs::write(&archive_file, INVOICE).unwrap();
    world.scan_settled(&archive);
    let id = world
        .index
        .document_by_path(&inbox_file)
        .unwrap()
        .unwrap()
        .id;
    assert_eq!(
        world.index.document(id).unwrap().unwrap().status,
        DocumentStatus::New
    );
    fs::remove_file(&inbox_file).unwrap();

    let report = scan_location(
        &mut world.index,
        &inbox,
        &mut world.tracker,
        t0() + STABLE_AFTER * 2,
    )
    .unwrap();

    assert_eq!(report.promoted_copies, 1);
    let document = world.index.document(id).unwrap().unwrap();
    assert_eq!(document.path, archive_file);
    assert_eq!(document.status, DocumentStatus::Filed);
}
