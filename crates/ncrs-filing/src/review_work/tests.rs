#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeSet;
use std::fs;

use chrono::{TimeZone as _, Utc};
use ncrs_index::{Condition, ConditionKind, DocumentSource, NewDocument, RuleConditions, RuleSpec};
use ncrs_intake::hash_path;
use tempfile::TempDir;

use super::*;
use crate::filing_work::file_planned;

struct Setup {
    _dir: TempDir,
    inbox: PathBuf,
    archive: PathBuf,
    slot: IndexSlot,
}

impl Setup {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path().join("inbox");
        let archive = dir.path().join("archive");
        fs::create_dir_all(&inbox).unwrap();
        fs::create_dir_all(&archive).unwrap();
        let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        Self {
            _dir: dir,
            inbox,
            archive,
            slot: Arc::new(Mutex::new(Some(index))),
        }
    }

    fn access(&self) -> ReviewAccess {
        ReviewAccess::new(Arc::clone(&self.slot), self.archive.clone())
    }

    fn with_index<T>(&self, work: impl FnOnce(&mut Index) -> T) -> T {
        let mut guard = self.slot.lock().unwrap();
        work(guard.as_mut().unwrap())
    }

    /// An inbox file in `status`; in review when no rule placed it.
    fn document(&self, name: &str, status: DocumentStatus) -> DocumentId {
        let path = self.inbox.join(name);
        fs::write(&path, format!("content of {name}")).unwrap();
        let metadata = fs::metadata(&path).unwrap();
        self.with_index(|index| {
            let id = index
                .insert_document(&NewDocument {
                    hash: hash_path(&path).unwrap(),
                    path: path.clone(),
                    source: DocumentSource::Inbox,
                    received_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                    stamp: Some(ncrs_index::FileStamp {
                        size: metadata.len(),
                        mtime: metadata.modified().unwrap(),
                    }),
                })
                .unwrap();
            index.set_document_text(id, "Invoice").unwrap();
            index.set_document_status(id, status).unwrap();
            id
        })
    }

    fn in_review(&self, name: &str) -> DocumentId {
        let id = self.document(name, DocumentStatus::InReview);
        self.with_index(|index| index.add_review(id, ReviewReason::NoMatchingRule))
            .unwrap();
        id
    }

    fn fields(sender: &str) -> DocumentFields {
        DocumentFields {
            sender: Some(sender.to_owned()),
            kind: Some("invoice".to_owned()),
            ..DocumentFields::default()
        }
    }

    /// An archive file of Company A's invoices, with the fields the pipeline would store.
    fn neighbour(&self, relative: &str, sender: &str, date: NaiveDate) {
        let path = self.archive.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format!("content of {relative}")).unwrap();
        let metadata = fs::metadata(&path).unwrap();
        self.with_index(|index| {
            let id = index
                .insert_document(&NewDocument {
                    hash: hash_path(&path).unwrap(),
                    path: path.clone(),
                    source: DocumentSource::Archive,
                    received_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                    stamp: Some(ncrs_index::FileStamp {
                        size: metadata.len(),
                        mtime: metadata.modified().unwrap(),
                    }),
                })
                .unwrap();
            let fields = DocumentFields {
                date: Some(date),
                ..Self::fields(sender)
            };
            index
                .set_document_fields(id, &fields, |_| FieldOrigin::Unknown)
                .unwrap();
            index.set_document_text(id, "Invoice").unwrap();
        });
    }

    /// Five dated invoices of `sender` below `folder`.
    fn neighbours_in(&self, folder: &str, sender: &str) {
        for month in 1..=5 {
            let date = NaiveDate::from_ymd_opt(2024, month, 1).unwrap();
            self.neighbour(&format!("{folder}/{date}_invoice.pdf"), sender, date);
        }
    }

    /// A review item the rules know nothing about, with the fields of an invoice of Company A.
    fn unplaced(&self) -> ReviewItem {
        let id = self.in_review("new.pdf");
        self.with_index(|index| {
            let fields = DocumentFields {
                date: NaiveDate::from_ymd_opt(2024, 6, 1),
                ..Self::fields("Company A")
            };
            index
                .set_document_fields(id, &fields, |_| FieldOrigin::Unknown)
                .unwrap();
        });
        first(&self.access())
    }
}

fn first(access: &ReviewAccess) -> ReviewItem {
    access.load().unwrap().remove(0)
}

#[test]
fn the_text_state_tells_images_from_empty_from_read_text() {
    let setup = Setup::new();
    let read = setup.in_review("invoice.pdf");
    let empty = setup.in_review("blank.pdf");
    let image = setup.in_review("photo.heic");
    let never_read = setup.in_review("never.pdf");
    let readable_photo = setup.in_review("receipt.jpg");
    setup.with_index(|index| {
        index.set_document_text(empty, "").unwrap();
        index.set_document_text(image, "").unwrap();
        index.set_document_text(readable_photo, "").unwrap();
        index
            .connection()
            .execute(
                "DELETE FROM document_text
                 WHERE rowid = (SELECT id FROM document WHERE path LIKE '%never.pdf')",
                [],
            )
            .unwrap();
    });

    let states: Vec<(DocumentId, TextState)> = setup
        .access()
        .load()
        .unwrap()
        .into_iter()
        .map(|item| (item.document_id, item.text_state))
        .collect();

    assert_eq!(
        states,
        vec![
            (read, TextState::Read),
            (empty, TextState::NoneFound),
            (image, TextState::NotReadable),
            (never_read, TextState::NoneFound),
            (readable_photo, TextState::NoneFound),
        ]
    );
}

#[test]
fn a_document_no_rule_filed_is_listed_with_its_reason() {
    let setup = Setup::new();
    let id = setup.in_review("scan.pdf");

    let item = first(&setup.access());

    assert_eq!(item.document_id, id);
    assert_eq!(item.reason, ReviewReason::NoMatchingRule);
    assert_eq!(item.proposed_pattern, None);
}

#[test]
fn confirming_with_changed_fields_files_the_document_where_the_pattern_says() {
    let setup = Setup::new();
    setup.in_review("scan.pdf");
    let access = setup.access();
    let item = first(&access);

    let outcome = access
        .confirm(
            item.entry,
            item.document_id,
            &Setup::fields("Company A"),
            "{sender}/{kind}",
        )
        .unwrap();

    assert_eq!(
        outcome,
        ConfirmOutcome::Filed {
            relative_path: PathBuf::from("Company A/Invoice.pdf")
        }
    );
    let target = setup.archive.join("Company A").join("Invoice.pdf");
    assert_eq!(fs::read_to_string(target).unwrap(), "content of scan.pdf");
    assert!(!setup.inbox.join("scan.pdf").exists());
    setup.with_index(|index| {
        let document = index.document(item.document_id).unwrap().unwrap();
        assert_eq!(document.status, DocumentStatus::Filed);
        assert_eq!(document.fields.sender.as_deref(), Some("Company A"));
        assert!(index.open_reviews().unwrap().is_empty());
        let origins = index.field_origins(item.document_id).unwrap();
        for field in [ncrs_index::Field::Sender, ncrs_index::Field::Kind] {
            assert_eq!(origins.of(field), FieldOrigin::Manual, "{field:?}");
        }
        assert_eq!(origins.of(ncrs_index::Field::Date), FieldOrigin::Unknown);
    });
}

#[test]
fn a_rule_for_the_sender_files_the_next_document_without_a_review() {
    let setup = Setup::new();
    setup.with_index(|index| {
        let spec = RuleSpec {
            name: None,
            conditions: RuleConditions::all_of(vec![Condition::new(
                ConditionKind::Sender,
                "Company A",
            )]),
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: Some("invoice".to_owned()),
            tags: BTreeSet::new(),
            filing_pattern: "{sender}/{kind}".to_owned(),
            confirmed: true,
            enabled: true,
        };
        index
            .create_rule_from(&spec, ncrs_index::RuleOrigin::Filing)
            .unwrap();
    });
    let next = setup.document("next.pdf", DocumentStatus::Planned);
    setup.with_index(|index| {
        index
            .set_document_fields(next, &Setup::fields("Company A"), |_| FieldOrigin::Unknown)
            .unwrap();
    });

    let summary = setup.with_index(|index| {
        file_planned(
            index,
            &setup.archive,
            &[],
            &[],
            &mut Counting,
            &Move,
            KindLanguage::English,
        )
        .unwrap()
    });

    assert_eq!(summary.filed, 1);
    assert_eq!(summary.reviewed, 0);
    assert!(setup.archive.join("Company A").join("Invoice.pdf").exists());
    setup.with_index(|index| assert!(index.open_reviews().unwrap().is_empty()));
}

#[test]
fn a_taken_place_keeps_the_entry_open_and_the_target_unchanged() {
    let setup = Setup::new();
    setup.in_review("scan.pdf");
    let taken = setup.archive.join("Company A").join("Invoice.pdf");
    fs::create_dir_all(taken.parent().unwrap()).unwrap();
    fs::write(&taken, "the other file").unwrap();
    let access = setup.access();
    let item = first(&access);

    let outcome = access
        .confirm(
            item.entry,
            item.document_id,
            &Setup::fields("Company A"),
            "{sender}/{kind}",
        )
        .unwrap();

    assert_eq!(outcome, ConfirmOutcome::TargetExists);
    assert_eq!(fs::read_to_string(&taken).unwrap(), "the other file");
    assert!(setup.inbox.join("scan.pdf").exists());
    assert_eq!(access.load().unwrap().len(), 1);
    setup.with_index(|index| {
        let stored = index.document(item.document_id).unwrap().unwrap();
        assert_eq!(stored.fields, item.fields);
    });
}

#[test]
fn an_excluded_document_is_neither_listed_nor_confirmed() {
    let setup = Setup::new();
    setup.in_review("scan.pdf");
    let access = setup.access();
    let item = first(&access);
    setup.with_index(|index| index.exclude_path(&setup.inbox.join("scan.pdf")).unwrap());

    assert!(access.load().unwrap().is_empty());
    let result = access.confirm(
        item.entry,
        item.document_id,
        &Setup::fields("Company A"),
        "{sender}/{kind}",
    );

    assert_eq!(result, Err(DOCUMENT_EXCLUDED.to_owned()));
    assert!(setup.inbox.join("scan.pdf").exists());
}

#[test]
fn a_document_that_is_missing_is_neither_counted_nor_listed() {
    let setup = Setup::new();
    let id = setup.in_review("scan.pdf");
    setup.with_index(|index| index.mark_document_missing(id).unwrap());

    let open_reviews = setup.with_index(|index| index.hermes_counts().unwrap().open_reviews);

    assert_eq!(open_reviews, 0);
    assert!(setup.access().load().unwrap().is_empty());
}

#[test]
fn a_pattern_that_cannot_be_rendered_moves_nothing() {
    let setup = Setup::new();
    setup.in_review("scan.pdf");
    let access = setup.access();
    let item = first(&access);

    for pattern in ["{nonsense}", "{date}/{kind}", "../{kind}", ""] {
        let result = access.confirm(
            item.entry,
            item.document_id,
            &Setup::fields("Company A"),
            pattern,
        );

        assert_eq!(result, Err(PATTERN_DOES_NOT_FIT.to_owned()), "{pattern}");
    }
    assert!(setup.inbox.join("scan.pdf").exists());
    assert_eq!(access.load().unwrap().len(), 1);
}

#[test]
fn a_file_that_vanished_goes_back_to_the_pipeline() {
    let setup = Setup::new();
    setup.in_review("scan.pdf");
    let access = setup.access();
    let item = first(&access);
    fs::remove_file(setup.inbox.join("scan.pdf")).unwrap();

    let outcome = access
        .confirm(
            item.entry,
            item.document_id,
            &Setup::fields("Company A"),
            "{sender}/{kind}",
        )
        .unwrap();

    assert_eq!(outcome, ConfirmOutcome::Requeued);
    setup.with_index(|index| {
        let document = index.document(item.document_id).unwrap().unwrap();
        assert_eq!(document.status, DocumentStatus::New);
        assert!(index.open_reviews().unwrap().is_empty());
    });
}

#[test]
fn a_rule_that_matches_the_text_gives_its_passages_and_the_fields_it_filled() {
    let setup = Setup::new();
    let document = setup.in_review("rule.pdf");
    let rule = setup.with_index(|index| {
        index
            .set_document_text(document, "Invoice 4711\nfrom COMPANY A Ltd")
            .unwrap();
        let spec = RuleSpec {
            name: None,
            conditions: RuleConditions::all_of(vec![
                Condition::new(ConditionKind::Substring, "company a"),
                Condition::not(ConditionKind::Substring, "agency"),
            ]),
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: Some("invoice".to_owned()),
            tags: BTreeSet::new(),
            filing_pattern: "{kind}".to_owned(),
            confirmed: false,
            enabled: true,
        };
        index
            .create_rule_from(&spec, ncrs_index::RuleOrigin::Filing)
            .unwrap()
    });

    let item = first(&setup.access());

    assert_eq!(item.passages, ["COMPANY A"]);
    assert_eq!(item.origins.of(Field::Kind), FieldOrigin::Rule(Some(rule)));
    assert_eq!(item.origins.of(Field::Sender), FieldOrigin::Unknown);
}

#[test]
fn the_sender_the_ai_read_is_a_passage_where_it_stands_in_the_text() {
    let setup = Setup::new();
    let document = setup.in_review("ai.pdf");
    setup.with_index(|index| {
        index
            .set_document_text(document, "Invoice 4711\nCompany A Ltd")
            .unwrap();
        index
            .set_document_fields(document, &Setup::fields("Company A"), |field| match field {
                Field::Sender => FieldOrigin::Ai {
                    confidence_percent: 82,
                },
                _ => FieldOrigin::Manual,
            })
            .unwrap();
    });

    let item = first(&setup.access());

    assert_eq!(item.passages, ["Company A"]);
    assert_eq!(
        item.origins.of(Field::Sender),
        FieldOrigin::Ai {
            confidence_percent: 82
        }
    );
}

#[test]
fn a_sure_placement_is_accepted_into_the_folder_of_its_neighbours() {
    let setup = Setup::new();
    setup.neighbours_in("Insurance/2024", "Company A");
    let item = setup.unplaced();

    let placement = item.placement.as_ref().unwrap();
    assert!(placement.is_sure());
    assert_eq!(placement.folder, Path::new("Insurance/2024"));
    assert!(item.proposed_pattern.is_some());

    let summary = setup.access().accept(&[item]);

    assert_eq!(summary.filed, 1, "{:?}", summary.first_failure);
    let filed = summary.last_filed.unwrap();
    assert!(filed.starts_with("Insurance/2024"), "{filed:?}");
    assert!(setup.archive.join(&filed).exists());
    assert!(!setup.inbox.join("new.pdf").exists());
}

#[test]
fn a_split_placement_proposes_nothing_and_accept_leaves_the_file_in_review() {
    let setup = Setup::new();
    setup.neighbours_in("Insurance/2024", "Company A");
    setup.neighbours_in("Contracts/2024", "Company A");
    let item = setup.unplaced();

    let placement = item.placement.as_ref().unwrap();
    assert!(!placement.is_sure());
    assert_eq!(item.proposed_pattern, None);

    let summary = setup.access().accept(&[item]);

    assert_eq!((summary.filed, summary.failed), (0, 1));
    assert!(setup.inbox.join("new.pdf").exists());
    assert_eq!(setup.access().load().unwrap().len(), 1);
}

#[test]
fn a_rule_proposal_wins_over_a_placement() {
    let setup = Setup::new();
    setup.neighbours_in("Insurance/2024", "Company A");
    let id = setup.in_review("new.pdf");
    setup.with_index(|index| {
        index
            .set_document_text(id, "Invoice 4711\nfrom COMPANY A Ltd")
            .unwrap();
        let spec = RuleSpec {
            name: None,
            conditions: RuleConditions::all_of(vec![Condition::new(
                ConditionKind::Substring,
                "company a",
            )]),
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: Some("invoice".to_owned()),
            tags: BTreeSet::new(),
            filing_pattern: "{kind}".to_owned(),
            confirmed: false,
            enabled: true,
        };
        index
            .create_rule_from(&spec, ncrs_index::RuleOrigin::Filing)
            .unwrap();
        let fields = DocumentFields {
            date: NaiveDate::from_ymd_opt(2024, 6, 1),
            ..Setup::fields("Company A")
        };
        index
            .set_document_fields(id, &fields, |_| FieldOrigin::Unknown)
            .unwrap();
    });

    let item = first(&setup.access());

    assert_eq!(item.proposed_pattern.as_deref(), Some("{kind}"));
    assert!(item.placement.as_ref().unwrap().is_sure());
}

#[test]
fn dates_are_iso_and_empty_is_none() {
    assert_eq!(parse_date(""), Ok(None));
    assert_eq!(
        parse_date(" 2024-03-09 "),
        Ok(NaiveDate::from_ymd_opt(2024, 3, 9))
    );
    for bad in ["09.03.2024", "2024-13-01", "yesterday"] {
        assert_eq!(parse_date(bad), Err(FieldError::Date), "{bad}");
    }
}

#[test]
fn amounts_take_a_point_or_a_comma_and_a_currency() {
    let eur = |minor_units| {
        Ok(Some(Amount {
            minor_units,
            currency: Currency::new("EUR").unwrap(),
        }))
    };

    assert_eq!(parse_amount(""), Ok(None));
    assert_eq!(parse_amount("12.34 EUR"), eur(1234));
    assert_eq!(parse_amount("12,34 eur"), eur(1234));
    assert_eq!(parse_amount("12 EUR"), eur(1200));
    assert_eq!(parse_amount("0.5 EUR"), eur(50));
    assert_eq!(parse_amount("-3.05 EUR"), eur(-305));
    for bad in [
        "12.34",
        "EUR",
        "12.345 EUR",
        "1x EUR",
        "12 EURO",
        "12 EUR now",
        ".5 EUR",
    ] {
        assert_eq!(parse_amount(bad), Err(FieldError::Amount), "{bad}");
    }
    assert_eq!(
        parse_amount("99999999999999999999 EUR"),
        Err(FieldError::Amount)
    );
}

#[test]
fn an_amount_shown_is_an_amount_that_parses() {
    let amount = parse_amount("-7,05 EUR").unwrap().unwrap();

    assert_eq!(amount_text(&amount), "-7.05 EUR");
    assert_eq!(parse_amount(&amount_text(&amount)), Ok(Some(amount)));
}
