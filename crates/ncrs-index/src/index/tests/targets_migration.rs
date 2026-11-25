use super::super::*;
use super::keys::*;
use super::releases::*;
use crate::key_store::InMemoryKeyStore;
use crate::model::{DocumentId, DocumentStatus, RuleId, TargetId};
use tempfile::TempDir;

#[test]
fn the_release_1_7_0_index_keeps_its_rules_in_order_under_main_and_switches_off_the_unconfirmed() {
    let (_dir, path, store) = copy_of_the_v1_7_0_index();

    let index = Index::open_with_migrations(&path, &store, MIGRATIONS).unwrap();

    let rules = index.rules().unwrap();
    let order: Vec<_> = rules.iter().map(|rule| rule.id.get()).collect();
    assert_eq!(order[..4], [4, 1, 2, 3]);
    assert_eq!(rules.len(), 5);
    assert!(rules.iter().all(|rule| rule.target == TargetId::main()));
    assert_eq!(index.rules_of(&TargetId::main()).unwrap(), rules);
    let enabled: Vec<_> = rules.iter().map(|rule| rule.spec.enabled).collect();
    assert_eq!(enabled, [false, true, false, false, true]);
    let confirmed: Vec<_> = rules.iter().map(|rule| rule.spec.confirmed).collect();
    assert_eq!(confirmed, [false, true, true, false, true]);
    assert!(index.open_reviews().unwrap().is_empty());
}

#[test]
fn a_version_36_database_loses_planned_documents_and_open_questions_and_keeps_filed_ones() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let old = Index::create_plain_with_migrations(&path, &MIGRATIONS[..36]).unwrap();
    assert_eq!(user_version(&old), 36);
    old.conn
        .execute_batch(
            "INSERT INTO rule (id, filing_pattern, position, confirmed, enabled)
                 VALUES (1, 'a', 1, 1, 1), (2, 'b', 2, 0, 1), (3, 'c', 3, 1, 0);
             INSERT INTO document (id, hash, path, source, received_at, status)
                 VALUES (1, zeroblob(32), 'a.pdf', 'inbox', 1, 'planned'),
                        (2, randomblob(32), 'b.pdf', 'inbox', 1, 'in_review'),
                        (3, randomblob(32), 'c.pdf', 'archive', 1, 'filed');
             INSERT INTO review_entry (id, document_id, reason, created_at)
                 VALUES (1, 2, 'target_exists', 1);
             INSERT INTO discarded_rule_suggestion
                     (match_text, condition_kind, kind, filing_pattern, discarded_at)
                 VALUES ('x', 'word', 'invoice', '{year}', 1);",
        )
        .unwrap();
    drop(old);

    let mut index = Index::open(&path, &InMemoryKeyStore::empty()).unwrap();

    assert_eq!(user_version(&index), latest_version());
    let rules = index.rules().unwrap();
    let flags: Vec<_> = rules
        .iter()
        .map(|rule| (rule.id, rule.spec.confirmed, rule.spec.enabled))
        .collect();
    assert_eq!(
        flags,
        [
            (RuleId::new(1), true, true),
            (RuleId::new(2), false, false),
            (RuleId::new(3), true, false)
        ]
    );
    let statuses: Vec<_> = (1..=3)
        .map(|id| index.document(DocumentId::new(id)).unwrap().unwrap().status)
        .collect();
    assert_eq!(
        statuses,
        [
            DocumentStatus::New,
            DocumentStatus::New,
            DocumentStatus::Filed
        ]
    );
    assert!(index.open_reviews().unwrap().is_empty());
    let discarded_target: String = index
        .conn
        .query_row("SELECT target FROM discarded_rule_suggestion", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(discarded_target, "main");
    assert!(table_exists(&index, "source_backlog"));
    index
        .set_source_backlog(std::path::Path::new("inbox"), &[], None)
        .unwrap();
    assert!(table_exists(&index, "source_watch"));
    let batch = index.begin_batch(crate::BatchKind::Filing).unwrap();
    assert_eq!(index.batch_info(batch).unwrap().unwrap().run, None);
}
