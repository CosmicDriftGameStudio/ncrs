//! Writes `index-v1.2.0.db`, an encrypted index as release v1.2.0 makes it.
//!
//! Not compiled by cargo in the main repository. Run in an extract of the
//! commit that becomes the tag v1.2.0. The tag does not exist yet; until then
//! use the base of the release preparation branch (50f5735). Write the file
//! again when a migration is added before the tag.
//!
//! ```text
//! git archive "<v1.2.0 commit>" | tar -x -C <dir>
//! cp make_index_v1_2_0.rs <dir>/crates/ncrs-index/examples/
//! cd <dir> && cargo run --locked -p ncrs-index --example make_index_v1_2_0 -- <out>.db
//! ```
//!
//! The key is the fixed test key bytes 0x00..0x1f, which the tests put in an
//! `InMemoryKeyStore`. Every table and column the steps 010 to 021 add holds a
//! value that is not its default.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use chrono::{NaiveDate, TimeZone as _, Utc};
use ncrs_index::{
    Amount, Condition, ConditionKind, ContentHash, Currency, DocumentFields, DocumentSource,
    DocumentStatus, Field, FieldOrigin, InMemoryKeyStore, Index, NewDocument, QuerySpec,
    ReviewReason, RuleConditions, RuleEvidence, RuleOrigin, RuleSpec, SenderAliasKind,
    SubtreePattern,
};

fn tags(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn document(seed: u8, path: &str, source: DocumentSource, day: u32) -> NewDocument {
    NewDocument {
        hash: ContentHash([seed; 32]),
        path: PathBuf::from(path),
        source,
        received_at: Utc.with_ymd_and_hms(2026, 1, day, 9, 30, 0).unwrap(),
        stamp: None,
    }
}

fn rule(conditions: RuleConditions, kind: Option<&str>, filing_pattern: &str) -> RuleSpec {
    RuleSpec {
        name: None,
        conditions,
        extract_number: None,
        extract_date: None,
        extract_amount: None,
        kind: kind.map(str::to_owned),
        tags: BTreeSet::new(),
        filing_pattern: filing_pattern.to_owned(),
        confirmed: true,
        enabled: true,
    }
}

fn main() {
    let out = std::env::args().nth(1).expect("output path");
    let out = Path::new(&out);
    let key: Vec<u8> = (0u8..32).collect();
    let store = InMemoryKeyStore::with_entry(key);
    let (mut index, _recovery_key) = Index::create_encrypted(out, &store).unwrap();

    // Rules first: the origins of the fields point at them. Rule order, enabled,
    // name, combinator, conditions (negated, every kind), tags, origin.
    let hosting = index
        .create_rule(&RuleSpec {
            name: Some("Hosting invoices".to_owned()),
            extract_number: Some(r"No\. (\d+)".to_owned()),
            extract_date: Some(r"(\d{2}\.\d{2}\.\d{4})".to_owned()),
            extract_amount: Some(r"Total (\d+,\d{2})".to_owned()),
            tags: tags(&["services"]),
            ..rule(
                RuleConditions::all_of(vec![
                    Condition::new(ConditionKind::Substring, "Invoice"),
                    Condition::new(ConditionKind::Sender, "Company A"),
                    Condition::not(ConditionKind::Word, "reminder"),
                ]),
                Some("Invoice"),
                "{year}/{sender}/",
            )
        })
        .unwrap();
    let inbox_rule = index
        .create_rule_from(
            &RuleSpec {
                name: Some("Inbox and tags".to_owned()),
                enabled: false,
                tags: tags(&["project-x", "inbox"]),
                ..rule(
                    RuleConditions::any_of(vec![
                        Condition::new(ConditionKind::Folder, "inbox"),
                        Condition::new(ConditionKind::Tag, "services"),
                        Condition::new(ConditionKind::SenderAddress, "company-a.example"),
                        Condition::not(ConditionKind::Recipient, "Muster GmbH"),
                    ]),
                    None,
                    "{year}/",
                )
            },
            RuleOrigin::Filing,
        )
        .unwrap();
    index
        .suggest_rule(
            &rule(
                RuleConditions::all_of(vec![Condition::new(ConditionKind::Word, "Rechnung")]),
                Some("Invoice"),
                "{year}/{kind}/",
            ),
            RuleEvidence {
                matching_in_folder: 8,
                folder_documents: 10,
                matching_elsewhere: 1,
                subtree: Some("2026".to_owned()),
            },
        )
        .unwrap();
    let identifiers = index
        .create_rule(&RuleSpec {
            confirmed: false,
            ..rule(
                RuleConditions::all_of(vec![
                    Condition::new(ConditionKind::Regex, r"(?i)rechnung\s+\d+"),
                    Condition::new(ConditionKind::Iban, "DE89 3704 0044 0532 0130 00"),
                    Condition::new(ConditionKind::TaxId, "12/345/67890"),
                ]),
                Some("Invoice"),
                "{year}/{sender}/{number}",
            )
        })
        .unwrap();
    // The suggestion is dropped again and remembered with its condition kind.
    let dropped = index
        .suggest_rule(
            &rule(
                RuleConditions::all_of(vec![Condition::new(ConditionKind::Word, "Mahnung")]),
                Some("Invoice"),
                "{year}/{kind}/",
            ),
            RuleEvidence {
                matching_in_folder: 2,
                folder_documents: 5,
                matching_elsewhere: 0,
                subtree: None,
            },
        )
        .unwrap();
    let ncrs_index::RuleSuggestion::Created(dropped_id) = dropped else {
        panic!("the suggestion to drop was not created");
    };
    assert!(index.discard_rule_suggestion(dropped_id).unwrap());
    index.move_rule(identifiers, 0).unwrap();

    // Documents: every field origin variant, the recipient, own and automatic tags.
    let invoice = index
        .insert_document(&document(
            1,
            "archive/2026/Company A/invoice-4711.pdf",
            DocumentSource::Archive,
            15,
        ))
        .unwrap();
    index
        .set_document_text(invoice, "Invoice 4711 from Company A for hosting services")
        .unwrap();
    index
        .set_document_fields(
            invoice,
            &DocumentFields {
                sender: Some("Company A".to_owned()),
                recipient: Some("Muster GmbH".to_owned()),
                kind: Some("Invoice".to_owned()),
                date: NaiveDate::from_ymd_opt(2026, 1, 15),
                number: Some("4711".to_owned()),
                amount: Some(Amount {
                    minor_units: 11900,
                    currency: Currency::new("EUR").unwrap(),
                }),
                tags: tags(&["services", "project-x"]),
            },
            |field| match field {
                Field::Sender => FieldOrigin::Rule(Some(hosting)),
                Field::Recipient => FieldOrigin::AddressBlock,
                Field::Kind => FieldOrigin::Heading,
                Field::Date => FieldOrigin::Ai {
                    confidence_percent: 87,
                },
                Field::Number => FieldOrigin::Manual,
                Field::Amount => FieldOrigin::Ai {
                    confidence_percent: 100,
                },
            },
        )
        .unwrap();
    index
        .set_document_status(invoice, DocumentStatus::Filed)
        .unwrap();

    let receipt = index
        .insert_document(&document(2, "inbox/scan-2.pdf", DocumentSource::Inbox, 16))
        .unwrap();
    index
        .set_document_text(receipt, "Receipt for an office chair, delivered to Berlin")
        .unwrap();
    index
        .set_document_fields(
            receipt,
            &DocumentFields {
                sender: Some("Shop B".to_owned()),
                kind: Some("Receipt".to_owned()),
                date: NaiveDate::from_ymd_opt(2026, 1, 16),
                ..DocumentFields::default()
            },
            |field| match field {
                // A rule that was deleted since.
                Field::Kind => FieldOrigin::Rule(None),
                Field::Sender => FieldOrigin::Rule(Some(inbox_rule)),
                _ => FieldOrigin::Manual,
            },
        )
        .unwrap();
    index
        .set_document_status(receipt, DocumentStatus::Planned)
        .unwrap();
    index
        .add_review(receipt, ReviewReason::MissingField(Field::Recipient))
        .unwrap();

    let notice = index
        .insert_document(&document(3, "inbox/scan-3.pdf", DocumentSource::Inbox, 17))
        .unwrap();
    index
        .set_document_text(notice, "Tax notice for the year 2025 from the Finanzamt")
        .unwrap();
    index
        .add_review(notice, ReviewReason::LowConfidence(Field::Kind))
        .unwrap();

    index
        .replace_location_profile(
            Path::new("archive"),
            &[SubtreePattern {
                subtree: "2026".to_owned(),
                pattern: Some("{year}/{sender}/".to_owned()),
                documents: 1,
                total: 1,
            }],
        )
        .unwrap();

    index
        .save_query(
            "Invoices 2026",
            &QuerySpec {
                year: Some(2026),
                sender: Some("Company A".to_owned()),
                kind: Some("Invoice".to_owned()),
                tags: tags(&["services"]),
                full_text: Some("hosting".to_owned()),
            },
        )
        .unwrap();

    // Sender aliases per sender: the same domain discarded for one sender and
    // confirmed for another, one alias of each other kind and state.
    let alias_id = |index: &Index, sender: &str, kind: SenderAliasKind| {
        index
            .sender_aliases()
            .unwrap()
            .into_iter()
            .find(|alias| alias.sender == sender && alias.kind == kind)
            .unwrap()
            .id
    };
    assert!(index
        .offer_sender_alias("Company A", SenderAliasKind::Domain, "company-a.example")
        .unwrap());
    let discarded = alias_id(&index, "Company A", SenderAliasKind::Domain);
    index.discard_sender_alias(discarded).unwrap();
    assert!(index
        .offer_sender_alias("Company B", SenderAliasKind::Domain, "company-a.example")
        .unwrap());
    let confirmed = alias_id(&index, "Company B", SenderAliasKind::Domain);
    index.confirm_sender_alias(confirmed).unwrap();
    assert!(index
        .offer_sender_alias("Company A", SenderAliasKind::Name, "Company A GmbH")
        .unwrap());
    let confirmed_name = alias_id(&index, "Company A", SenderAliasKind::Name);
    index.confirm_sender_alias(confirmed_name).unwrap();
    assert!(index
        .offer_sender_alias(
            "Company A",
            SenderAliasKind::Iban,
            "DE89 3704 0044 0532 0130 00"
        )
        .unwrap());
    assert!(index
        .offer_sender_alias("Company B", SenderAliasKind::TaxId, "12/345/67890")
        .unwrap());

    index.set_file_list_tag("services", Some(true)).unwrap();
    index.set_file_list_tag("project-x", Some(false)).unwrap();
}
