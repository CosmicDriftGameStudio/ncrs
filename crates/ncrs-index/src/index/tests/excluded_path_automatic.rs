use super::super::*;
use super::keys::*;
use super::releases::*;
use crate::key_store::InMemoryKeyStore;
use crate::model::FileStamp;
use crate::test_support::native;
use std::time::{Duration, UNIX_EPOCH};
use tempfile::TempDir;

#[test]
fn a_version_32_database_keeps_its_exclusions_as_the_users_own() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let old = Index::create_plain_with_migrations(&path, &MIGRATIONS[..32]).unwrap();
    assert_eq!(user_version(&old), 32);
    let file = native("/inbox/Scan.png");
    old.conn
        .execute(
            "INSERT INTO excluded_path (path) VALUES (?1)",
            [file.to_str().unwrap()],
        )
        .unwrap();
    drop(old);

    let index = Index::open(&path, &InMemoryKeyStore::empty()).unwrap();

    assert_eq!(user_version(&index), latest_version());
    assert!(index.is_excluded(&file).unwrap());
    let other_file = FileStamp {
        size: 7,
        mtime: UNIX_EPOCH + Duration::from_secs(5),
    };
    assert!(!index
        .lift_outdated_automatic_exclusion(&file, other_file)
        .unwrap());
    assert!(index.is_excluded(&file).unwrap());
}
