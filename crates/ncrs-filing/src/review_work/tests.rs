use std::collections::BTreeSet;
use std::fs;

use chrono::{NaiveDate, TimeZone as _, Utc};
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
            std::slice::from_ref(&setup.inbox),
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

    assert_eq!(
        result,
        Err("the document is no longer in the index".to_owned())
    );
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
fn accepting_into_a_folder_keeps_braces_of_the_file_name() {
    let setup = Setup::new();
    let id = setup.in_review("Notiz {sender}.pdf");
    setup.with_index(|index| {
        index
            .set_document_fields(id, &Setup::fields("Telekom"), |_| FieldOrigin::Unknown)
            .unwrap();
    });
    let item = first(&setup.access());

    let summary = setup.access().accept_into(&[item], Path::new("Inbox"));

    assert_eq!(summary.filed, 1, "{:?}", summary.first_failure);
    assert!(setup.archive.join("Inbox/Notiz {sender}.pdf").exists());
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
fn the_reviews_of_some_files_come_in_the_order_of_the_paths_each_once() {
    let setup = Setup::new();
    setup.in_review("a.pdf");
    setup.in_review("b.pdf");
    setup.in_review("c.pdf");
    setup.document("filed.pdf", DocumentStatus::Filed);
    let path = |name: &str| setup.inbox.join(name);

    let items = setup
        .access()
        .items_for(&[
            path("c.pdf"),
            path("filed.pdf"),
            path("unknown.pdf"),
            path("a.pdf"),
            path("c.pdf"),
        ])
        .unwrap();

    let listed: Vec<_> = items.iter().map(|item| item.path.clone()).collect();
    assert_eq!(listed, vec![path("c.pdf"), path("a.pdf")]);
}

#[test]
fn confirming_a_document_from_a_copy_folder_keeps_the_original_and_marks_it_filed() {
    let setup = Setup::new();
    setup.in_review("scan.pdf");
    let access = setup.access().with_copy_roots(vec![setup.inbox.clone()]);
    let item = first(&access);

    let outcome = access
        .confirm(
            item.entry,
            item.document_id,
            &Setup::fields("Company A"),
            "{sender}/{kind}",
        )
        .unwrap();

    assert!(matches!(outcome, ConfirmOutcome::Filed { .. }));
    assert!(setup.inbox.join("scan.pdf").exists());
    assert!(setup.archive.join("Company A/Invoice.pdf").exists());
    setup.with_index(|index| {
        let document = index.document(item.document_id).unwrap().unwrap();
        assert_eq!(document.status, DocumentStatus::Filed);
    });
}

#[test]
fn an_item_that_names_another_file_than_the_document_is_refused() {
    let setup = Setup::new();
    setup.in_review("scan.pdf");
    let access = setup.access();
    let mut item = first(&access);
    item.path = setup.inbox.join("other.pdf");

    let summary = access.accept(&[item]);

    assert_eq!(summary.failed, 1);
    assert_eq!(summary.first_failure.as_deref(), Some(ITEM_CHANGED));
    assert!(setup.inbox.join("scan.pdf").exists());
}

#[test]
fn confirming_an_entry_that_is_no_longer_open_files_nothing() {
    let setup = Setup::new();
    setup.in_review("scan.pdf");
    let access = setup.access();
    let item = first(&access);
    setup
        .with_index(|index| index.resolve_review(item.entry))
        .unwrap();

    let outcome = access.confirm(
        item.entry,
        item.document_id,
        &Setup::fields("Company A"),
        "{sender}/{kind}",
    );

    assert_eq!(outcome, Err(ITEM_CHANGED.to_owned()));
    assert!(setup.inbox.join("scan.pdf").exists());
    assert!(!setup.archive.join("Company A").exists());
}

#[test]
fn a_folder_outside_the_target_files_nothing() {
    let setup = Setup::new();
    setup.in_review("scan.pdf");
    let access = setup.access();
    let outside = setup._dir.path().join("elsewhere");

    for folder in [Path::new("../outside"), outside.as_path()] {
        let item = first(&access);
        let summary = access.accept_into(&[item], folder);

        assert_eq!(summary.failed, 1, "{folder:?}");
        assert_eq!(
            summary.first_failure.as_deref(),
            Some(FOLDER_ESCAPES_TARGET),
            "{folder:?}"
        );
    }
    assert!(setup.inbox.join("scan.pdf").exists());
    assert!(!outside.exists());
    assert!(!setup._dir.path().join("outside").exists());
}

#[test]
fn everything_one_accept_files_goes_into_one_batch_that_one_undo_takes_back() {
    let setup = Setup::new();
    setup.in_review("one.pdf");
    setup.in_review("two.pdf");
    let items = setup.access().load().unwrap();
    assert_eq!(items.len(), 2);

    let summary = setup.access().accept_into(&items, Path::new("Box"));

    assert_eq!(summary.filed, 2, "{:?}", summary.first_failure);
    let batches = setup.with_index(|index| index.undoable_batches(BatchKind::Filing, 10).unwrap());
    assert_eq!(batches.len(), 1);
    let batch = batches[0];
    let info = setup.with_index(|index| index.batch_info(batch).unwrap().unwrap());
    assert_eq!(
        info.name,
        Some(setup.archive.join("Box").to_string_lossy().into_owned())
    );
    let journal = setup.with_index(|index| index.journal_items(batch).unwrap());
    assert_eq!(journal.len(), 2);

    setup.with_index(|index| {
        crate::execute::undo_with_trash(index, batch, &[], None).unwrap();
    });
    assert!(setup.inbox.join("one.pdf").exists() && setup.inbox.join("two.pdf").exists());
}

#[test]
fn the_neighbours_are_built_once_while_the_index_is_not_written_to() {
    let setup = Setup::new();
    setup.neighbours_in("Insurance/2024", "Company A");
    setup.in_review("new.pdf");
    let access = setup.access();

    let first_load = access.load().unwrap();
    let second_load = access.load().unwrap();
    let paths: Vec<PathBuf> = first_load.iter().map(|item| item.path.clone()).collect();
    let for_paths = access.items_for(&paths).unwrap();

    assert_eq!(first_load, second_load);
    assert_eq!(first_load, for_paths);
    assert_eq!(access.neighbour_builds(), 1);
}

#[test]
fn a_write_to_the_index_makes_the_neighbours_build_again() {
    let setup = Setup::new();
    setup.neighbours_in("Insurance/2024", "Company A");
    let before = setup.unplaced();
    let access = setup.access();
    access.load().unwrap();
    assert_eq!(access.neighbour_builds(), 1);

    setup.neighbours_in("Other/2024", "Company A");
    let after = access.load().unwrap().remove(0);

    assert_eq!(access.neighbour_builds(), 2);
    assert_ne!(before.placement, after.placement);
}

/// A document in review whose place already holds `existing`, with the
/// archive-side trash of the answers.
struct Taken {
    access: ReviewAccess,
    item: ReviewItem,
    target: PathBuf,
    bin: PathBuf,
}

fn taken(setup: &Setup, existing: &str) -> Taken {
    setup.in_review("scan.pdf");
    let target = setup.archive.join("Company A").join("Invoice.pdf");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, existing).unwrap();
    let bin = setup._dir.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let trash: Arc<dyn ncrs_fs::Trash> = Arc::new(ncrs_fs::JournaledTrash::new(
        setup._dir.path().join("trash.toml"),
        ncrs_fs::FolderBackend(bin.clone()),
    ));
    let access = setup
        .access()
        .with_location_roots(vec![setup.inbox.clone()])
        .with_trash(trash);
    let mut item = first(&access);
    item.fields = Setup::fields("Company A");
    item.proposed_pattern = Some("{sender}/{kind}".to_owned());
    Taken {
        access,
        item,
        target,
        bin,
    }
}

const INCOMING: &str = "content of scan.pdf";

#[test]
fn comparing_tells_equal_content_from_different_content() {
    let setup = Setup::new();
    let same = taken(&setup, INCOMING);
    let relative = PathBuf::from("Company A/Invoice");
    let equal = same
        .access
        .compare_targets(&[(same.item.path.clone(), relative.clone())]);
    assert_eq!(equal[0].map(|target| target.same_content), Some(true));

    fs::write(&same.target, "something else entirely").unwrap();
    let different = same
        .access
        .compare_targets(&[(same.item.path.clone(), relative)]);
    assert_eq!(different[0].map(|target| target.same_content), Some(false));
}

#[test]
fn a_duplicate_goes_to_the_trash_and_the_target_stays() {
    let setup = Setup::new();
    let found = taken(&setup, INCOMING);

    let summary = found
        .access
        .delete_duplicates(std::slice::from_ref(&found.item));

    assert_eq!((summary.trashed, summary.failed), (1, 0));
    assert!(!found.item.path.exists());
    assert_eq!(fs::read_to_string(&found.target).unwrap(), INCOMING);
    assert_eq!(
        fs::read_to_string(found.bin.join("scan.pdf")).unwrap(),
        INCOMING
    );
    assert!(found.access.load().unwrap().is_empty());
}

#[test]
fn a_file_that_differs_is_not_taken_for_a_duplicate() {
    let setup = Setup::new();
    let found = taken(&setup, "another content");

    let summary = found
        .access
        .delete_duplicates(std::slice::from_ref(&found.item));

    assert_eq!((summary.trashed, summary.failed), (0, 1));
    assert_eq!(
        summary.first_failure.as_deref(),
        Some(answers::NOT_A_DUPLICATE)
    );
    assert!(found.item.path.exists());
    assert_eq!(found.access.load().unwrap().len(), 1);
}

#[test]
fn overwriting_puts_the_old_file_in_the_trash_and_files_the_new_one() {
    let setup = Setup::new();
    let found = taken(&setup, "the old file");

    let summary = found.access.overwrite(std::slice::from_ref(&found.item));

    assert_eq!((summary.filed, summary.failed), (1, 0));
    assert_eq!(fs::read_to_string(&found.target).unwrap(), INCOMING);
    assert_eq!(
        fs::read_to_string(found.bin.join("Invoice.pdf")).unwrap(),
        "the old file"
    );
    assert!(!found.item.path.exists());
    assert!(found.access.load().unwrap().is_empty());
}

#[test]
fn keeping_both_files_the_new_one_under_the_next_free_number() {
    let setup = Setup::new();
    let found = taken(&setup, "the old file");
    let second = found.target.with_file_name("Invoice (2).pdf");
    fs::write(&second, "already there").unwrap();

    let summary = found.access.keep_both(std::slice::from_ref(&found.item));

    assert_eq!((summary.filed, summary.failed), (1, 0));
    assert_eq!(fs::read_to_string(&found.target).unwrap(), "the old file");
    assert_eq!(fs::read_to_string(&second).unwrap(), "already there");
    assert_eq!(
        fs::read_to_string(found.target.with_file_name("Invoice (3).pdf")).unwrap(),
        INCOMING
    );
    assert!(found.access.load().unwrap().is_empty());
}

#[test]
fn a_document_without_a_date_is_filed_without_it() {
    let setup = Setup::new();
    setup.in_review("scan.pdf");
    let access = setup.access();
    let mut item = first(&access);
    item.fields = Setup::fields("Company A");
    item.proposed_pattern = Some("{sender}/{date}_{kind}".to_owned());

    let summary = access.accept(&[item]);

    assert_eq!((summary.filed, summary.failed), (1, 0));
    assert_eq!(
        fs::read_to_string(setup.archive.join("Company A").join("Invoice.pdf")).unwrap(),
        INCOMING
    );
    assert!(access.load().unwrap().is_empty());
}

#[test]
fn an_ignored_document_leaves_the_list_and_stays_where_it_is() {
    let setup = Setup::new();
    setup.in_review("scan.pdf");
    let access = setup.access();
    let item = first(&access);

    access.exclude(std::slice::from_ref(&item.path)).unwrap();

    assert!(access.load().unwrap().is_empty());
    assert!(item.path.exists());
}

#[test]
fn a_typed_date_is_filed_into_the_name() {
    let setup = Setup::new();
    setup.in_review("scan.pdf");
    let access = setup.access();
    let mut item = first(&access);
    item.fields = Setup::fields("Company A");
    item.fields.date = chrono::NaiveDate::from_ymd_opt(2025, 3, 7);
    item.proposed_pattern = Some("{sender}/{year}-{month}-{day}_{kind}".to_owned());

    let summary = access.accept(&[item]);

    assert_eq!((summary.filed, summary.failed), (1, 0));
    assert!(setup
        .archive
        .join("Company A")
        .join("2025-03-07_Invoice.pdf")
        .exists());
}

#[test]
fn a_typed_sender_is_filed_into_the_folder() {
    let setup = Setup::new();
    setup.in_review("scan.pdf");
    let access = setup.access();
    let mut item = first(&access);
    item.fields = Setup::fields("Company A");
    item.fields.sender = Some("Company B".to_owned());
    item.proposed_pattern = Some("{sender}/{kind}".to_owned());

    let summary = access.accept(&[item]);

    assert_eq!((summary.filed, summary.failed), (1, 0));
    assert!(setup.archive.join("Company B").join("Invoice.pdf").exists());
}
