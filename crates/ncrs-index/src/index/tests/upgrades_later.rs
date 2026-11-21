use super::super::*;
use super::keys::*;
use super::releases::*;
use crate::key_store::InMemoryKeyStore;
use tempfile::TempDir;

#[test]
fn a_version_29_database_keeps_its_file_list_exceptions_in_every_group() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let old = Index::create_plain_with_migrations(&path, &MIGRATIONS[..29]).unwrap();
    assert_eq!(user_version(&old), 29);
    old.conn
        .execute_batch(
            "INSERT INTO file_list_tag (tag, shown) VALUES ('telekom', 0);
             INSERT INTO file_list_tag (tag, shown) VALUES ('steuer', 1);",
        )
        .unwrap();
    drop(old);

    let index = Index::open(&path, &InMemoryKeyStore::empty()).unwrap();

    assert_eq!(user_version(&index), latest_version());
    let exceptions = index.file_list_tag_exceptions().unwrap();
    assert_eq!(exceptions.len(), 2 * crate::TagGroup::ALL.len());
    for group in crate::TagGroup::ALL {
        assert_eq!(exceptions[&(group, "telekom".to_owned())], false);
        assert_eq!(exceptions[&(group, "steuer".to_owned())], true);
    }
    index
        .set_file_list_tag(crate::TagGroup::Sender, "telekom", None)
        .unwrap();
    assert_eq!(
        index.file_list_tag_exceptions().unwrap().len(),
        2 * crate::TagGroup::ALL.len() - 1
    );
}

#[test]
fn a_version_30_database_marks_the_documents_that_have_a_text() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let old = Index::create_plain_with_migrations(&path, &MIGRATIONS[..30]).unwrap();
    assert_eq!(user_version(&old), 30);
    old.conn
        .execute_batch(
            "INSERT INTO document (id, hash, path, source, received_at, status, file_size, file_mtime)
                 VALUES (1, randomblob(32), 'a.pdf', 'archive', 1700000000, 'new', 1, 1),
                        (2, randomblob(32), 'b.pdf', 'archive', 1700000000, 'new', 1, 1),
                        (3, randomblob(32), 'c.pdf', 'archive', 1700000000, 'new', 1, 1);
             INSERT INTO document_text (rowid, body) VALUES (1, 'Rechnung'), (2, '   ');",
        )
        .unwrap();
    drop(old);

    let index = Index::open(&path, &InMemoryKeyStore::empty()).unwrap();

    assert_eq!(user_version(&index), latest_version());
    let marks: Vec<i64> = index
        .conn
        .prepare("SELECT has_text FROM document ORDER BY id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(marks, [1, 0, 0]);
    assert_eq!(
        index.hermes_counts().unwrap().archive_documents_with_text,
        1
    );
}

#[test]
fn a_version_34_database_keeps_its_copies_with_an_unknown_place() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let old = Index::create_plain_with_migrations(&path, &MIGRATIONS[..34]).unwrap();
    assert_eq!(user_version(&old), 34);
    old.conn
        .execute_batch(
            "INSERT INTO document (id, hash, path, source, received_at, status, file_size, file_mtime)
                 VALUES (1, randomblob(32), 'a.pdf', 'inbox', 1700000000, 'new', 1, 1);
             INSERT INTO duplicate_path (document_id, path, seen_at, file_size, file_mtime)
                 VALUES (1, 'archive/copy.pdf', 1, 2, 3);",
        )
        .unwrap();
    drop(old);

    let index = Index::open(&path, &InMemoryKeyStore::empty()).unwrap();

    assert_eq!(user_version(&index), latest_version());
    let copy = Path::new("archive/copy.pdf");
    assert_eq!(
        index.duplicate_paths(crate::DocumentId::new(1)).unwrap(),
        vec![copy.to_path_buf()]
    );
    assert_eq!(index.duplicate_path_in_archive(copy).unwrap(), None);
    index
        .record_duplicate_path_in(
            crate::DocumentId::new(1),
            copy,
            crate::FileStamp {
                size: 2,
                mtime: std::time::UNIX_EPOCH + std::time::Duration::from_nanos(3),
            },
            true,
        )
        .unwrap();
    assert_eq!(index.duplicate_path_in_archive(copy).unwrap(), Some(true));
    index
        .record_duplicate_path(
            crate::DocumentId::new(1),
            copy,
            crate::FileStamp {
                size: 2,
                mtime: std::time::UNIX_EPOCH + std::time::Duration::from_nanos(3),
            },
        )
        .unwrap();
    assert_eq!(index.duplicate_path_in_archive(copy).unwrap(), Some(true));
}
