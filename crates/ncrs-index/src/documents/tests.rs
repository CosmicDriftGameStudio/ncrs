use chrono::NaiveDate;

use super::*;
use crate::model::Currency;
use crate::test_support::{encrypted_index, new_document, plain_index};

fn indexed_with_text(index: &mut Index, seed: u8, text: &str) -> DocumentId {
    let id = index.insert_document(&new_document(seed)).unwrap();
    index.set_document_text(id, text).unwrap();
    id
}

#[test]
fn a_snippet_marks_the_match_through_the_folded_diacritics() {
    let (_dir, mut index) = encrypted_index();
    let id = indexed_with_text(&mut index, 1, "Invoice for Herr Müller of Berlin");

    let snippets = index.text_snippets(&[id], "Muller").unwrap();

    assert_eq!(
        snippets,
        [(
            id,
            format!("Invoice for Herr {SNIPPET_MARK_START}Müller{SNIPPET_MARK_END} of Berlin")
        )]
    );
}

#[test]
fn a_document_the_words_do_not_match_gets_no_snippet() {
    let (_dir, mut index) = encrypted_index();
    let matching = indexed_with_text(&mut index, 1, "rent for the office");
    let other = indexed_with_text(&mut index, 2, "insurance policy");

    let snippets = index.text_snippets(&[matching, other], "rent").unwrap();

    assert_eq!(snippets.len(), 1);
    assert_eq!(snippets[0].0, matching);
    assert!(index.text_snippets(&[matching], "  ").unwrap().is_empty());
    assert!(index.text_snippets(&[], "rent").unwrap().is_empty());
}

#[test]
fn several_matching_documents_get_their_snippets_in_the_order_asked() {
    let (_dir, mut index) = encrypted_index();
    let first = indexed_with_text(&mut index, 1, "rent for the office");
    let second = indexed_with_text(&mut index, 2, "the rent of the flat");
    let third = indexed_with_text(&mut index, 3, "rent due in May");

    let snippets = index
        .text_snippets(&[third, first, second], "rent")
        .unwrap();

    let ids: Vec<_> = snippets.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, [third, first, second]);
    assert!(snippets.iter().all(
        |(_, snippet)| snippet.contains(&format!("{SNIPPET_MARK_START}rent{SNIPPET_MARK_END}"))
    ));
}

#[test]
fn inserted_document_round_trips_with_its_fields() {
    let (_dir, mut index) = encrypted_index();
    let new = new_document(1);
    let id = index.insert_document(&new).unwrap();
    let fields = DocumentFields {
        sender: Some("Company A".to_owned()),
        kind: Some("invoice".to_owned()),
        date: NaiveDate::from_ymd_opt(2026, 2, 28),
        number: Some("4711".to_owned()),
        amount: Some(Amount {
            minor_units: 12_345,
            currency: Currency::new("EUR").unwrap(),
        }),
        recipient: Some("Erika Beispiel".to_owned()),
        tags: BTreeSet::from(["services".to_owned(), "2026".to_owned()]),
    };
    index
        .set_document_fields(id, &fields, |_| FieldOrigin::Unknown)
        .unwrap();

    let expected = Document {
        id,
        hash: new.hash,
        path: new.path.clone(),
        source: new.source,
        received_at: new.received_at,
        status: DocumentStatus::New,
        fields,
        stamp: None,
        missing_since: None,
    };
    assert_eq!(
        index.document_by_hash(&new.hash).unwrap(),
        Some(expected.clone())
    );
    assert_eq!(index.document(id).unwrap(), Some(expected));
    assert_eq!(index.document(DocumentId::new(id.get() + 1)).unwrap(), None);
}

#[test]
fn new_document_has_no_fields_and_status_can_change() {
    let (_dir, index) = plain_index();
    let id = index.insert_document(&new_document(1)).unwrap();
    assert_eq!(
        index.document(id).unwrap().unwrap().fields,
        DocumentFields::default()
    );

    index
        .set_document_status(id, DocumentStatus::Filed)
        .unwrap();

    assert_eq!(
        index.document(id).unwrap().unwrap().status,
        DocumentStatus::Filed
    );
}

#[test]
fn second_insert_with_the_same_hash_reports_the_first_id() {
    let (_dir, index) = encrypted_index();
    let first = index.insert_document(&new_document(1)).unwrap();

    let err = index.insert_document(&new_document(1)).unwrap_err();

    assert!(
        matches!(err, IndexError::DuplicateHash(id) if id == first),
        "{err}"
    );
}

#[test]
fn setters_report_an_unknown_document() {
    let (_dir, mut index) = encrypted_index();
    let missing = DocumentId::new(99);

    let status = index.set_document_status(missing, DocumentStatus::Filed);
    let text = index.set_document_text(missing, "x");
    let fields = index.set_document_fields(
        missing,
        &DocumentFields {
            sender: Some("Company A".to_owned()),
            ..DocumentFields::default()
        },
        |_| FieldOrigin::Unknown,
    );

    for result in [status, text, fields] {
        assert!(
            matches!(result, Err(IndexError::DocumentNotFound(id)) if id == missing),
            "{result:?}"
        );
    }
    let senders: i64 = index
        .connection()
        .query_row("SELECT count(*) FROM sender", [], |row| row.get(0))
        .unwrap();
    assert_eq!(senders, 0);
}

#[test]
fn setting_fields_again_replaces_the_tags() {
    let (_dir, mut index) = encrypted_index();
    let id = index.insert_document(&new_document(1)).unwrap();
    let tagged = DocumentFields {
        tags: BTreeSet::from(["a".to_owned(), "b".to_owned()]),
        ..DocumentFields::default()
    };
    index
        .set_document_fields(id, &tagged, |_| FieldOrigin::Unknown)
        .unwrap();
    let only_b = DocumentFields {
        tags: BTreeSet::from(["b".to_owned()]),
        ..DocumentFields::default()
    };

    index
        .set_document_fields(id, &only_b, |_| FieldOrigin::Unknown)
        .unwrap();

    assert_eq!(index.document(id).unwrap().unwrap().fields, only_b);
}

fn tag_rows(index: &Index, id: DocumentId) -> Vec<(String, i64)> {
    let mut statement = index
        .connection()
        .prepare(
            "SELECT t.name, dt.automatic FROM document_tag dt
             JOIN tag t ON t.id = dt.tag_id WHERE dt.document_id = ?1 ORDER BY t.name",
        )
        .unwrap();
    statement
        .query_map([id.get()], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn invoice_of(sender: &str, own_tags: &[&str]) -> DocumentFields {
    DocumentFields {
        sender: Some(sender.to_owned()),
        kind: Some("Rechnung".to_owned()),
        date: NaiveDate::from_ymd_opt(2026, 2, 28),
        tags: own_tags.iter().map(|tag| (*tag).to_owned()).collect(),
        ..DocumentFields::default()
    }
}

#[test]
fn automatic_tags_follow_a_field_change() {
    let (_dir, mut index) = encrypted_index();
    let id = index.insert_document(&new_document(1)).unwrap();
    index
        .set_document_fields(id, &invoice_of("Firma A", &[]), |_| FieldOrigin::Unknown)
        .unwrap();
    assert_eq!(
        tag_rows(&index, id),
        vec![
            ("2026".to_owned(), 1),
            ("firma-a".to_owned(), 1),
            ("invoice".to_owned(), 1)
        ]
    );

    index
        .set_document_fields(id, &invoice_of("Firma B", &[]), |_| FieldOrigin::Unknown)
        .unwrap();

    let names: Vec<String> = tag_rows(&index, id).into_iter().map(|row| row.0).collect();
    assert_eq!(names, ["2026", "firma-b", "invoice"]);
}

#[test]
fn own_tags_survive_a_rewrite_and_are_the_only_ones_read() {
    let (_dir, mut index) = encrypted_index();
    let id = index.insert_document(&new_document(1)).unwrap();
    let fields = invoice_of("Firma A", &["Steuer"]);
    index
        .set_document_fields(id, &fields, |_| FieldOrigin::Unknown)
        .unwrap();
    index
        .set_document_fields(id, &fields, |_| FieldOrigin::Unknown)
        .unwrap();

    let read = index.document(id).unwrap().unwrap().fields;

    assert_eq!(read.tags, BTreeSet::from(["steuer".to_owned()]));
    assert!(tag_rows(&index, id).contains(&("steuer".to_owned(), 0)));
}

#[test]
fn an_own_tag_equal_to_an_automatic_one_stays_own() {
    let (_dir, mut index) = encrypted_index();
    let id = index.insert_document(&new_document(1)).unwrap();
    index
        .set_document_fields(id, &invoice_of("Firma A", &["invoice"]), |_| {
            FieldOrigin::Unknown
        })
        .unwrap();

    assert!(tag_rows(&index, id).contains(&("invoice".to_owned(), 0)));
    assert_eq!(
        index.document(id).unwrap().unwrap().fields.tags,
        BTreeSet::from(["invoice".to_owned()])
    );
}

#[test]
fn a_changed_recipient_gets_the_origin_given() {
    let (_dir, mut index) = encrypted_index();
    let id = index.insert_document(&new_document(1)).unwrap();
    let fields = DocumentFields {
        recipient: Some("Erika Beispiel".to_owned()),
        ..DocumentFields::default()
    };
    index
        .set_document_fields(id, &fields, |_| FieldOrigin::AddressBlock)
        .unwrap();

    let origins = index.field_origins(id).unwrap();

    assert_eq!(origins.of(Field::Recipient), FieldOrigin::AddressBlock);
}

#[test]
fn full_text_is_found_by_word_number_and_several_words() {
    let (_dir, mut index) = encrypted_index();
    let id = indexed_with_text(&mut index, 1, "Rechnung Nr. 4711 von Company A");
    indexed_with_text(&mut index, 2, "Mahnung Nr. 12 von Agency B");

    assert_eq!(index.search_text("rechnung", 10).unwrap(), vec![id]);
    assert_eq!(index.search_text("4711", 10).unwrap(), vec![id]);
    assert_eq!(index.search_text("company  4711", 10).unwrap(), vec![id]);
    assert!(index.search_text("rechnung agency", 10).unwrap().is_empty());
    assert!(index.search_text("absent", 10).unwrap().is_empty());
    assert!(index.search_text("   ", 10).unwrap().is_empty());
    assert_eq!(index.search_text("nr", 1).unwrap().len(), 1);
}

#[test]
fn missing_documents_do_not_use_up_the_limit() {
    let (_dir, mut index) = encrypted_index();
    for n in 1..=3 {
        let gone = indexed_with_text(&mut index, n, "Rechnung Nr. 1");
        index.mark_document_missing(gone).unwrap();
    }
    let present = indexed_with_text(&mut index, 4, "Rechnung Nr. 2");

    assert_eq!(index.search_text("rechnung", 1).unwrap(), vec![present]);
    assert_eq!(index.search_text_prefix("rechn", 1).unwrap(), vec![present]);
}

#[test]
fn the_prefix_search_finds_a_word_by_its_beginning() {
    let (_dir, mut index) = encrypted_index();
    let id = indexed_with_text(&mut index, 1, "Rechnung Nr. 4711 von Company A");
    indexed_with_text(&mut index, 2, "Mahnung Nr. 12 von Agency B");

    assert_eq!(index.search_text_prefix("rechn", 10).unwrap(), vec![id]);
    assert_eq!(
        index.search_text_prefix("company rech", 10).unwrap(),
        vec![id]
    );
    assert!(index.search_text("rechn", 10).unwrap().is_empty());
    assert!(index
        .search_text_prefix("rech agency", 10)
        .unwrap()
        .is_empty());
}

#[test]
fn the_prefix_search_folds_diacritics_and_quotes_its_input() {
    let (_dir, mut index) = encrypted_index();
    let id = indexed_with_text(&mut index, 1, "Gebühr für Januar");

    assert_eq!(index.search_text_prefix("gebu", 10).unwrap(), vec![id]);
    assert_eq!(index.search_text_prefix("Gebüh", 10).unwrap(), vec![id]);
    for input in [
        "\"",
        "AND",
        "col:x",
        "*",
        "-x",
        "\"unterminated",
        "NEAR(a b)",
    ] {
        index.search_text_prefix(input, 10).unwrap();
    }
}

#[test]
fn the_prefix_search_of_nothing_finds_nothing() {
    let (_dir, mut index) = encrypted_index();
    indexed_with_text(&mut index, 1, "Rechnung");

    assert!(index.search_text_prefix("", 10).unwrap().is_empty());
    assert!(index.search_text_prefix("   ", 10).unwrap().is_empty());
}

#[test]
fn full_text_folds_diacritics() {
    let (_dir, mut index) = encrypted_index();
    let id = indexed_with_text(&mut index, 1, "Gebühr für Januar");

    assert_eq!(index.search_text("gebuhr", 10).unwrap(), vec![id]);
    assert_eq!(index.search_text("Gebühr", 10).unwrap(), vec![id]);
}

#[test]
fn hostile_search_input_is_plain_words_and_never_an_error() {
    let (_dir, mut index) = encrypted_index();
    indexed_with_text(&mut index, 1, "Rechnung and not x");

    for input in [
        "\"",
        "AND",
        "NOT x",
        "col:x",
        "*",
        "-",
        "-x",
        "\"unterminated",
        "( )",
        "NEAR(a b)",
    ] {
        index.search_text(input, 10).unwrap();
    }
}

#[test]
fn the_stored_text_is_read_back() {
    let (_dir, mut index) = encrypted_index();
    let id = indexed_with_text(&mut index, 1, "Company A, Invoice 4711");
    let without = index.insert_document(&new_document(2)).unwrap();

    assert_eq!(
        index.document_text(id).unwrap().as_deref(),
        Some("Company A, Invoice 4711")
    );
    assert_eq!(index.document_text(without).unwrap(), None);
}

#[test]
fn setting_the_text_again_replaces_it() {
    let (_dir, mut index) = encrypted_index();
    let id = indexed_with_text(&mut index, 1, "alpha");

    index.set_document_text(id, "beta").unwrap();

    assert!(index.search_text("alpha", 10).unwrap().is_empty());
    assert_eq!(index.search_text("beta", 10).unwrap(), vec![id]);
}

#[test]
fn deleting_a_document_removes_its_tags_reviews_and_text() {
    let (_dir, mut index) = encrypted_index();
    let id = indexed_with_text(&mut index, 1, "Rechnung");
    index
        .set_document_fields(
            id,
            &DocumentFields {
                tags: BTreeSet::from(["a".to_owned()]),
                ..DocumentFields::default()
            },
            |_| FieldOrigin::Unknown,
        )
        .unwrap();
    index
        .add_review(id, crate::ReviewReason::NoMatchingRule)
        .unwrap();

    index
        .connection()
        .execute("DELETE FROM document WHERE id = ?1", [id.get()])
        .unwrap();

    let count = |table: &str| -> i64 {
        index
            .connection()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    };
    assert_eq!(count("document_tag"), 0);
    assert_eq!(count("review_entry"), 0);
    assert!(index.search_text("rechnung", 10).unwrap().is_empty());

    // The deleted id is reused; the old text must not find the new document.
    let reused = index.insert_document(&new_document(2)).unwrap();
    assert_eq!(reused, id);
    assert!(index.search_text("rechnung", 10).unwrap().is_empty());
}

fn all_fields() -> DocumentFields {
    DocumentFields {
        sender: Some("Company A".to_owned()),
        recipient: Some("Erika Beispiel".to_owned()),
        kind: Some("invoice".to_owned()),
        date: NaiveDate::from_ymd_opt(2026, 2, 28),
        number: Some("4711".to_owned()),
        amount: Some(Amount {
            minor_units: 12_345,
            currency: Currency::new("EUR").unwrap(),
        }),
        tags: BTreeSet::new(),
    }
}

fn all_origins(index: &Index, id: DocumentId) -> Vec<FieldOrigin> {
    let origins = index.field_origins(id).unwrap();
    Field::ALL
        .into_iter()
        .map(|field| origins.of(field))
        .collect()
}

#[test]
fn a_fresh_document_reads_every_field_origin_as_unknown() {
    let (_dir, mut index) = encrypted_index();
    let id = index.insert_document(&new_document(1)).unwrap();

    assert_eq!(all_origins(&index, id), [const { FieldOrigin::Unknown }; 6]);
    index
        .set_document_fields(id, &all_fields(), |_| FieldOrigin::Unknown)
        .unwrap();
    assert_eq!(all_origins(&index, id), [const { FieldOrigin::Unknown }; 6]);
}

#[test]
fn the_manual_field_origins_are_listed_in_field_order() {
    let (_dir, mut index) = encrypted_index();
    let id = index.insert_document(&new_document(1)).unwrap();
    assert!(index.field_origins(id).unwrap().manual_fields().is_empty());

    index
        .set_document_fields(id, &all_fields(), |field| match field {
            Field::Date | Field::Kind => FieldOrigin::Manual,
            _ => FieldOrigin::Rule(None),
        })
        .unwrap();

    assert_eq!(
        index.field_origins(id).unwrap().manual_fields(),
        vec![Field::Kind, Field::Date]
    );
}

#[test]
fn each_kind_of_origin_round_trips() {
    let (_dir, mut index) = encrypted_index();
    let rule = index
        .create_rule(&crate::test_support::rule_spec())
        .unwrap();
    let id = index.insert_document(&new_document(1)).unwrap();

    index
        .set_document_fields(id, &all_fields(), |field| match field {
            Field::Sender => FieldOrigin::Rule(Some(rule)),
            Field::Recipient => FieldOrigin::AddressBlock,
            Field::Kind => FieldOrigin::Ai {
                confidence_percent: 0,
            },
            Field::Date => FieldOrigin::Ai {
                confidence_percent: 100,
            },
            Field::Number => FieldOrigin::Manual,
            Field::Amount => FieldOrigin::Unknown,
        })
        .unwrap();

    assert_eq!(
        all_origins(&index, id),
        [
            FieldOrigin::Rule(Some(rule)),
            FieldOrigin::AddressBlock,
            FieldOrigin::Ai {
                confidence_percent: 0
            },
            FieldOrigin::Ai {
                confidence_percent: 100
            },
            FieldOrigin::Manual,
            FieldOrigin::Unknown,
        ]
    );
}

#[test]
fn an_unchanged_field_keeps_its_origin_and_a_changed_one_takes_the_new_origin() {
    let (_dir, mut index) = encrypted_index();
    let id = index.insert_document(&new_document(1)).unwrap();
    index
        .set_document_fields(id, &all_fields(), |_| FieldOrigin::Manual)
        .unwrap();

    let mut edited = all_fields();
    edited.number = Some("4712".to_owned());
    edited.amount = Some(Amount {
        minor_units: 12_345,
        currency: Currency::new("USD").unwrap(),
    });
    index
        .set_document_fields(id, &edited, |_| FieldOrigin::Ai {
            confidence_percent: 90,
        })
        .unwrap();

    let ai = FieldOrigin::Ai {
        confidence_percent: 90,
    };
    assert_eq!(
        all_origins(&index, id),
        [
            FieldOrigin::Manual,
            FieldOrigin::Manual,
            FieldOrigin::Manual,
            FieldOrigin::Manual,
            ai.clone(),
            ai.clone(),
        ]
    );
}

#[test]
fn a_changed_field_set_with_an_unknown_origin_loses_its_old_origin() {
    let (_dir, mut index) = encrypted_index();
    let id = index.insert_document(&new_document(1)).unwrap();
    index
        .set_document_fields(id, &all_fields(), |_| FieldOrigin::Manual)
        .unwrap();

    let mut edited = all_fields();
    edited.sender = Some("Company B".to_owned());
    index
        .set_document_fields(id, &edited, |_| FieldOrigin::Unknown)
        .unwrap();

    let origins = index.field_origins(id).unwrap();
    assert_eq!(origins.of(Field::Sender), FieldOrigin::Unknown);
    assert_eq!(origins.of(Field::Kind), FieldOrigin::Manual);
}

#[test]
fn clearing_a_field_makes_its_origin_unknown() {
    let (_dir, mut index) = encrypted_index();
    let id = index.insert_document(&new_document(1)).unwrap();
    index
        .set_document_fields(id, &all_fields(), |_| FieldOrigin::Manual)
        .unwrap();

    let mut cleared = all_fields();
    cleared.date = None;
    cleared.amount = None;
    index
        .set_document_fields(id, &cleared, |_| FieldOrigin::Manual)
        .unwrap();

    let origins = index.field_origins(id).unwrap();
    assert_eq!(origins.of(Field::Date), FieldOrigin::Unknown);
    assert_eq!(origins.of(Field::Amount), FieldOrigin::Unknown);
    assert_eq!(origins.of(Field::Sender), FieldOrigin::Manual);
}

#[test]
fn deleting_the_rule_leaves_a_rule_origin_without_a_rule() {
    let (_dir, mut index) = encrypted_index();
    let rule = index
        .create_rule(&crate::test_support::rule_spec())
        .unwrap();
    let id = index.insert_document(&new_document(1)).unwrap();
    index
        .set_document_fields(id, &all_fields(), |_| FieldOrigin::Rule(Some(rule)))
        .unwrap();

    assert!(index.delete_rule(rule).unwrap());

    assert_eq!(
        all_origins(&index, id),
        [const { FieldOrigin::Rule(None) }; 6]
    );
}

#[test]
fn deleting_a_document_removes_its_field_origins() {
    let (_dir, mut index) = encrypted_index();
    let id = index.insert_document(&new_document(1)).unwrap();
    index
        .set_document_fields(id, &all_fields(), |_| FieldOrigin::Manual)
        .unwrap();

    index
        .connection()
        .execute("DELETE FROM document WHERE id = ?1", [id.get()])
        .unwrap();

    let remaining: i64 = index
        .connection()
        .query_row("SELECT count(*) FROM document_field_origin", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(remaining, 0);
}

#[cfg(unix)]
#[test]
fn a_path_that_is_not_utf8_is_refused() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;

    let (_dir, index) = encrypted_index();
    let mut document = new_document(1);
    document.path = PathBuf::from(OsStr::from_bytes(b"inbox/caf\xE9.pdf"));

    let err = index.insert_document(&document).unwrap_err();

    assert!(
        matches!(&err, IndexError::NonUtf8Path(path) if *path == document.path),
        "{err}"
    );
}

#[test]
fn names_are_trimmed_and_compared_without_case() {
    let (_dir, mut index) = encrypted_index();
    for (seed, name) in [(1, "  Company A "), (2, "Company A"), (3, "company a")] {
        let id = index.insert_document(&new_document(seed)).unwrap();
        let fields = DocumentFields {
            sender: Some(name.to_owned()),
            ..DocumentFields::default()
        };
        index
            .set_document_fields(id, &fields, |_| FieldOrigin::Unknown)
            .unwrap();
    }

    let senders: i64 = index
        .connection()
        .query_row("SELECT count(*) FROM sender", [], |row| row.get(0))
        .unwrap();
    assert_eq!(senders, 1);
}

#[test]
fn names_with_non_ascii_letters_are_one_sender_and_keep_the_first_spelling() {
    let (_dir, mut index) = encrypted_index();
    let spellings = ["Ärztekammer", "ÄRZTEKAMMER", " ärztekammer "];
    for (seed, name) in (1..).zip(spellings) {
        let id = index.insert_document(&new_document(seed)).unwrap();
        let fields = DocumentFields {
            sender: Some(name.to_owned()),
            tags: BTreeSet::from([name.to_owned()]),
            ..DocumentFields::default()
        };
        index
            .set_document_fields(id, &fields, |_| FieldOrigin::Unknown)
            .unwrap();
    }

    let stored = |table: &str| -> Vec<String> {
        let mut statement = index
            .connection()
            .prepare(&format!("SELECT name FROM {table}"))
            .unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(stored("sender"), vec!["Ärztekammer".to_owned()]);
    assert_eq!(stored("tag"), vec!["ärztekammer".to_owned()]);
    let last = index.document(DocumentId::new(3)).unwrap().unwrap();
    assert_eq!(last.fields.sender.as_deref(), Some("Ärztekammer"));
}

#[test]
fn an_empty_name_is_refused() {
    let (_dir, mut index) = encrypted_index();
    let id = index.insert_document(&new_document(1)).unwrap();

    let err = index
        .set_document_fields(
            id,
            &DocumentFields {
                sender: Some("   ".to_owned()),
                ..DocumentFields::default()
            },
            |_| FieldOrigin::Unknown,
        )
        .unwrap_err();

    assert!(
        matches!(err, IndexError::EmptyName { what: "sender" }),
        "{err}"
    );
}

#[test]
fn an_unknown_stored_status_is_an_error_not_a_panic() {
    let (_dir, index) = encrypted_index();
    let id = index.insert_document(&new_document(1)).unwrap();
    index
        .connection()
        .execute("UPDATE document SET status = 'archived'", [])
        .unwrap();

    let err = index.document(id).unwrap_err();

    assert!(
        matches!(&err, IndexError::CorruptValue { column: "document.status", value } if value == "archived"),
        "{err}"
    );
}
