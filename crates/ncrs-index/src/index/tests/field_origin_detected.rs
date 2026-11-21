use super::super::*;
use super::keys::*;
use super::releases::*;
use crate::key_store::InMemoryKeyStore;
use crate::model::{Field, FieldOrigin};
use tempfile::TempDir;

#[test]
fn a_version_31_database_keeps_every_origin_and_learns_the_detected_one() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let old = Index::create_plain_with_migrations(&path, &MIGRATIONS[..31]).unwrap();
    assert_eq!(user_version(&old), 31);
    old.conn
        .execute_batch(
            "INSERT INTO document (id, hash, path, source, received_at, status, file_size, file_mtime)
                 VALUES (1, randomblob(32), 'a.pdf', 'archive', 1700000000, 'filed', 1, 1);
             INSERT INTO rule (id, filing_pattern, confirmed) VALUES (5, '{year}', 1);
             INSERT INTO document_field_origin (document_id, field, origin, rule_id, confidence)
                 VALUES (1, 'sender', 'rule', 5, NULL),
                        (1, 'kind', 'rule', NULL, NULL),
                        (1, 'recipient', 'address', NULL, NULL),
                        (1, 'date', 'ai', NULL, 80),
                        (1, 'number', 'manual', NULL, NULL);",
        )
        .unwrap();
    drop(old);

    let index = Index::open(&path, &InMemoryKeyStore::empty()).unwrap();

    assert_eq!(user_version(&index), latest_version());
    let id = index
        .document_by_path(std::path::Path::new("a.pdf"))
        .unwrap()
        .unwrap()
        .id;
    let origins = index.field_origins(id).unwrap();
    assert!(matches!(
        origins.of(Field::Sender),
        FieldOrigin::Rule(Some(_))
    ));
    assert_eq!(origins.of(Field::Kind), FieldOrigin::Rule(None));
    assert_eq!(origins.of(Field::Recipient), FieldOrigin::AddressBlock);
    assert_eq!(
        origins.of(Field::Date),
        FieldOrigin::Ai {
            confidence_percent: 80
        }
    );
    assert_eq!(origins.of(Field::Number), FieldOrigin::Manual);
    index
        .conn
        .execute(
            "INSERT INTO document_field_origin (document_id, field, origin) VALUES (1, 'amount', 'detected')",
            [],
        )
        .unwrap();
    assert_eq!(
        index.field_origins(id).unwrap().of(Field::Amount),
        FieldOrigin::Detected
    );
}
