use super::super::*;
use super::keys::*;
use crate::key_store::InMemoryKeyStore;
use crate::model::ReviewReason;
use crate::test_support::{new_document, rule_spec};
use tempfile::TempDir;

fn max_sequence(index: &Index, table: &str) -> Option<i64> {
    index
        .conn
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = ?1",
            [table],
            |row| row.get(0),
        )
        .ok()
}

#[test]
fn a_deleted_rule_or_review_entry_does_not_hand_its_id_to_the_next_one() {
    let dir = TempDir::new().unwrap();
    let store = InMemoryKeyStore::empty();
    let (mut index, _) = Index::create_encrypted(&db_in(&dir), &store).unwrap();

    let rule = index.create_rule(&rule_spec()).unwrap();
    assert!(index.delete_rule(rule).unwrap());
    let next_rule = index.create_rule(&rule_spec()).unwrap();
    assert_ne!(next_rule, rule);

    let first = index.insert_document(&new_document(1)).unwrap();
    let entry = index
        .add_review(first, ReviewReason::NoMatchingRule)
        .unwrap();
    index
        .conn
        .execute("DELETE FROM document WHERE id = ?1", [first.get()])
        .unwrap();
    let second = index.insert_document(&new_document(2)).unwrap();
    let next_entry = index
        .add_review(second, ReviewReason::NoMatchingRule)
        .unwrap();
    assert_ne!(next_entry, entry);
}

#[test]
fn a_version_28_database_keeps_its_rules_and_reviews_and_stops_reusing_their_ids() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let store = InMemoryKeyStore::empty();
    let (old, _) =
        Index::create_encrypted_with_migrations(&path, &store, &MIGRATIONS[..28]).unwrap();
    assert_eq!(user_version(&old), 28);
    old.conn
        .execute_batch(
            "INSERT INTO tag (id, name, name_key) VALUES (1, 'steuer', 'steuer');
             INSERT INTO rule (id, filing_pattern, position, name) VALUES (3, 'a/{year}', 1, 'one');
             INSERT INTO rule (id, filing_pattern, position, enabled) VALUES (7, 'b/{year}', 2, 0);
             INSERT INTO rule_tag (rule_id, tag_id) VALUES (7, 1);
             INSERT INTO document (id, hash, path, source, received_at, status)
                 VALUES (1, zeroblob(32), 'a.pdf', 'inbox', 1, 'in_review');
             INSERT INTO document (id, hash, path, source, received_at, status)
                 VALUES (2, x'0101010101010101010101010101010101010101010101010101010101010101',
                         'b.pdf', 'inbox', 1, 'in_review');
             INSERT INTO review_entry (id, document_id, reason, created_at)
                 VALUES (5, 1, 'no_matching_rule', 1);
             INSERT INTO review_entry (id, document_id, reason, created_at, resolved_at)
                 VALUES (9, 2, 'no_matching_rule', 1, 2);",
        )
        .unwrap();
    drop(old);

    let mut index = Index::open(&path, &store).unwrap();

    let rows = |sql: &str| -> Vec<(i64, String)> {
        let mut statement = index.conn.prepare(sql).unwrap();
        let mapped = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap();
        mapped.map(Result::unwrap).collect()
    };
    assert_eq!(
        rows("SELECT id, filing_pattern FROM rule ORDER BY id"),
        [(3, "a/{year}".to_owned()), (7, "b/{year}".to_owned())]
    );
    assert_eq!(
        rows("SELECT rule_id, 'x' FROM rule_tag"),
        [(7, "x".to_owned())]
    );
    assert_eq!(
        rows("SELECT id, reason FROM review_entry ORDER BY id"),
        [
            (5, "no_matching_rule".to_owned()),
            (9, "no_matching_rule".to_owned())
        ]
    );
    let open_twice = index.conn.execute(
        "INSERT INTO review_entry (document_id, reason, created_at) VALUES (1, 'no_matching_rule', 3)",
        [],
    );
    assert!(open_twice.is_err(), "one open review per document survives");
    let violations: i64 = index
        .conn
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(violations, 0);
    assert_eq!(max_sequence(&index, "rule"), Some(7));
    assert_eq!(max_sequence(&index, "review_entry"), Some(9));

    assert!(index.delete_rule(crate::RuleId::new(7)).unwrap());
    assert!(index.create_rule(&rule_spec()).unwrap().get() > 7);
    index
        .conn
        .execute("DELETE FROM review_entry WHERE id = 9", [])
        .unwrap();
    let entry = index
        .add_review(crate::DocumentId::new(2), ReviewReason::NoMatchingRule)
        .unwrap();
    assert!(entry.get() > 9);
}
