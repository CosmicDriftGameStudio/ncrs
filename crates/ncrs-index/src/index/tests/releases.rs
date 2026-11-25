use super::super::*;
use super::keys::*;
use crate::key_store::InMemoryKeyStore;
use crate::model::{DocumentId, FileStamp, ReviewReason};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

pub(in crate::index::tests) const FIXTURE_V0_6_0: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/index-v0.6.0.db"
);
pub(in crate::index::tests) const PROBE_COLUMN: &str =
    "ALTER TABLE document ADD COLUMN probe INTEGER";

pub(in crate::index::tests) const FIXTURE_V1_2_0: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/index-v1.2.0.db"
);

/// A copy of an index a release wrote, and the store holding the key it was
/// written with (bytes 0x00 to 0x1f).
pub(in crate::index::tests) fn copy_of_fixture(
    fixture: &str,
) -> (TempDir, PathBuf, InMemoryKeyStore) {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    fs::copy(fixture, &path).unwrap();
    let key: Vec<u8> = (0..32).collect();
    (dir, path, InMemoryKeyStore::with_entry(key))
}

pub(in crate::index::tests) fn copy_of_the_v0_6_0_index() -> (TempDir, PathBuf, InMemoryKeyStore) {
    copy_of_fixture(FIXTURE_V0_6_0)
}

pub(in crate::index::tests) fn copy_of_the_v1_2_0_index() -> (TempDir, PathBuf, InMemoryKeyStore) {
    copy_of_fixture(FIXTURE_V1_2_0)
}

pub(in crate::index::tests) const FIXTURE_V1_5_1: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/index-v1.5.1.db"
);

pub(in crate::index::tests) fn copy_of_the_v1_5_1_index() -> (TempDir, PathBuf, InMemoryKeyStore) {
    copy_of_fixture(FIXTURE_V1_5_1)
}

pub(in crate::index::tests) const FIXTURE_V1_6_0: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/index-v1.6.0.db"
);

pub(in crate::index::tests) fn copy_of_the_v1_6_0_index() -> (TempDir, PathBuf, InMemoryKeyStore) {
    copy_of_fixture(FIXTURE_V1_6_0)
}

pub(in crate::index::tests) const FIXTURE_V1_7_0: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/index-v1.7.0.db"
);

pub(in crate::index::tests) fn copy_of_the_v1_7_0_index() -> (TempDir, PathBuf, InMemoryKeyStore) {
    copy_of_fixture(FIXTURE_V1_7_0)
}

pub(in crate::index::tests) const FIXTURE_V0_6_0_VERSION: u32 = 9;
pub(in crate::index::tests) const FIXTURE_V1_2_0_VERSION: u32 = 25;
pub(in crate::index::tests) const FIXTURE_V1_5_1_VERSION: u32 = 26;
pub(in crate::index::tests) const FIXTURE_V1_6_0_VERSION: u32 = 35;
pub(in crate::index::tests) const FIXTURE_V1_7_0_VERSION: u32 = 36;

pub(in crate::index::tests) fn latest_version() -> u32 {
    u32::try_from(MIGRATIONS.len()).unwrap()
}

pub(in crate::index::tests) fn probe_version() -> u32 {
    latest_version() + 1
}

pub(in crate::index::tests) fn migrations_and(extra: &[&'static str]) -> Vec<&'static str> {
    MIGRATIONS.iter().chain(extra).copied().collect()
}

pub(in crate::index::tests) fn has_column(index: &Index, table: &str, column: &str) -> bool {
    index
        .conn
        .query_row(
            &format!("SELECT count(*) FROM pragma_table_info('{table}') WHERE name = ?1"),
            [column],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
        == 1
}

pub(in crate::index::tests) fn assert_v0_6_0_content(index: &Index) {
    use crate::model::{DocumentSource, DocumentStatus};

    assert!(index.is_encrypted());
    let invoice = index
        .document_by_path(Path::new("archive/2026/Company A/invoice-4711.pdf"))
        .unwrap()
        .unwrap();
    assert_eq!(invoice.id.get(), 1);
    assert_eq!(invoice.source, DocumentSource::Archive);
    assert_eq!(invoice.status, DocumentStatus::Filed);
    assert_eq!(invoice.fields.sender.as_deref(), Some("Company A"));
    assert_eq!(invoice.fields.kind.as_deref(), Some("Invoice"));
    assert_eq!(invoice.fields.number.as_deref(), Some("4711"));
    assert_eq!(
        invoice.fields.date.map(|date| date.to_string()).as_deref(),
        Some("2026-01-15")
    );
    let amount = invoice.fields.amount.clone().unwrap();
    assert_eq!(
        (amount.minor_units, amount.currency.as_str()),
        (11900, "EUR")
    );
    assert_eq!(
        invoice
            .fields
            .tags
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["2026", "services"]
    );

    let receipt = index
        .document_by_path(Path::new("inbox/scan-2.pdf"))
        .unwrap()
        .unwrap();
    assert_eq!((receipt.id.get(), receipt.status), (2, DocumentStatus::New));
    let notice = index
        .document_by_path(Path::new("inbox/scan-3.pdf"))
        .unwrap()
        .unwrap();
    assert_eq!((notice.id.get(), notice.status), (3, DocumentStatus::New));

    assert_v0_6_0_search_and_other_tables(index, &invoice, &receipt, &notice);
}

pub(in crate::index::tests) fn assert_v0_6_0_search_and_other_tables(
    index: &Index,
    invoice: &crate::model::Document,
    receipt: &crate::model::Document,
    notice: &crate::model::Document,
) {
    use crate::model::{Condition, ConditionKind, RuleConditions};

    assert_eq!(index.search_text("hosting", 10).unwrap(), [invoice.id]);
    assert_eq!(index.search_text("chair", 10).unwrap(), [receipt.id]);
    assert_eq!(index.search_text("Finanzamt", 10).unwrap(), [notice.id]);
    assert!(index.search_text("nonexistentword", 10).unwrap().is_empty());

    let rules = index.rules().unwrap();
    assert_eq!(rules.len(), 1);
    let rule = &rules[0].spec;
    assert_eq!(
        rule.conditions,
        RuleConditions::all_of(vec![
            Condition::new(ConditionKind::Substring, "Invoice"),
            Condition::new(ConditionKind::Sender, "Company A"),
        ])
    );
    assert_eq!(rule.filing_pattern, "{year}/{sender}/");
    assert!(rule.confirmed);

    assert!(index.open_reviews().unwrap().is_empty());

    let learned_pattern: String = index
        .connection()
        .query_row(
            "SELECT pattern FROM location_subtree WHERE location = 'archive'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(learned_pattern, "{year}/{sender}/");

    let queries = index.saved_queries().unwrap();
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].name, "Invoices 2026");
    assert_eq!(queries[0].spec.year, Some(2026));
    assert_eq!(queries[0].spec.full_text.as_deref(), Some("hosting"));
}

#[test]
fn an_index_written_by_release_0_6_0_opens_with_all_its_content() {
    let (_dir, path, store) = copy_of_the_v0_6_0_index();

    let index = Index::open_with_migrations(&path, &store, MIGRATIONS).unwrap();

    assert_eq!(
        user_version(&index),
        u32::try_from(MIGRATIONS.len()).unwrap()
    );
    assert_v0_6_0_content(&index);
    let invoice = index
        .document_by_path(Path::new("archive/2026/Company A/invoice-4711.pdf"))
        .unwrap()
        .unwrap();
    let origins = index.field_origins(invoice.id).unwrap();
    for field in crate::model::Field::ALL {
        assert_eq!(origins.of(field), crate::model::FieldOrigin::Unknown);
    }
}

#[test]
fn a_later_step_migrates_the_release_0_6_0_index_and_keeps_its_content() {
    let (_dir, path, store) = copy_of_the_v0_6_0_index();
    let steps = migrations_and(&[PROBE_COLUMN]);

    let index = Index::open_with_migrations(&path, &store, &steps).unwrap();

    assert_eq!(user_version(&index), probe_version());
    assert!(has_column(&index, "document", "probe"));
    assert_v0_6_0_content(&index);
    drop(index);
    let reopened = Index::open_with_migrations(&path, &store, &steps).unwrap();
    assert_eq!(user_version(&reopened), probe_version());
}

#[test]
fn a_failing_second_step_leaves_the_release_0_6_0_file_byte_for_byte_as_it_was() {
    let (_dir, path, store) = copy_of_the_v0_6_0_index();
    let before = fs::read(&path).unwrap();
    let steps = migrations_and(&[PROBE_COLUMN, "CREATE TABLE document (x)"]);

    let err = Index::open_with_migrations(&path, &store, &steps).unwrap_err();

    assert!(
        matches!(
            err,
            IndexError::MigrationFailed { from, step, .. }
                if from == FIXTURE_V0_6_0_VERSION && step == latest_version() + 2
        ),
        "{err}"
    );
    assert!(err.to_string().contains("left unchanged"), "{err}");
    assert_eq!(fs::read(&path).unwrap(), before);
    let index = Index::open_with_migrations(&path, &store, MIGRATIONS).unwrap();
    assert_eq!(user_version(&index), latest_version());
    assert!(!has_column(&index, "document", "probe"));
    assert_v0_6_0_content(&index);
}

pub(in crate::index::tests) fn tag_set(names: &[&str]) -> std::collections::BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

pub(in crate::index::tests) fn assert_v1_2_0_content(index: &Index) {
    assert_v1_2_0_content_but_tag_exceptions(index);
    assert_eq!(
        index.file_list_tag_exceptions().unwrap(),
        crate::TagGroup::ALL
            .into_iter()
            .flat_map(|group| {
                [
                    ((group, "project-x".to_owned()), false),
                    ((group, "services".to_owned()), true),
                ]
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    );
}

fn assert_v1_2_0_content_but_tag_exceptions(index: &Index) -> DocumentId {
    let invoice = assert_v1_2_0_invoice(index);
    let (receipt, notice) = assert_v1_2_0_inbox_documents(index, invoice);
    assert_v1_2_0_rules(index);
    assert_v1_2_0_aliases_reviews_queries_and_tags(index, receipt, notice);
    assert_v1_2_0_reading_versions(index, invoice, notice);
    assert_v1_2_0_hermes_runs(index);
    invoice
}

fn assert_v1_6_0_content(index: &mut Index) {
    use crate::model::{Field, FieldOrigin};
    use crate::test_support::{new_document, rule_spec};
    use crate::TagGroup;
    use std::time::{Duration, UNIX_EPOCH};

    let stamp = |size: u64, seconds: u64| FileStamp {
        size,
        mtime: UNIX_EPOCH + Duration::from_secs(seconds),
    };
    let invoice = assert_v1_2_0_content_but_tag_exceptions(index);

    let blank = index
        .document_by_path(Path::new("inbox/scan-5.pdf"))
        .unwrap()
        .unwrap();
    let has_text: Vec<(String, i64)> = index
        .conn
        .prepare("SELECT path, has_text FROM document WHERE path LIKE '%scan-%' ORDER BY path")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        has_text,
        [
            ("inbox/scan-2.pdf".to_owned(), 1),
            ("inbox/scan-3.pdf".to_owned(), 1),
            ("inbox/scan-5.pdf".to_owned(), 0),
        ]
    );
    assert_eq!(
        index.field_origins(blank.id).unwrap().of(Field::Number),
        FieldOrigin::Detected
    );

    assert_eq!(
        index.file_list_tag_exceptions().unwrap(),
        std::collections::BTreeMap::from([
            ((TagGroup::Own, "project-x".to_owned()), false),
            ((TagGroup::Own, "services".to_owned()), true),
            ((TagGroup::Sender, "services".to_owned()), false),
        ])
    );

    // The fixture was written on a machine that spells paths with `/`; an index
    // of this machine keeps them with its own separator.
    index
        .conn
        .execute(
            "UPDATE excluded_path SET path = replace(path, '/', ?1)",
            [std::path::MAIN_SEPARATOR_STR],
        )
        .unwrap();
    let judged_path = crate::test_support::native("inbox/logo.png");
    let judged = judged_path.as_path();
    let private_path = crate::test_support::native("inbox/private");
    let private = private_path.as_path();
    assert!(index.is_excluded(judged).unwrap() && index.is_excluded(private).unwrap());
    assert!(!index
        .lift_outdated_automatic_exclusion(private, stamp(1, 1))
        .unwrap());
    assert!(!index
        .lift_outdated_automatic_exclusion(judged, stamp(2048, 1_700_000_000))
        .unwrap());
    assert!(index
        .lift_outdated_automatic_exclusion(judged, stamp(2049, 1_700_000_000))
        .unwrap());
    assert!(!index.is_excluded(judged).unwrap() && index.is_excluded(private).unwrap());

    let filed = Path::new("inbox/invoice-4711.pdf");
    assert_eq!(
        index
            .filed_document_for_inbox_path(filed, stamp(4096, 1_700_000_100))
            .unwrap(),
        Some(invoice)
    );
    assert_eq!(
        index
            .filed_document_for_inbox_path(filed, stamp(1, 1_700_000_100))
            .unwrap(),
        None
    );

    for (path, in_archive) in [
        ("archive/copies/invoice-4711.pdf", Some(true)),
        ("inbox/invoice-4711-copy.pdf", Some(false)),
        ("elsewhere/invoice-4711.pdf", None),
    ] {
        assert_eq!(
            index.duplicate_path_in_archive(Path::new(path)).unwrap(),
            in_archive,
            "{path}"
        );
    }

    // Rule 5 (a dropped suggestion) and rule 7 are gone, and so is the review
    // entry of a deleted document: none of the numbers comes back.
    let next_rule = index.create_rule(&rule_spec()).unwrap();
    assert_eq!(next_rule.get(), 8);
    let fresh = index.insert_document(&new_document(9)).unwrap();
    assert_eq!(fresh.get(), 7);
    let next_entry = index
        .add_review(fresh, ReviewReason::NoMatchingRule)
        .unwrap();
    assert_eq!(next_entry.get(), 4);
}

pub(in crate::index::tests) fn assert_v1_2_0_reading_versions(
    index: &Index,
    invoice: DocumentId,
    notice: DocumentId,
) {
    assert_eq!(index.fields_read_version(invoice).unwrap(), 3);
    assert_eq!(index.ai_read_version(invoice).unwrap(), 2);
    assert_eq!(index.fields_read_version(notice).unwrap(), 0);
    assert_eq!(index.ai_read_version(notice).unwrap(), 1);
}

pub(in crate::index::tests) fn assert_v1_2_0_hermes_runs(index: &Index) {
    use crate::journal::{BatchKind, SourceRoot, Transfer};
    use crate::model::BatchId;

    let copy = index.batch_info(BatchId::new(1)).unwrap().unwrap();
    assert_eq!(copy.kind, BatchKind::Hermes);
    assert_eq!(copy.name.as_deref(), Some("Copy contracts"));
    assert_eq!(copy.transfer, Transfer::Copy);
    assert!(copy.finished_at.is_some() && copy.undone_at.is_none());
    assert_eq!(
        copy.source_root,
        Some(SourceRoot {
            given: PathBuf::from("inbox"),
            resolved: PathBuf::from("/data/inbox"),
        })
    );
    assert_eq!(
        copy.target_root,
        Some(SourceRoot {
            given: PathBuf::from("archive/2026"),
            resolved: PathBuf::from("/data/archive/2026"),
        })
    );
    let trash = index.batch_info(BatchId::new(2)).unwrap().unwrap();
    assert_eq!(trash.kind, BatchKind::Hermes);
    assert_eq!(trash.name.as_deref(), Some("Clear out drafts"));
    assert_eq!(trash.transfer, Transfer::Trash);
    assert_eq!((trash.source_root, trash.target_root), (None, None));
}

pub(in crate::index::tests) fn assert_v1_2_0_invoice(index: &Index) -> DocumentId {
    use crate::model::{DocumentSource, DocumentStatus, Field, FieldOrigin, RuleId};

    assert!(index.is_encrypted());

    let invoice = index
        .document_by_path(Path::new("archive/2026/Company A/invoice-4711.pdf"))
        .unwrap()
        .unwrap();
    assert_eq!(invoice.source, DocumentSource::Archive);
    assert_eq!(invoice.status, DocumentStatus::Filed);
    assert_eq!(invoice.fields.sender.as_deref(), Some("Company A"));
    assert_eq!(invoice.fields.recipient.as_deref(), Some("Muster GmbH"));
    assert_eq!(invoice.fields.kind.as_deref(), Some("invoice"));
    assert_eq!(invoice.fields.number.as_deref(), Some("4711"));
    assert_eq!(
        invoice.fields.date.map(|date| date.to_string()).as_deref(),
        Some("2026-01-15")
    );
    let amount = invoice.fields.amount.clone().unwrap();
    assert_eq!(
        (amount.minor_units, amount.currency.as_str()),
        (11900, "EUR")
    );
    assert_eq!(
        index.document_tags(invoice.id.get()).unwrap(),
        tag_set(&["project-x", "services"])
    );
    let automatic_tags: Vec<String> = index
        .conn
        .prepare(
            "SELECT t.name FROM document_tag d JOIN tag t ON t.id = d.tag_id
             WHERE d.document_id = ?1 AND d.automatic = 1 ORDER BY t.name",
        )
        .unwrap()
        .query_map([invoice.id.get()], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        automatic_tags,
        ["2026", "company-a", "invoice", "muster-gmbh"]
    );

    let origins = index.field_origins(invoice.id).unwrap();
    assert_eq!(
        origins.of(Field::Sender),
        FieldOrigin::Rule(Some(RuleId::new(1)))
    );
    assert_eq!(origins.of(Field::Recipient), FieldOrigin::AddressBlock);
    assert_eq!(origins.of(Field::Kind), FieldOrigin::Heading);
    assert_eq!(
        origins.of(Field::Date),
        FieldOrigin::Ai {
            confidence_percent: 87
        }
    );
    assert_eq!(origins.of(Field::Number), FieldOrigin::Manual);
    assert_eq!(
        origins.of(Field::Amount),
        FieldOrigin::Ai {
            confidence_percent: 100
        }
    );

    invoice.id
}

pub(in crate::index::tests) fn assert_v1_2_0_inbox_documents(
    index: &Index,
    invoice: DocumentId,
) -> (DocumentId, DocumentId) {
    use crate::model::{DocumentStatus, Field, FieldOrigin, RuleId};

    let receipt = index
        .document_by_path(Path::new("inbox/scan-2.pdf"))
        .unwrap()
        .unwrap();
    assert_eq!(receipt.status, DocumentStatus::New);
    assert_eq!(receipt.fields.kind.as_deref(), Some("receipt"));
    let receipt_origins = index.field_origins(receipt.id).unwrap();
    assert_eq!(receipt_origins.of(Field::Kind), FieldOrigin::Rule(None));
    assert_eq!(
        receipt_origins.of(Field::Sender),
        FieldOrigin::Rule(Some(RuleId::new(2)))
    );
    assert_eq!(receipt_origins.of(Field::Date), FieldOrigin::Manual);
    assert_eq!(receipt_origins.of(Field::Number), FieldOrigin::Unknown);
    let notice = index
        .document_by_path(Path::new("inbox/scan-3.pdf"))
        .unwrap()
        .unwrap();
    assert_eq!(notice.status, DocumentStatus::New);
    assert_eq!(index.search_text("hosting", 10).unwrap(), [invoice]);
    assert_eq!(index.search_text("chair", 10).unwrap(), [receipt.id]);
    assert_eq!(index.search_text("Finanzamt", 10).unwrap(), [notice.id]);

    (receipt.id, notice.id)
}

pub(in crate::index::tests) fn assert_v1_2_0_rules(index: &Index) {
    let rules = index.rules().unwrap();
    let ids: Vec<(i64, i64)> = rules.iter().map(|r| (r.id.get(), r.position)).collect();
    assert_eq!(ids[..4], [(4, 1), (1, 2), (2, 3), (3, 4)]);
    assert_eq!(rules.len(), 5);
    assert_eq!(rules[4].origin, crate::model::RuleOrigin::Chat);
    assert_eq!(rules[4].spec.name.as_deref(), Some("From the chat"));
    assert_v1_2_0_rule_with_identifiers(&rules[0]);
    assert_v1_2_0_hosting_rule(index, &rules[1]);
    assert_v1_2_0_inbox_rule(&rules[2]);
    assert_v1_2_0_suggested_rule(index, &rules[3]);
    let (hosting, inbox) = (&rules[1], &rules[2]);
    let hits = index.rule_hits().unwrap();
    assert_eq!(hits.get(&hosting.id), Some(&1));
    assert_eq!(hits.get(&inbox.id), Some(&1));
    assert_v1_2_0_discarded_suggestion(index);
}

pub(in crate::index::tests) fn assert_v1_2_0_rule_with_identifiers(
    identifiers: &crate::model::Rule,
) {
    use crate::model::{Condition, ConditionKind, RuleOrigin};

    assert_eq!(identifiers.origin, RuleOrigin::Hand);
    assert!(!identifiers.spec.confirmed && !identifiers.spec.enabled);
    assert_eq!(
        identifiers.spec.conditions.list,
        [
            Condition::new(ConditionKind::Regex, r"(?i)rechnung\s+\d+"),
            Condition::new(ConditionKind::Iban, "DE89 3704 0044 0532 0130 00"),
            Condition::new(ConditionKind::TaxId, "12/345/67890"),
        ]
    );
}

pub(in crate::index::tests) fn assert_v1_2_0_hosting_rule(
    index: &Index,
    hosting: &crate::model::Rule,
) {
    use crate::model::{Combinator, Condition, ConditionKind, RuleOrigin};

    assert_eq!(hosting.origin, RuleOrigin::Hand);
    assert_eq!(hosting.spec.name.as_deref(), Some("Hosting invoices"));
    assert_eq!(hosting.spec.conditions.combinator, Combinator::AllOf);
    assert_eq!(
        hosting.spec.conditions.list,
        [
            Condition::new(ConditionKind::Substring, "Invoice"),
            Condition::new(ConditionKind::Sender, "Company A"),
            Condition::not(ConditionKind::Word, "reminder"),
        ]
    );
    assert_eq!(hosting.spec.extract_number.as_deref(), Some(r"No\. (\d+)"));
    assert_eq!(
        hosting.spec.extract_date.as_deref(),
        Some(r"(\d{2}\.\d{2}\.\d{4})")
    );
    assert_eq!(
        hosting.spec.extract_amount.as_deref(),
        Some(r"Total (\d+,\d{2})")
    );
    assert_eq!(hosting.spec.kind.as_deref(), Some("invoice"));
    assert_eq!(hosting.spec.tags, tag_set(&["services"]));
    assert_eq!(hosting.spec.filing_pattern, "{year}/{sender}/");
    assert!(hosting.spec.confirmed && hosting.spec.enabled);
    assert_eq!(index.rule_evidence(hosting.id).unwrap(), None);
}

pub(in crate::index::tests) fn assert_v1_2_0_inbox_rule(inbox: &crate::model::Rule) {
    use crate::model::{Combinator, Condition, ConditionKind, RuleOrigin};

    assert_eq!(inbox.origin, RuleOrigin::Filing);
    assert_eq!(inbox.spec.name.as_deref(), Some("Inbox and tags"));
    assert!(!inbox.spec.enabled);
    assert_eq!(inbox.spec.kind, None);
    assert_eq!(inbox.spec.conditions.combinator, Combinator::AnyOf);
    assert_eq!(
        inbox.spec.conditions.list,
        [
            Condition::new(ConditionKind::Folder, "inbox"),
            Condition::new(ConditionKind::Tag, "services"),
            Condition::new(ConditionKind::SenderAddress, "company-a.example"),
            Condition::not(ConditionKind::Recipient, "Muster GmbH"),
        ]
    );
    assert_eq!(inbox.spec.tags, tag_set(&["inbox", "project-x"]));
    assert_eq!(inbox.spec.filing_pattern, "{year}/");
}

pub(in crate::index::tests) fn assert_v1_2_0_suggested_rule(
    index: &Index,
    suggested: &crate::model::Rule,
) {
    use crate::model::{Condition, ConditionKind, RuleEvidence, RuleOrigin};

    assert_eq!(suggested.origin, RuleOrigin::Suggestion);
    assert!(!suggested.spec.confirmed && !suggested.spec.enabled);
    assert_eq!(
        suggested.spec.conditions.list,
        [Condition::new(ConditionKind::Word, "Rechnung")]
    );
    assert_eq!(
        index.rule_evidence(suggested.id).unwrap(),
        Some(RuleEvidence {
            matching_in_folder: 8,
            folder_documents: 10,
            matching_elsewhere: 1,
            subtree: Some("2026".to_owned()),
        })
    );
    let (suggested_rule, _) = index.suggested_rules().unwrap().remove(0);
    assert_eq!(suggested_rule.id, suggested.id);
}

pub(in crate::index::tests) fn assert_v1_2_0_discarded_suggestion(index: &Index) {
    let discarded: Vec<(String, String, String, String)> = index
        .conn
        .prepare(
            "SELECT match_text, condition_kind, kind, filing_pattern
             FROM discarded_rule_suggestion",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        discarded,
        [(
            "mahnung".to_owned(),
            "word".to_owned(),
            "invoice".to_owned(),
            "{year}/{kind}/".to_owned()
        )]
    );
}

pub(in crate::index::tests) fn assert_v1_2_0_aliases_reviews_queries_and_tags(
    index: &Index,
    receipt: DocumentId,
    notice: DocumentId,
) {
    use crate::model::{SenderAliasKind, SenderAliasState};

    let aliases: Vec<_> = index
        .sender_aliases()
        .unwrap()
        .into_iter()
        .map(|alias| (alias.sender, alias.kind, alias.value, alias.state))
        .collect();
    assert_eq!(
        aliases,
        [
            (
                "Company A".to_owned(),
                SenderAliasKind::Domain,
                "company-a.example".to_owned(),
                SenderAliasState::Discarded
            ),
            (
                "Company A".to_owned(),
                SenderAliasKind::Iban,
                SenderAliasKind::Iban.normalise("DE89 3704 0044 0532 0130 00"),
                SenderAliasState::Offered
            ),
            (
                "Company A".to_owned(),
                SenderAliasKind::Name,
                "company a gmbh".to_owned(),
                SenderAliasState::Confirmed
            ),
            (
                "Company B".to_owned(),
                SenderAliasKind::Domain,
                "company-a.example".to_owned(),
                SenderAliasState::Confirmed
            ),
            (
                "Company B".to_owned(),
                SenderAliasKind::TaxId,
                SenderAliasKind::TaxId.normalise("12/345/67890"),
                SenderAliasState::Offered
            ),
        ]
    );

    // Step 037 closes every open question; the documents are looked at again.
    assert!(index.open_reviews().unwrap().is_empty());
    assert!(index.open_review_of(receipt).unwrap().is_none());
    assert!(index.open_review_of(notice).unwrap().is_none());
    let reasons: Vec<String> = index
        .conn
        .prepare("SELECT reason || ':' || COALESCE(field, '') FROM review_entry ORDER BY id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        reasons,
        ["missing_field:recipient", "low_confidence:kind"]
    );

    let learned_pattern: String = index
        .connection()
        .query_row(
            "SELECT pattern FROM location_subtree WHERE location = 'archive'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(learned_pattern, "{year}/{sender}/");

    let queries = index.saved_queries().unwrap();
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].name, "Invoices 2026");
    assert_eq!(queries[0].spec.year, Some(2026));
    assert_eq!(queries[0].spec.sender.as_deref(), Some("Company A"));
    assert_eq!(queries[0].spec.kind.as_deref(), Some("invoice"));
    assert_eq!(queries[0].spec.tags, tag_set(&["services"]));
    assert_eq!(queries[0].spec.full_text.as_deref(), Some("hosting"));
}

#[test]
fn an_index_written_by_release_1_2_0_opens_with_all_its_content_and_applies_none_of_its_steps_again(
) {
    let (_dir, path, store) = copy_of_the_v1_2_0_index();
    let applied = usize::try_from(FIXTURE_V1_2_0_VERSION).unwrap();
    // The steps the file already has fail, so opening succeeds only when
    // none of them runs again; steps added after 1.2.0 run as usual.
    let mut steps = vec!["this is not sql"; applied];
    steps.extend_from_slice(&MIGRATIONS[applied..]);

    let index = Index::open_with_migrations(&path, &store, &steps).unwrap();

    assert_eq!(user_version(&index), latest_version());
    assert_v1_2_0_content(&index);
}

#[test]
fn a_later_step_migrates_the_release_1_2_0_index_and_keeps_its_content() {
    let (_dir, path, store) = copy_of_the_v1_2_0_index();
    let steps = migrations_and(&[PROBE_COLUMN]);

    let index = Index::open_with_migrations(&path, &store, &steps).unwrap();

    assert_eq!(user_version(&index), probe_version());
    assert!(has_column(&index, "document", "probe"));
    assert_v1_2_0_content(&index);
    drop(index);
    let reopened = Index::open_with_migrations(&path, &store, &steps).unwrap();
    assert_eq!(user_version(&reopened), probe_version());
}

#[test]
fn an_index_written_by_release_1_5_1_opens_with_all_its_content_and_applies_none_of_its_steps_again(
) {
    let (_dir, path, store) = copy_of_the_v1_5_1_index();
    let applied = usize::try_from(FIXTURE_V1_5_1_VERSION).unwrap();
    let mut steps = vec!["this is not sql"; applied];
    steps.extend_from_slice(&MIGRATIONS[applied..]);

    let index = Index::open_with_migrations(&path, &store, &steps).unwrap();

    assert_eq!(user_version(&index), latest_version());
    assert_v1_2_0_content(&index);
}

#[test]
fn a_later_step_migrates_the_release_1_5_1_index_and_keeps_its_content() {
    let (_dir, path, store) = copy_of_the_v1_5_1_index();
    let steps = migrations_and(&[PROBE_COLUMN]);

    let index = Index::open_with_migrations(&path, &store, &steps).unwrap();

    assert_eq!(user_version(&index), probe_version());
    assert!(has_column(&index, "document", "probe"));
    assert_v1_2_0_content(&index);
}

#[test]
fn an_index_written_by_release_1_6_0_opens_with_all_its_content_and_applies_none_of_its_steps_again(
) {
    let (_dir, path, store) = copy_of_the_v1_6_0_index();
    let applied = usize::try_from(FIXTURE_V1_6_0_VERSION).unwrap();
    let mut steps = vec!["this is not sql"; applied];
    steps.extend_from_slice(&MIGRATIONS[applied..]);

    let mut index = Index::open_with_migrations(&path, &store, &steps).unwrap();

    assert_eq!(user_version(&index), latest_version());
    assert_v1_6_0_content(&mut index);
}

#[test]
fn a_later_step_migrates_the_release_1_6_0_index_and_keeps_its_content() {
    let (_dir, path, store) = copy_of_the_v1_6_0_index();
    let steps = migrations_and(&[PROBE_COLUMN]);

    let mut index = Index::open_with_migrations(&path, &store, &steps).unwrap();

    assert_eq!(user_version(&index), probe_version());
    assert!(has_column(&index, "document", "probe"));
    assert_v1_6_0_content(&mut index);
}

#[test]
fn an_index_written_by_release_1_7_0_opens_with_all_its_content_and_applies_none_of_its_steps_again(
) {
    let (_dir, path, store) = copy_of_the_v1_7_0_index();
    let applied = usize::try_from(FIXTURE_V1_7_0_VERSION).unwrap();
    let mut steps = vec!["this is not sql"; applied];
    steps.extend_from_slice(&MIGRATIONS[applied..]);

    let mut index = Index::open_with_migrations(&path, &store, &steps).unwrap();

    assert_eq!(user_version(&index), latest_version());
    assert_v1_6_0_content(&mut index);
}

#[test]
fn a_later_step_migrates_the_release_1_7_0_index_and_keeps_its_content() {
    let (_dir, path, store) = copy_of_the_v1_7_0_index();
    let steps = migrations_and(&[PROBE_COLUMN]);

    let mut index = Index::open_with_migrations(&path, &store, &steps).unwrap();

    assert_eq!(user_version(&index), probe_version());
    assert!(has_column(&index, "document", "probe"));
    assert_v1_6_0_content(&mut index);
}

#[test]
fn a_failing_second_step_leaves_the_release_1_2_0_file_byte_for_byte_as_it_was() {
    let (_dir, path, store) = copy_of_the_v1_2_0_index();
    let before = fs::read(&path).unwrap();
    let steps = migrations_and(&[PROBE_COLUMN, "CREATE TABLE document (x)"]);

    let err = Index::open_with_migrations(&path, &store, &steps).unwrap_err();

    assert!(
        matches!(
            err,
            IndexError::MigrationFailed { from, step, .. }
                if from == FIXTURE_V1_2_0_VERSION && step == latest_version() + 2
        ),
        "{err}"
    );
    assert!(err.to_string().contains("left unchanged"), "{err}");
    assert_eq!(fs::read(&path).unwrap(), before);
    let index = Index::open_with_migrations(&path, &store, MIGRATIONS).unwrap();
    assert_eq!(user_version(&index), latest_version());
    assert!(!has_column(&index, "document", "probe"));
    assert_v1_2_0_content(&index);
}
