use std::collections::BTreeSet;
use std::fs;

use chrono::{TimeZone as _, Utc};
use ncrs_index::{
    Condition, ConditionKind, DocumentFields, DocumentSource, Field, NewDocument, RuleConditions,
    RuleSpec,
};
use ncrs_intake::hash_path;

use super::*;
use ncrs_fs::transfer::Counting;

use super::fixtures::*;

impl Setup {
    fn preview(&self, scope: &ReplanScope) -> Vec<ReplanItem> {
        preview(
            &self.index,
            scope,
            std::slice::from_ref(&self.inbox),
            KindLanguage::English,
        )
        .unwrap()
    }

    fn file(&mut self, items: &[ReplanItem]) -> ReplanSummary {
        file_previewed(
            &mut self.index,
            &self.archive,
            items,
            &mut Counting,
            &Move,
            KindLanguage::English,
        )
        .unwrap()
    }

    fn target(&self) -> PathBuf {
        self.archive.join("Insurance").join("Invoice.pdf")
    }
}

impl Setup {
    /// A rule on the tags "rechnung" and "firma-a", which no text holds.
    fn add_tag_rule(&mut self) {
        self.index
            .create_rule(&RuleSpec {
                conditions: RuleConditions::all_of(vec![
                    Condition::new(ConditionKind::Tag, "rechnung"),
                    Condition::new(ConditionKind::Tag, "firma-a"),
                ]),
                ..rule_on_text()
            })
            .unwrap();
    }

    /// Gives the document the own tag "rechnung" and the sender Firma A
    /// GmbH, whose tag is "firma-a-gmbh".
    fn tag(&mut self, document: &Document) {
        let fields = DocumentFields {
            sender: Some("Firma A GmbH".to_owned()),
            tags: BTreeSet::from(["rechnung".to_owned()]),
            ..DocumentFields::default()
        };
        self.index
            .set_document_fields(document.id, &fields, |_| ncrs_index::FieldOrigin::Manual)
            .unwrap();
    }
}

fn rule_on_text() -> RuleSpec {
    RuleSpec {
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
        filing_pattern: "Insurance/{kind}.pdf".to_owned(),
        confirmed: true,
        enabled: true,
    }
}

#[test]
fn a_rule_on_tags_files_open_cases_and_a_folder_by_the_tags_of_the_document() {
    let mut setup = Setup::new();
    let tagged = setup.in_review("tagged.pdf", "no word of the rule");
    let untagged = setup.in_review("untagged.pdf", "no word of the rule");
    let in_folder = setup.in_review("sub/folder.pdf", "no word of the rule");
    setup.tag(&tagged);
    setup.tag(&in_folder);
    setup.add_tag_rule();
    let filed = ReplanAfter::Filed(PathBuf::from("Insurance").join("Invoice.pdf"));

    let open_cases = setup.preview(&ReplanScope::OpenCases);
    let after_of = |items: &[ReplanItem], document: &Document| {
        items
            .iter()
            .find(|item| item.document_id == document.id)
            .map(|item| item.after.clone())
    };
    assert_eq!(after_of(&open_cases, &tagged), Some(filed.clone()));
    assert_eq!(
        after_of(&open_cases, &untagged),
        Some(ReplanAfter::Review(ReviewReason::NoMatchingRule))
    );

    let folder = setup.preview(&ReplanScope::Folder(setup.inbox.join("sub")));
    assert_eq!(after_of(&folder, &in_folder), Some(filed));
}

const INVOICE_TEXT: &str = "Invoice from Company A";

#[test]
fn open_cases_are_planned_again_after_a_new_rule() {
    let mut setup = Setup::new();
    let document = setup.in_review("scan.pdf", INVOICE_TEXT);

    let before = setup.preview(&ReplanScope::OpenCases);
    assert!(before
        .iter()
        .all(|item| item.after == ReplanAfter::Review(ReviewReason::NoMatchingRule)));

    setup.add_rule();
    let after = setup.preview(&ReplanScope::OpenCases);
    let item = after
        .iter()
        .find(|item| item.document_id == document.id)
        .unwrap();
    assert_eq!(item.now, Some(ReviewReason::NoMatchingRule));
    assert!(item.review.is_some());
    assert_eq!(
        item.after,
        ReplanAfter::Filed(PathBuf::from("Insurance").join("Invoice.pdf"))
    );
}

#[test]
fn a_document_without_stored_text_is_unread() {
    let mut setup = Setup::new();
    setup.add_rule();
    let path = setup.inbox.join("blank.pdf");
    fs::write(&path, "blank").unwrap();
    let id = setup
        .index
        .insert_document(&NewDocument {
            hash: hash_path(&path).unwrap(),
            path,
            source: DocumentSource::Inbox,
            received_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
            stamp: None,
        })
        .unwrap();
    setup
        .index
        .add_review(id, ReviewReason::NoMatchingRule)
        .unwrap();

    let items = setup.preview(&ReplanScope::OpenCases);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].after, ReplanAfter::Unread);
}

#[test]
fn the_preview_writes_nothing() {
    let mut setup = Setup::new();
    let document = setup.in_review("scan.pdf", INVOICE_TEXT);
    setup.add_rule();
    let origins = setup.index.field_origins(document.id).unwrap();

    let items = setup.preview(&ReplanScope::OpenCases);
    assert_eq!(items.len(), 1);

    let unchanged = setup.index.document(document.id).unwrap().unwrap();
    assert_eq!(unchanged, document);
    assert_eq!(setup.index.field_origins(document.id).unwrap(), origins);
    assert_eq!(setup.index.open_reviews().unwrap().len(), 1);
    assert!(document.path.exists());
    assert!(!setup.target().exists());
    assert!(setup
        .index
        .last_undoable_batch(BatchKind::Filing)
        .unwrap()
        .is_none());
}

#[test]
fn filing_the_previewed_moves_the_file_and_settles_the_review() {
    let mut setup = Setup::new();
    let document = setup.in_review("scan.pdf", INVOICE_TEXT);
    setup.add_rule();
    let items = setup.preview(&ReplanScope::OpenCases);

    let summary = setup.file(&items);

    assert_eq!(
        summary,
        ReplanSummary {
            filed: 1,
            ..ReplanSummary::default()
        }
    );
    let after = setup.index.document(document.id).unwrap().unwrap();
    assert_eq!(after.path, setup.target());
    assert_eq!(after.status, DocumentStatus::Filed);
    assert_eq!(fs::read_to_string(setup.target()).unwrap(), "scan.pdf");
    assert!(!document.path.exists());
    assert!(setup.index.open_reviews().unwrap().is_empty());
    assert!(setup
        .index
        .last_undoable_batch(BatchKind::Filing)
        .unwrap()
        .is_some());
}

#[test]
fn a_document_whose_plan_changed_since_the_preview_is_not_moved() {
    let mut setup = Setup::new();
    let document = setup.in_review("scan.pdf", INVOICE_TEXT);
    setup.add_rule();
    let items = setup.preview(&ReplanScope::OpenCases);
    setup
        .index
        .set_document_text(document.id, "Something else")
        .unwrap();

    let summary = setup.file(&items);

    assert_eq!(
        summary,
        ReplanSummary {
            changed: 1,
            ..ReplanSummary::default()
        }
    );
    assert!(document.path.exists());
    assert!(!setup.target().exists());
    assert_eq!(setup.index.open_reviews().unwrap().len(), 1);
    assert!(setup
        .index
        .last_undoable_batch(BatchKind::Filing)
        .unwrap()
        .is_none());
}

#[test]
fn only_the_documents_the_preview_would_file_are_moved() {
    let mut setup = Setup::new();
    let filed = setup.in_review("a.pdf", INVOICE_TEXT);
    let stays = setup.in_review("b.pdf", "Nothing known");
    setup.add_rule();
    let items = setup.preview(&ReplanScope::OpenCases);
    assert_eq!(items.iter().filter(|item| item.would_be_filed()).count(), 1);

    let summary = setup.file(&items);

    assert_eq!(summary.filed, 1);
    assert!(!filed.path.exists());
    assert!(stays.path.exists());
    assert_eq!(setup.index.open_reviews().unwrap().len(), 1);
}

#[test]
fn a_manual_kind_survives_the_filing() {
    let mut setup = Setup::new();
    let document = setup.in_review("scan.pdf", INVOICE_TEXT);
    setup.add_rule();
    setup
        .index
        .set_document_fields(
            document.id,
            &DocumentFields {
                kind: Some("contract".to_owned()),
                ..DocumentFields::default()
            },
            |_| FieldOrigin::Manual,
        )
        .unwrap();
    let items = setup.preview(&ReplanScope::OpenCases);
    assert_eq!(
        items[0].after,
        ReplanAfter::Filed(PathBuf::from("Insurance").join("Contract.pdf"))
    );

    let summary = setup.file(&items);

    assert_eq!(summary.filed, 1);
    let after = setup.index.document(document.id).unwrap().unwrap();
    assert_eq!(
        after.path,
        setup.archive.join("Insurance").join("Contract.pdf")
    );
    assert_eq!(after.fields.kind.as_deref(), Some("contract"));
    assert_eq!(
        setup
            .index
            .field_origins(document.id)
            .unwrap()
            .of(Field::Kind),
        FieldOrigin::Manual
    );
}

#[test]
fn a_folder_outside_the_inbox_is_refused() {
    let mut setup = Setup::new();
    setup.in_review("scan.pdf", INVOICE_TEXT);
    setup.add_rule();
    let elsewhere = setup.archive.clone();

    assert!(!lies_in_inbox(
        &elsewhere,
        std::slice::from_ref(&setup.inbox)
    ));
    assert!(setup.preview(&ReplanScope::Folder(elsewhere)).is_empty());
    assert!(lies_in_inbox(
        &setup.inbox.join("deeper"),
        std::slice::from_ref(&setup.inbox)
    ));
    assert!(!lies_in_inbox(
        &setup.inbox.join("..").join("archive"),
        std::slice::from_ref(&setup.inbox)
    ));
}

#[test]
fn a_folder_scope_takes_only_the_documents_below_it() {
    let mut setup = Setup::new();
    let inside = setup.in_review("sub/a.pdf", INVOICE_TEXT);
    setup.in_review("sub2/b.pdf", INVOICE_TEXT);
    setup.in_review("c.pdf", INVOICE_TEXT);
    setup.add_rule();

    let items = setup.preview(&ReplanScope::Folder(setup.inbox.join("sub")));

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].document_id, inside.id);
    assert!(items[0].would_be_filed());
}

#[test]
fn a_folder_scope_takes_documents_not_in_review_but_not_the_filed_ones() {
    let mut setup = Setup::new();
    let waiting = setup.in_review("sub/a.pdf", INVOICE_TEXT);
    let done = setup.in_review("sub/b.pdf", INVOICE_TEXT);
    setup
        .index
        .set_document_status(done.id, DocumentStatus::Filed)
        .unwrap();
    setup.add_rule();

    let items = setup.preview(&ReplanScope::Folder(setup.inbox.join("sub")));

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].document_id, waiting.id);
}
