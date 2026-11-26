use super::super::*;
use super::keys::*;
use super::releases::*;
use crate::key_store::InMemoryKeyStore;
use crate::model::FileStamp;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};
use tempfile::TempDir;

fn stamp(size: u64, seconds: u64) -> FileStamp {
    FileStamp {
        size,
        mtime: UNIX_EPOCH + Duration::from_secs(seconds),
    }
}

#[test]
fn a_version_33_database_upgrades_and_keeps_its_documents() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let old = Index::create_plain_with_migrations(&path, &MIGRATIONS[..33]).unwrap();
    assert!(!table_exists(&old, "filed_inbox_path"));
    old.conn
        .execute_batch(
            "INSERT INTO document (id, hash, path, source, received_at, status)
             VALUES (1, zeroblob(32), '/archive/a.pdf', 'archive', 1, 'filed');",
        )
        .unwrap();
    drop(old);

    let index = Index::open(&path, &InMemoryKeyStore::empty()).unwrap();

    assert_eq!(user_version(&index), latest_version());
    let document = index
        .document_by_path(Path::new("/archive/a.pdf"))
        .unwrap()
        .unwrap();
    assert!(index
        .filed_document_for_inbox_path(Path::new("/inbox/a.pdf"), stamp(1, 1))
        .unwrap()
        .is_none());
    index
        .record_filed_inbox_path(document.id, Path::new("/inbox/a.pdf"), stamp(1, 1))
        .unwrap();
    assert_eq!(
        index
            .filed_document_for_inbox_path(Path::new("/inbox/a.pdf"), stamp(1, 1))
            .unwrap(),
        Some(document.id)
    );
    assert!(index
        .filed_document_for_inbox_path(Path::new("/inbox/a.pdf"), stamp(2, 1))
        .unwrap()
        .is_none());
}

#[test]
fn a_noted_repeat_can_be_dropped() {
    let dir = TempDir::new().unwrap();
    let index = Index::create_plain(&db_in(&dir)).unwrap();
    index
        .conn
        .execute_batch(
            "INSERT INTO document (id, hash, path, source, received_at, status)
             VALUES (1, zeroblob(32), '/archive/a.pdf', 'archive', 1, 'filed');",
        )
        .unwrap();
    let keeper = crate::model::DocumentId::new(1);
    let noted = |path: &str| {
        index
            .filed_document_for_inbox_path(Path::new(path), stamp(1, 1))
            .unwrap()
    };
    for name in ["/inbox/b.pdf", "/inbox/a.pdf"] {
        index
            .record_filed_inbox_path(keeper, Path::new(name), stamp(1, 1))
            .unwrap();
    }

    assert!(index
        .remove_filed_inbox_path(Path::new("/inbox/a.pdf"))
        .unwrap());
    assert!(!index
        .remove_filed_inbox_path(Path::new("/inbox/a.pdf"))
        .unwrap());
    assert_eq!(noted("/inbox/a.pdf"), None);
    assert_eq!(noted("/inbox/b.pdf"), Some(keeper));
}
