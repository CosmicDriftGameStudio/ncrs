#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use tempfile::TempDir;

use crate::key_store::InMemoryKeyStore;
use crate::Index;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/index-v1.2.0.db"
);
const FIXTURE_VERSION: u32 = 25;
const COUNTED_TABLES: [&str; 5] = [
    "document_tag",
    "review_entry",
    "document_field_origin",
    "duplicate_path",
    "execution_item WHERE document_id IS NOT NULL",
];

struct Before {
    documents: Vec<(i64, Vec<u8>, String)>,
    counts: Vec<i64>,
}

fn copy_of_the_fixture() -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("index.db");
    fs::copy(FIXTURE, &path).unwrap();
    (dir, path)
}

fn fixture_key_hex() -> String {
    (0..32).map(|byte: u8| format!("{byte:02x}")).collect()
}

fn count(conn: &Connection, from: &str) -> i64 {
    conn.query_row(&format!("SELECT count(*) FROM {from}"), [], |row| {
        row.get(0)
    })
    .unwrap()
}

fn user_version(conn: &Connection) -> u32 {
    conn.pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap()
}

/// Reads the copy as v1.2.0 wrote it, before any step of this release ran.
fn read_before(path: &Path) -> Before {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(&format!("PRAGMA key = \"x'{}'\";", fixture_key_hex()))
        .unwrap();
    conn.execute_batch("PRAGMA cipher_compatibility = 4;")
        .unwrap();
    assert_eq!(user_version(&conn), FIXTURE_VERSION);
    let mut statement = conn
        .prepare("SELECT id, hash, path FROM document ORDER BY id")
        .unwrap();
    let documents = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    drop(statement);
    let counts = COUNTED_TABLES
        .iter()
        .map(|from| count(&conn, from))
        .collect();
    Before { documents, counts }
}

fn migrated(path: &Path) -> Index {
    let key: Vec<u8> = (0..32).collect();
    Index::open(path, &InMemoryKeyStore::with_entry(key)).unwrap()
}

fn insert_raw(conn: &Connection, seed: u8, path: Option<&str>, origin_kind: &str) -> bool {
    conn.execute(
        "INSERT INTO document (hash, path, source, received_at, status, origin_kind)
         VALUES (?1, ?2, 'inbox', 0, 'new', ?3)",
        rusqlite::params![[seed; 32].as_slice(), path, origin_kind],
    )
    .is_ok()
}

#[test]
fn the_step_keeps_every_document_and_everything_that_points_at_it() {
    let (_dir, path) = copy_of_the_fixture();
    let before = read_before(&path);
    assert!(!before.documents.is_empty());

    let index = migrated(&path);
    let conn = index.connection();

    assert_eq!(user_version(conn), 26);
    let after: Vec<(i64, Vec<u8>, String)> = conn
        .prepare("SELECT id, hash, path FROM document ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(after, before.documents);
    assert_eq!(
        count(conn, "document WHERE origin_kind = 'file'"),
        i64::try_from(before.documents.len()).unwrap()
    );
    let counts: Vec<i64> = COUNTED_TABLES
        .iter()
        .map(|from| count(conn, from))
        .collect();
    assert_eq!(counts, before.counts);
}

#[test]
fn index_and_trigger_are_rebuilt_and_no_foreign_key_is_broken() {
    let (_dir, path) = copy_of_the_fixture();
    let index = migrated(&path);
    let conn = index.connection();

    for name in ["document_by_path", "document_text_follows_document"] {
        assert_eq!(
            count(conn, &format!("sqlite_master WHERE name = '{name}'")),
            1,
            "{name}"
        );
    }
    let mut check = conn.prepare("PRAGMA foreign_key_check").unwrap();
    assert!(check.query([]).unwrap().next().unwrap().is_none());
}

#[test]
fn text_search_and_lookup_by_path_find_the_same_document() {
    let (_dir, path) = copy_of_the_fixture();
    let index = migrated(&path);
    let conn = index.connection();
    let (id, body, document_path): (i64, String, String) = conn
        .query_row(
            "SELECT d.id, t.body, d.path FROM document d
             JOIN document_text t ON t.rowid = d.id ORDER BY d.id LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    let word = body
        .split(|character: char| !character.is_alphanumeric())
        .find(|part| part.len() >= 4)
        .unwrap();

    let found = index.search_text(word, 100).unwrap();
    let by_path = index
        .document_by_path(Path::new(&document_path))
        .unwrap()
        .unwrap();

    assert!(found.iter().any(|found_id| found_id.get() == id));
    assert_eq!(by_path.id.get(), id);
}

#[test]
fn deleting_a_document_still_takes_its_text_and_tags_along() {
    let (_dir, path) = copy_of_the_fixture();
    let index = migrated(&path);
    let conn = index.connection();
    let id: i64 = conn
        .query_row(
            "SELECT document_id FROM document_tag
             WHERE document_id IN (SELECT rowid FROM document_text) LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();

    conn.execute("DELETE FROM document WHERE id = ?1", [id])
        .unwrap();

    assert_eq!(count(conn, &format!("document_text WHERE rowid = {id}")), 0);
    assert_eq!(
        count(conn, &format!("document_tag WHERE document_id = {id}")),
        0
    );
}

#[test]
fn a_file_needs_a_path_but_a_mail_does_not_and_the_kind_is_closed() {
    let (_dir, path) = copy_of_the_fixture();
    let index = migrated(&path);
    let conn = index.connection();

    assert!(!insert_raw(conn, 200, None, "file"));
    assert!(insert_raw(conn, 201, None, "mail"));
    assert!(!insert_raw(conn, 202, Some("a.pdf"), "fax"));
    assert!(index
        .document_by_path(Path::new("a.pdf"))
        .unwrap()
        .is_none());
}
