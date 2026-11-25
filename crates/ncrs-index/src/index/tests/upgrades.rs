use super::super::*;
use super::keys::*;
use super::releases::*;
use crate::key_store::InMemoryKeyStore;
use tempfile::TempDir;

#[test]
fn a_version_1_database_keeps_its_documents_and_gains_the_location_columns() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let store = InMemoryKeyStore::empty();
    let (old, _) =
        Index::create_encrypted_with_migrations(&path, &store, &MIGRATIONS[..1]).unwrap();
    assert_eq!(user_version(&old), 1);
    old.conn
        .execute(
            "INSERT INTO document (hash, path, source, received_at, status)
             VALUES (zeroblob(32), 'inbox/a.pdf', 'inbox', 1, 'new')",
            [],
        )
        .unwrap();
    drop(old);

    let index = Index::open(&path, &store).unwrap();

    assert_eq!(
        user_version(&index),
        u32::try_from(MIGRATIONS.len()).unwrap()
    );
    let document = index
        .document_by_path(std::path::Path::new("inbox/a.pdf"))
        .unwrap()
        .unwrap();
    assert_eq!(document.stamp, None);
    assert!(table_exists(&index, "duplicate_path"));
    index
        .record_duplicate_path(
            document.id,
            std::path::Path::new("inbox/b.pdf"),
            crate::FileStamp {
                size: 1,
                mtime: std::time::UNIX_EPOCH,
            },
        )
        .unwrap();
}

#[test]
fn a_version_2_database_gains_the_location_profile_tables() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let store = InMemoryKeyStore::empty();
    let (old, _) =
        Index::create_encrypted_with_migrations(&path, &store, &MIGRATIONS[..2]).unwrap();
    assert_eq!(user_version(&old), 2);
    assert!(!table_exists(&old, "location_profile"));
    drop(old);

    let index = Index::open(&path, &store).unwrap();

    assert_eq!(
        user_version(&index),
        u32::try_from(MIGRATIONS.len()).unwrap()
    );
    assert!(table_exists(&index, "location_profile"));
    assert!(table_exists(&index, "location_subtree"));
}

#[test]
fn a_version_3_database_gains_the_rule_evidence_table() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let store = InMemoryKeyStore::empty();
    let (old, _) =
        Index::create_encrypted_with_migrations(&path, &store, &MIGRATIONS[..3]).unwrap();
    assert_eq!(user_version(&old), 3);
    assert!(!table_exists(&old, "rule_evidence"));
    drop(old);

    let index = Index::open(&path, &store).unwrap();

    assert_eq!(
        user_version(&index),
        u32::try_from(MIGRATIONS.len()).unwrap()
    );
    assert!(table_exists(&index, "rule_evidence"));
}

#[test]
fn a_version_4_database_gains_the_discarded_suggestions_table() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let store = InMemoryKeyStore::empty();
    let (old, _) =
        Index::create_encrypted_with_migrations(&path, &store, &MIGRATIONS[..4]).unwrap();
    assert_eq!(user_version(&old), 4);
    assert!(!table_exists(&old, "discarded_rule_suggestion"));
    drop(old);

    let index = Index::open(&path, &store).unwrap();

    assert_eq!(
        user_version(&index),
        u32::try_from(MIGRATIONS.len()).unwrap()
    );
    assert!(table_exists(&index, "discarded_rule_suggestion"));
}

#[test]
fn a_version_5_database_gains_the_reorganisation_plan_tables() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let store = InMemoryKeyStore::empty();
    let (old, _) =
        Index::create_encrypted_with_migrations(&path, &store, &MIGRATIONS[..5]).unwrap();
    assert_eq!(user_version(&old), 5);
    assert!(!table_exists(&old, "reorganisation_plan"));
    drop(old);

    let index = Index::open(&path, &store).unwrap();

    assert_eq!(
        user_version(&index),
        u32::try_from(MIGRATIONS.len()).unwrap()
    );
    for table in [
        "reorganisation_plan",
        "reorganisation_item",
        "reorganisation_finding",
    ] {
        assert!(table_exists(&index, table), "{table}");
    }
}

#[test]
fn a_version_6_database_gains_the_execution_journal_tables() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let store = InMemoryKeyStore::empty();
    let (old, _) =
        Index::create_encrypted_with_migrations(&path, &store, &MIGRATIONS[..6]).unwrap();
    assert_eq!(user_version(&old), 6);
    assert!(!table_exists(&old, "execution_batch"));
    drop(old);

    let index = Index::open(&path, &store).unwrap();

    assert_eq!(
        user_version(&index),
        u32::try_from(MIGRATIONS.len()).unwrap()
    );
    for table in ["execution_batch", "execution_item", "execution_created_dir"] {
        assert!(table_exists(&index, table), "{table}");
    }
}

#[test]
fn a_version_10_database_turns_its_rule_columns_into_conditions() {
    use crate::model::{Combinator, Condition, ConditionKind, FieldOrigin, RuleConditions, RuleId};

    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let old = Index::create_plain_with_migrations(&path, &MIGRATIONS[..10]).unwrap();
    assert_eq!(user_version(&old), 10);
    old.conn
        .execute_batch(
            "INSERT INTO document_kind (id, name, name_key) VALUES (1, 'Invoice', 'invoice');
             INSERT INTO sender (id, name, name_key) VALUES (1, 'Company A', 'company a');
             INSERT INTO tag (id, name, name_key) VALUES (1, 'services', 'services');
             INSERT INTO rule (id, match_text, match_text_is_regex, match_sender_id,
                 kind_id, tax_context, filing_pattern, confirmed)
             VALUES (3, 'Invoice', 0, NULL, 1, 'company', '{year}', 1),
                    (5, 'Invoice \\d+', 1, NULL, 1, 'private', '{year}', 0),
                    (8, NULL, 0, 1, 1, 'none', '{sender}', 1);
             INSERT INTO rule_tag (rule_id, tag_id) VALUES (8, 1);
             INSERT INTO rule_evidence
                 (rule_id, matching_in_folder, folder_documents, matching_elsewhere, updated_at)
             VALUES (8, 4, 5, 1, 1700000000);
             INSERT INTO document (id, hash, path, source, received_at, status)
             VALUES (1, zeroblob(32), 'inbox/a.pdf', 'inbox', 1700000000, 'planned');
             INSERT INTO document_field_origin (document_id, field, origin, rule_id)
             VALUES (1, 'kind', 'rule', 8);",
        )
        .unwrap();
    drop(old);

    let index = Index::open(&path, &InMemoryKeyStore::empty()).unwrap();

    assert_eq!(user_version(&index), latest_version());
    let rules = index.rules().unwrap();
    let ids: Vec<_> = rules.iter().map(|rule| rule.id).collect();
    assert_eq!(ids, [RuleId::new(3), RuleId::new(5), RuleId::new(8)]);
    let conditions: Vec<_> = rules.iter().map(|rule| &rule.spec.conditions).collect();
    assert_eq!(
        conditions,
        [
            &RuleConditions::all_of(vec![Condition::new(ConditionKind::Substring, "Invoice")]),
            &RuleConditions::all_of(vec![Condition::new(ConditionKind::Regex, "Invoice \\d+")]),
            &RuleConditions::all_of(vec![Condition::new(ConditionKind::Sender, "Company A")]),
        ]
    );
    assert!(rules
        .iter()
        .all(|rule| rule.spec.conditions.combinator == Combinator::AllOf));
    assert!(rules[0].spec.confirmed && !rules[1].spec.confirmed);
    let sender_rule = index.rule(RuleId::new(8)).unwrap().unwrap();
    assert_eq!(sender_rule.spec.tags.len(), 1);
    assert!(sender_rule.spec.tags.contains("services"));
    assert!(index.rule_evidence(RuleId::new(8)).unwrap().is_some());
    let origins = index.field_origins(crate::DocumentId::new(1)).unwrap();
    assert_eq!(
        origins.of(crate::model::Field::Kind),
        FieldOrigin::Rule(Some(RuleId::new(8)))
    );
    let violations = index
        .conn
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .count();
    assert_eq!(violations, 0);
}

#[test]
fn a_version_11_database_gains_sender_aliases() {
    use crate::model::{Condition, ConditionKind, RuleConditions, RuleId, SenderAliasKind};

    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let old = Index::create_plain_with_migrations(&path, &MIGRATIONS[..11]).unwrap();
    assert_eq!(user_version(&old), 11);
    old.conn
        .execute_batch(
            "INSERT INTO sender (id, name, name_key) VALUES (1, 'Company A', 'company a');
             INSERT INTO document (id, hash, path, source, received_at, status, sender_id)
             VALUES (1, zeroblob(32), 'inbox/a.pdf', 'inbox', 1700000000, 'planned', 1);
             INSERT INTO document_kind (id, name, name_key) VALUES (1, 'Invoice', 'invoice');
             INSERT INTO rule (id, kind_id, tax_context, filing_pattern, confirmed)
             VALUES (4, 1, 'none', '{year}', 1);
             INSERT INTO rule_condition (rule_id, position, kind, value)
             VALUES (4, 0, 'sender', 'Company A');",
        )
        .unwrap();
    drop(old);

    let index = Index::open(&path, &InMemoryKeyStore::empty()).unwrap();

    assert_eq!(user_version(&index), latest_version());
    let document = index.document(crate::DocumentId::new(1)).unwrap().unwrap();
    assert_eq!(document.fields.sender.as_deref(), Some("Company A"));
    let rule = index.rule(RuleId::new(4)).unwrap().unwrap();
    assert_eq!(
        rule.spec.conditions,
        RuleConditions::all_of(vec![Condition::new(ConditionKind::Sender, "Company A")])
    );
    assert!(index.sender_aliases().unwrap().is_empty());
    assert!(index
        .offer_sender_alias("Company A", SenderAliasKind::Domain, "company-a.example")
        .unwrap());
    let violations = index
        .conn
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .count();
    assert_eq!(violations, 0);
}

#[test]
fn a_version_12_database_keeps_its_rule_order_and_enables_every_confirmed_rule() {
    use crate::model::{Field, FieldOrigin, RuleId};

    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let old = Index::create_plain_with_migrations(&path, &MIGRATIONS[..12]).unwrap();
    assert_eq!(user_version(&old), 12);
    old.conn
        .execute_batch(
            "INSERT INTO document_kind (id, name, name_key) VALUES (1, 'Invoice', 'invoice');
             INSERT INTO rule (id, kind_id, tax_context, filing_pattern, confirmed)
             VALUES (7, 1, 'none', '{year}', 1),
                    (2, 1, 'none', '{year}', 1),
                    (5, 1, 'none', '{year}', 0);
             INSERT INTO rule_condition (rule_id, position, kind, value)
             VALUES (7, 0, 'substring', 'a'), (2, 0, 'substring', 'b'),
                    (5, 0, 'substring', 'c');
             INSERT INTO document (id, hash, path, source, received_at, status)
             VALUES (1, zeroblob(32), 'inbox/a.pdf', 'inbox', 1700000000, 'planned');
             INSERT INTO document_field_origin (document_id, field, origin, rule_id)
             VALUES (1, 'kind', 'rule', 5);",
        )
        .unwrap();
    drop(old);

    let index = Index::open(&path, &InMemoryKeyStore::empty()).unwrap();

    assert_eq!(user_version(&index), latest_version());
    let rules = index.rules().unwrap();
    let ids: Vec<_> = rules.iter().map(|rule| rule.id).collect();
    assert_eq!(ids, [RuleId::new(2), RuleId::new(5), RuleId::new(7)]);
    let positions: Vec<_> = rules.iter().map(|rule| rule.position).collect();
    assert_eq!(positions, [2, 5, 7]);
    // Step 037 switches the unconfirmed rule 5 off.
    assert!(rules
        .iter()
        .all(|rule| rule.spec.enabled == rule.spec.confirmed));
    let origins = index.field_origins(crate::DocumentId::new(1)).unwrap();
    assert_eq!(
        origins.of(Field::Kind),
        FieldOrigin::Rule(Some(RuleId::new(5)))
    );
    let violations = index
        .conn
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .count();
    assert_eq!(violations, 0);
}

#[test]
fn a_version_13_database_keeps_its_discarded_suggestions_as_word_suggestions() {
    use crate::model::{
        Condition, ConditionKind, RuleConditions, RuleId, RuleSpec, RuleSuggestion,
    };

    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let old = Index::create_plain_with_migrations(&path, &MIGRATIONS[..13]).unwrap();
    assert_eq!(user_version(&old), 13);
    old.conn
        .execute_batch(
            "INSERT INTO discarded_rule_suggestion
                 (match_text, kind, filing_pattern, discarded_at)
             VALUES ('company a ltd', 'insurance', '{year}', 1700000000);
             INSERT INTO document_kind (id, name, name_key) VALUES (1, 'Insurance', 'insurance');
             INSERT INTO rule (id, kind_id, tax_context, filing_pattern, confirmed)
             VALUES (3, 1, 'none', 'Insurance/{date}', 0);
             INSERT INTO rule_condition (rule_id, position, kind, value)
             VALUES (3, 0, 'word', 'agency b');
             INSERT INTO rule_evidence
                 (rule_id, matching_in_folder, folder_documents, matching_elsewhere, updated_at)
             VALUES (3, 5, 5, 0, 1700000000);",
        )
        .unwrap();
    drop(old);

    let mut index = Index::open(&path, &InMemoryKeyStore::empty()).unwrap();

    assert_eq!(user_version(&index), latest_version());
    let evidence = index.rule_evidence(RuleId::new(3)).unwrap().unwrap();
    assert_eq!(evidence.matching_in_folder, 5);
    assert_eq!(evidence.subtree, None);
    let suggestion = |kind| RuleSpec {
        name: None,
        conditions: RuleConditions::all_of(vec![Condition::new(kind, "Company A Ltd")]),
        extract_number: None,
        extract_date: None,
        extract_amount: None,
        kind: Some("Insurance".to_owned()),
        tags: Default::default(),
        filing_pattern: "{year}/".to_owned(),
        confirmed: false,
        enabled: true,
    };
    assert_eq!(
        index
            .suggest_rule(&suggestion(ConditionKind::Word), evidence.clone())
            .unwrap(),
        RuleSuggestion::Discarded
    );
    assert!(matches!(
        index
            .suggest_rule(&suggestion(ConditionKind::Sender), evidence)
            .unwrap(),
        RuleSuggestion::Created(_)
    ));
}

#[test]
fn a_version_14_database_keeps_its_sender_aliases_and_offers_a_discarded_value_to_another_sender() {
    use crate::model::{SenderAliasKind, SenderAliasState};

    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let old = Index::create_plain_with_migrations(&path, &MIGRATIONS[..14]).unwrap();
    assert_eq!(user_version(&old), 14);
    old.conn
        .execute_batch(
            "INSERT INTO sender (id, name, name_key) VALUES (1, 'A GmbH', 'a gmbh');
             INSERT INTO sender (id, name, name_key) VALUES (2, 'B GmbH', 'b gmbh');
             INSERT INTO sender_alias (id, sender_id, kind, value, state)
             VALUES (7, 1, 'domain', 'a.example', 'offered');
             INSERT INTO sender_alias (id, sender_id, kind, value, state)
             VALUES (9, 1, 'domain', 'shared.example', 'discarded');",
        )
        .unwrap();
    drop(old);

    let index = Index::open(&path, &InMemoryKeyStore::empty()).unwrap();

    assert_eq!(user_version(&index), latest_version());
    let aliases = index.sender_aliases().unwrap();
    let kept: Vec<_> = aliases
        .iter()
        .map(|alias| (alias.id.get(), alias.sender.as_str(), alias.state))
        .collect();
    assert_eq!(
        kept,
        [
            (7, "A GmbH", SenderAliasState::Offered),
            (9, "A GmbH", SenderAliasState::Discarded)
        ]
    );
    assert!(!index
        .offer_sender_alias("A GmbH", SenderAliasKind::Domain, "shared.example")
        .unwrap());
    assert!(index
        .offer_sender_alias("B GmbH", SenderAliasKind::Domain, "shared.example")
        .unwrap());
    assert!(!index
        .offer_sender_alias("B GmbH", SenderAliasKind::Domain, "a.example")
        .unwrap());
}

#[test]
fn a_version_17_database_gains_the_origin_of_its_rules() {
    use crate::model::{RuleId, RuleOrigin};

    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let old = Index::create_plain_with_migrations(&path, &MIGRATIONS[..17]).unwrap();
    assert_eq!(user_version(&old), 17);
    old.conn
        .execute_batch(
            "INSERT INTO document_kind (id, name, name_key) VALUES (1, 'Invoice', 'invoice');
             INSERT INTO rule (id, kind_id, tax_context_id, filing_pattern, confirmed)
             VALUES (1, 1, 0, '{year}', 0), (2, 1, 0, '{year}', 1);
             INSERT INTO rule_condition (rule_id, position, kind, value)
             VALUES (1, 0, 'word', 'rent'), (2, 0, 'word', 'tax');
             INSERT INTO rule_evidence
                 (rule_id, matching_in_folder, folder_documents, matching_elsewhere, updated_at)
             VALUES (1, 3, 3, 0, 1700000000);",
        )
        .unwrap();
    drop(old);

    let index = Index::open(&path, &InMemoryKeyStore::empty()).unwrap();

    assert_eq!(user_version(&index), latest_version());
    let origin_of = |id: i64| index.rule(RuleId::new(id)).unwrap().unwrap().origin;
    assert_eq!(origin_of(1), RuleOrigin::Suggestion);
    assert_eq!(origin_of(2), RuleOrigin::Filing);
}

/// A version 19 database with the data a tax context touched, opened so
/// that migration 020 runs.
fn migrated_from_19() -> (TempDir, Index) {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let old = Index::create_plain_with_migrations(&path, &MIGRATIONS[..19]).unwrap();
    assert_eq!(user_version(&old), 19);
    old.conn
        .execute_batch(
            "INSERT INTO tax_context (id, name, short_name, combinator)
                 VALUES (3, 'Firma A', 'firma-a', 'any'), (4, 'Firma B', 'firma-b', 'any');
             INSERT INTO tax_context_condition (tax_context_id, position, kind, value)
                 VALUES (3, 0, 'recipient', 'Firma A GmbH'), (4, 0, 'recipient', 'Firma B');
             INSERT INTO sender (id, name, name_key) VALUES (1, 'Company A', 'company a');
             INSERT INTO document_kind (id, name, name_key) VALUES (1, 'Invoice', 'invoice');
             INSERT INTO tag (id, name, name_key) VALUES (1, 'services', 'services');
             INSERT INTO document (id, hash, path, source, received_at, status, sender_id,
                     kind_id, tax_context_id, document_date, document_number, amount_minor,
                     currency, file_size, file_mtime)
                 VALUES (1, zeroblob(32), 'a.pdf', 'inbox', 1700000000, 'in_review', 1, 1, 3,
                     '2026-02-28', '4711', 11900, 'EUR', 10, 20),
                     (2, randomblob(32), 'b.pdf', 'inbox', 1700000000, 'in_review', NULL, NULL,
                     NULL, NULL, NULL, NULL, NULL, NULL, NULL),
                     (3, randomblob(32), 'c.pdf', 'inbox', 1700000000, 'planned', NULL, NULL,
                     4, NULL, NULL, NULL, NULL, NULL, NULL);
             INSERT INTO document_text (rowid, body) VALUES (1, 'Rechnung fuer die Miete');
             INSERT INTO document_tag (document_id, tag_id) VALUES (1, 1);
             INSERT INTO rule (id, kind_id, tax_context_id, filing_pattern, confirmed)
                 VALUES (3, 1, 3, '{tax_context}/{year}/{sender}', 1),
                        (4, 1, 4, '{tax_context}', 0),
                        (5, 1, 0, '{year}/{number}', 1),
                        (6, 1, 0, '{year}/{year}{month}{day}_{sender}', 1);
             INSERT INTO rule_condition (rule_id, position, kind, value)
                 VALUES (3, 0, 'word', 'rent'), (4, 0, 'word', 'tax'),
                        (5, 0, 'word', 'a'), (6, 0, 'word', 'b');
             INSERT INTO document_field_origin
                     (document_id, field, origin, rule_id, confidence, condition_kind,
                      condition_value)
                 VALUES (1, 'sender', 'rule', 3, NULL, NULL, NULL),
                        (1, 'tax_context', 'condition', NULL, NULL, 'recipient', 'Firma A GmbH');
             INSERT INTO review_entry (id, document_id, reason, field, created_at, resolved_at)
                 VALUES (1, 1, 'missing_field', 'tax_context', 1700000000, NULL),
                        (2, 2, 'no_tax_context_matches', NULL, 1700000000, NULL),
                        (3, 3, 'several_tax_contexts_match', NULL, 1700000000, 1700000100),
                        (4, 2, 'rule_conflict', NULL, 1700000000, 1700000200);
             INSERT INTO discarded_rule_suggestion
                     (match_text, condition_kind, kind, filing_pattern, discarded_at)
                 VALUES ('company a ltd', 'word', 'insurance', '{tax_context}/{year}', 1),
                        ('company a ltd', 'word', 'insurance', '{year}', 2);
             INSERT INTO location_profile (location, learned_at) VALUES ('archive', 1);
             INSERT INTO location_subtree (location, subtree, pattern, documents, total)
                 VALUES ('archive', 'a', '{year}/{tax_context}', 3, 3),
                        ('archive', 'b', NULL, 1, 1),
                        ('archive', 'c', '{year}/{date}_{sender}', 2, 2);
             INSERT INTO saved_query (id, name, year, tax_context_id, kind, created_at, sender)
                 VALUES (7, 'Firma A only', 2026, 3, 'Invoice', 1700000000, 'Company A');",
        )
        .unwrap();
    drop(old);

    let index = Index::open(&path, &InMemoryKeyStore::empty()).unwrap();
    (dir, index)
}

#[test]
fn a_version_20_database_gains_an_empty_file_list_tag_table() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let old = Index::create_plain_with_migrations(&path, &MIGRATIONS[..20]).unwrap();
    assert_eq!(user_version(&old), 20);
    old.conn
        .execute_batch(
            "INSERT INTO tag (id, name, name_key) VALUES (1, 'steuer', 'steuer');
             INSERT INTO document (id, hash, path, source, received_at, status)
                 VALUES (1, zeroblob(32), 'a.pdf', 'inbox', 1700000000, 'in_review');
             INSERT INTO document_tag (document_id, tag_id, automatic) VALUES (1, 1, 0);",
        )
        .unwrap();
    drop(old);

    let index = Index::open(&path, &InMemoryKeyStore::empty()).unwrap();

    assert_eq!(user_version(&index), latest_version());
    assert!(index.file_list_tag_exceptions().unwrap().is_empty());
    index
        .set_file_list_tag(crate::TagGroup::Own, "steuer", Some(false))
        .unwrap();
    assert_eq!(index.file_list_tag_exceptions().unwrap().len(), 1);
    assert_eq!(
        index.tag_tally().unwrap().own,
        vec![("steuer".to_owned(), 1)]
    );
}

#[test]
fn a_version_19_database_loses_its_tax_context_tables_and_columns() {
    use crate::model::RuleId;

    let (_dir, index) = migrated_from_19();

    assert_eq!(user_version(&index), latest_version());
    let violations = index
        .conn
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .count();
    assert_eq!(violations, 0);
    assert!(!table_exists(&index, "tax_context"));
    assert!(!table_exists(&index, "tax_context_condition"));
    for (table, column) in [
        ("document", "tax_context_id"),
        ("rule", "tax_context_id"),
        ("saved_query", "tax_context_id"),
        ("document_field_origin", "condition_kind"),
    ] {
        assert!(!has_column(&index, table, column), "{table}.{column}");
    }
    assert!(has_column(&index, "document", "recipient"));
    assert!(has_column(&index, "rule", "name"));

    let rules = index.rules().unwrap();
    assert_eq!(
        rules.iter().map(|rule| rule.id).collect::<Vec<_>>(),
        [
            RuleId::new(3),
            RuleId::new(4),
            RuleId::new(5),
            RuleId::new(6)
        ]
    );
    assert_eq!(rules[0].spec.filing_pattern, "{year}/{sender}/");
    assert_eq!(rules[1].spec.filing_pattern, "{year}/");
    assert_eq!(rules[2].spec.filing_pattern, "{year}/{number}");
    assert_eq!(
        rules[3].spec.filing_pattern,
        "{year}/{year}{month}{day}_{sender}"
    );
    assert_eq!(rules[0].spec.conditions.list.len(), 1);
    index
        .conn
        .execute("UPDATE rule SET kind_id = NULL WHERE id = 3", [])
        .unwrap();
    let open_kind: Option<i64> = index
        .conn
        .query_row("SELECT kind_id FROM rule WHERE id = 3", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(open_kind, None);
}

#[test]
fn a_version_19_database_keeps_its_discarded_suggestions_and_turns_reviews_into_no_rule() {
    use crate::model::{Field, FieldOrigin, ReviewReason, RuleId};

    let (_dir, index) = migrated_from_19();

    let kept: i64 = index
        .conn
        .query_row(
            "SELECT count(*) FROM discarded_rule_suggestion",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(kept, 1);
    let subtree_patterns: Vec<Option<String>> = index
        .conn
        .prepare("SELECT pattern FROM location_subtree ORDER BY subtree")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        subtree_patterns,
        [
            Some("{year}/".to_owned()),
            None,
            Some("{year}/{date}_{sender}".to_owned())
        ]
    );

    let reasons: Vec<ReviewReason> = index
        .conn
        .prepare("SELECT reason, field FROM review_entry ORDER BY id")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .unwrap()
        .map(|row| {
            let (reason, field) = row.unwrap();
            ReviewReason::parse(&reason, field.as_deref()).unwrap()
        })
        .collect();
    assert_eq!(reasons, [ReviewReason::NoMatchingRule; 4]);
    let open: Vec<_> = index
        .open_reviews()
        .unwrap()
        .into_iter()
        .map(|entry| (entry.document_id, entry.reason))
        .collect();
    assert!(open.is_empty(), "{open:?}");

    let origins = index.field_origins(crate::DocumentId::new(1)).unwrap();
    assert_eq!(
        origins.of(Field::Sender),
        FieldOrigin::Rule(Some(RuleId::new(3)))
    );
    assert_eq!(origins.of(Field::Kind), FieldOrigin::Unknown);
    let origin_rows: i64 = index
        .conn
        .query_row("SELECT count(*) FROM document_field_origin", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(origin_rows, 1);
}

#[test]
fn a_version_19_database_keeps_the_fields_tags_and_reports_of_its_documents() {
    let (_dir, index) = migrated_from_19();

    let document = index.document(crate::DocumentId::new(1)).unwrap().unwrap();
    assert_eq!(document.fields.sender.as_deref(), Some("Company A"));
    assert_eq!(document.fields.number.as_deref(), Some("4711"));
    assert!(document.fields.tags.contains("services"));
    let automatic: i64 = index
        .conn
        .query_row("SELECT sum(automatic) FROM document_tag", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(automatic, 0);
    let queries = index.saved_queries().unwrap();
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].spec.year, Some(2026));
    assert_eq!(queries[0].spec.sender.as_deref(), Some("Company A"));
}

#[test]
fn a_version_19_database_keeps_the_text_search_and_the_cascade_of_its_documents() {
    let (_dir, index) = migrated_from_19();

    assert_eq!(
        index.search_text("miete", 10).unwrap(),
        [crate::DocumentId::new(1)]
    );
    let by_path: i64 = index
        .conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name = 'document_by_path'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(by_path, 1);
    index
        .conn
        .execute("DELETE FROM document WHERE id = 1", [])
        .unwrap();
    let text_left: i64 = index
        .conn
        .query_row("SELECT count(*) FROM document_text", [], |row| row.get(0))
        .unwrap();
    assert_eq!(text_left, 0);
}

#[test]
fn a_migration_on_open_leaves_a_copy_of_the_file_at_its_old_version_beside_it() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let store = InMemoryKeyStore::empty();
    let older = MIGRATIONS.len() - 1;
    drop(Index::create_plain_with_migrations(&path, &MIGRATIONS[..older]).unwrap());

    let index = Index::open(&path, &store).unwrap();

    assert_eq!(
        user_version(&index),
        u32::try_from(MIGRATIONS.len()).unwrap()
    );
    let copies = crate::backup::migration_backups(&path);
    assert_eq!(copies.len(), 1);
    assert!(copies[0]
        .to_string_lossy()
        .ends_with(&format!(".before-v{older}")));
    let copy = rusqlite::Connection::open(&copies[0]).unwrap();
    let copied_version: i64 = copy
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(copied_version, i64::try_from(older).unwrap());
}

#[test]
fn a_sound_index_reports_no_damage() {
    let dir = TempDir::new().unwrap();
    let index = Index::create_plain(&db_in(&dir)).unwrap();
    assert_eq!(index.damage().unwrap(), None);
}

const PATH_EXAMPLES: [(&str, &str); 6] = [
    (r"\\?\C:\a", r"C:\a"),
    (r"\\?\c:\a", r"c:\a"),
    (r"\\?\UNC\s\x", r"\\s\x"),
    (r"\\?\Volume{1}\x", r"\\?\Volume{1}\x"),
    (r"C:\a", r"C:\a"),
    ("/data", "/data"),
];

/// Like `PATH_EXAMPLES`, but no plain path is the stripped form of another
/// row, as the columns with a uniqueness rule need.
const DISTINCT_PATH_EXAMPLES: [(&str, &str); 6] = [
    (r"\\?\C:\a", r"C:\a"),
    (r"\\?\d:\b", r"d:\b"),
    (r"\\?\UNC\s\x", r"\\s\x"),
    (r"\\?\Volume{1}\x", r"\\?\Volume{1}\x"),
    (r"E:\c", r"E:\c"),
    ("/data", "/data"),
];

fn version_27_index(dir: &TempDir) -> Index {
    let old = Index::create_plain_with_migrations(&db_in(dir), &MIGRATIONS[..27]).unwrap();
    assert_eq!(user_version(&old), 27);
    old
}

fn column_values(index: &Index, sql: &str) -> Vec<String> {
    let mut statement = index.conn.prepare(sql).unwrap();
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap();
    rows.map(Result::unwrap).collect()
}

#[test]
fn a_version_27_database_gets_the_paths_the_app_now_compares() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let store = InMemoryKeyStore::empty();
    let old = version_27_index(&dir);
    for (seed, (given, _)) in PATH_EXAMPLES.iter().enumerate() {
        let batch = i64::try_from(seed).unwrap() + 1;
        old.conn
            .execute(
                "INSERT INTO execution_batch
                    (id, kind, started_at, source_root, source_resolved, target_root, target_resolved)
                 VALUES (?1, 'filing', 1, ?2, ?2, ?2, ?2)",
                rusqlite::params![batch, given],
            )
            .unwrap();
        old.conn
            .execute(
                "INSERT INTO execution_item (batch_id, seq, from_path, to_path, hash, duplicate, state)
                 VALUES (?1, 1, ?2, ?2, zeroblob(32), 0, 'moved')",
                rusqlite::params![batch, given],
            )
            .unwrap();
    }
    for (seed, (given, _)) in DISTINCT_PATH_EXAMPLES.iter().enumerate() {
        old.conn
            .execute(
                "INSERT INTO document (hash, path, source, received_at, status)
                 VALUES (randomblob(32), ?1, 'inbox', 1, 'new')",
                [given],
            )
            .unwrap();
        old.conn
            .execute(
                "INSERT INTO duplicate_path (document_id, path, seen_at) VALUES (1, ?1, 1)",
                [given],
            )
            .unwrap();
        old.conn
            .execute("INSERT INTO excluded_path (path) VALUES (?1)", [given])
            .unwrap();
        old.conn
            .execute(
                "INSERT INTO execution_created_dir (batch_id, path) VALUES (?1, ?2)",
                rusqlite::params![i64::try_from(seed).unwrap() + 1, given],
            )
            .unwrap();
    }
    drop(old);

    let index = Index::open(&path, &store).unwrap();

    let converted = |examples: &[(&str, &str)]| -> Vec<String> {
        examples
            .iter()
            .map(|(_, plain)| (*plain).to_owned())
            .collect()
    };
    for sql in [
        "SELECT from_path FROM execution_item ORDER BY batch_id",
        "SELECT to_path FROM execution_item ORDER BY batch_id",
        "SELECT source_root FROM execution_batch ORDER BY id",
        "SELECT source_resolved FROM execution_batch ORDER BY id",
        "SELECT target_root FROM execution_batch ORDER BY id",
        "SELECT target_resolved FROM execution_batch ORDER BY id",
    ] {
        assert_eq!(
            column_values(&index, sql),
            converted(&PATH_EXAMPLES),
            "{sql}"
        );
    }
    for sql in [
        "SELECT path FROM document ORDER BY id",
        "SELECT path FROM duplicate_path ORDER BY id",
        "SELECT path FROM excluded_path ORDER BY rowid",
        "SELECT path FROM execution_created_dir ORDER BY batch_id",
    ] {
        assert_eq!(
            column_values(&index, sql),
            converted(&DISTINCT_PATH_EXAMPLES),
            "{sql}"
        );
    }
}

#[test]
fn a_version_27_database_loses_no_row_when_a_prefixed_and_a_plain_path_meet() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let store = InMemoryKeyStore::empty();
    let old = version_27_index(&dir);
    for given in [r"\\?\C:\a", r"C:\a"] {
        old.conn
            .execute(
                "INSERT INTO document (hash, path, source, received_at, status)
                 VALUES (randomblob(32), ?1, 'inbox', 1, 'new')",
                [given],
            )
            .unwrap();
        old.conn
            .execute(
                "INSERT INTO duplicate_path (document_id, path, seen_at) VALUES (1, ?1, 1)",
                [given],
            )
            .unwrap();
        old.conn
            .execute("INSERT INTO excluded_path (path) VALUES (?1)", [given])
            .unwrap();
    }
    old.conn
        .execute(
            "INSERT INTO execution_batch (id, kind, started_at) VALUES (1, 'filing', 1)",
            [],
        )
        .unwrap();
    for given in [r"\\?\C:\a", r"C:\a"] {
        old.conn
            .execute(
                "INSERT INTO execution_created_dir (batch_id, path) VALUES (1, ?1)",
                [given],
            )
            .unwrap();
    }
    drop(old);

    let index = Index::open(&path, &store).unwrap();

    assert_eq!(
        column_values(&index, "SELECT path FROM document ORDER BY id"),
        [r"\\?\C:\a", r"C:\a"]
    );
    for sql in [
        "SELECT path FROM duplicate_path",
        "SELECT path FROM excluded_path",
        "SELECT path FROM execution_created_dir",
    ] {
        assert_eq!(column_values(&index, sql), [r"C:\a"], "{sql}");
    }
}
