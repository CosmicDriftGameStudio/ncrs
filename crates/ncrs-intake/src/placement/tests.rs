#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use chrono::NaiveDate;
use ncrs_index::{ContentHash, KindCatalog, SenderAliases};
use sha2::{Digest as _, Sha256};

use super::*;
use crate::field_detection::detect_fields;
use crate::reorganisation_plan::location_structure;
use crate::structure::UnorderedFolder;
use crate::test_archive::{test_archive, TestFile, OWN_NAME};

const OWN_IBAN: &str = "DE89 3704 0044 0532 0130 00";

fn no_own() -> OwnNames<'static> {
    OwnNames {
        names: &[],
        ibans: &[],
        tax_ids: &[],
    }
}

fn ordered() -> LocationStructure {
    LocationStructure {
        areas: Vec::new(),
        unordered: Vec::new(),
    }
}

fn unordered_in(folder: &str) -> LocationStructure {
    LocationStructure {
        areas: Vec::new(),
        unordered: vec![UnorderedFolder {
            folder: folder.to_owned(),
            documents: 1,
        }],
    }
}

fn fields(sender: Option<&str>, kind: Option<&str>) -> DocumentFields {
    DocumentFields {
        sender: sender.map(str::to_owned),
        kind: kind.map(str::to_owned),
        ..DocumentFields::default()
    }
}

fn file(path: &str, seed: u8, fields: DocumentFields, text: Option<&str>) -> PlanFile {
    PlanFile {
        relative_path: PathBuf::from(path),
        hash: ContentHash([seed; 32]),
        fields,
        text: text.map(str::to_owned),
        recorded_at: i64::from(seed),
    }
}

/// `folder/<n>.pdf` for each of `seeds`, all with the same fields and text.
fn filed(
    folder: &str,
    seeds: std::ops::Range<u8>,
    fields: &DocumentFields,
    text: Option<&str>,
) -> Vec<PlanFile> {
    seeds
        .map(|seed| file(&format!("{folder}/{seed}.pdf"), seed, fields.clone(), text))
        .collect()
}

fn place(
    documents: &[PlanFile],
    structure: &LocationStructure,
    document: &DocumentFields,
    text: Option<&str>,
) -> Option<Placement> {
    NeighbourIndex::new(documents, &no_own(), structure).place(document, text)
}

#[test]
fn two_documents_with_the_same_labelled_number_decide_before_anything_weaker() {
    let sender = fields(Some("Bank"), Some("Letter"));
    let documents = filed(
        "Loans/Flat",
        1..3,
        &sender,
        Some("Darlehensnummer: 595610814"),
    );

    let other_label = place(
        &documents,
        &ordered(),
        &fields(None, None),
        Some("Loan number - 595610814"),
    );
    assert_eq!(other_label, None, "another label is another number");

    let placement = place(
        &documents,
        &ordered(),
        &fields(None, None),
        Some("Darlehensnummer 595610814"),
    )
    .unwrap();
    assert_eq!(placement.folder, Path::new("Loans/Flat"));
    assert_eq!(
        placement.signal,
        Signal::SameNumber {
            label: "Darlehensnummer".to_owned(),
            value: "595610814".to_owned()
        }
    );
    assert!(placement.is_sure());
    assert_eq!((placement.agreeing, placement.similar), (2, 2));
}

#[test]
fn one_document_with_the_number_is_not_enough() {
    let documents = filed(
        "Loans/Flat",
        1..2,
        &fields(None, None),
        Some("Kundennummer 4711002"),
    );

    assert_eq!(
        place(
            &documents,
            &ordered(),
            &fields(None, None),
            Some("Kundennummer 4711002")
        ),
        None
    );
}

#[test]
fn the_documents_of_a_number_in_several_subfolders_are_placed_in_the_folder_above() {
    let none = fields(None, None);
    let mut documents = filed(
        "Loans/Flat/Draft",
        1..4,
        &none,
        Some("Darlehensnummer 123456"),
    );
    documents.extend(filed(
        "Loans/Flat/Signed",
        4..8,
        &none,
        Some("Darlehensnummer 123456"),
    ));
    documents.extend(filed(
        "Loans/House",
        8..9,
        &none,
        Some("Darlehensnummer 123456"),
    ));

    let placement = place(
        &documents,
        &ordered(),
        &none,
        Some("Darlehensnummer 123456"),
    )
    .unwrap();

    assert_eq!(placement.folder, Path::new("Loans/Flat"));
    assert_eq!((placement.agreeing, placement.similar), (7, 8));
    assert!(placement.is_sure());
}

#[test]
fn a_plate_with_a_space_is_one_value() {
    let none = fields(None, None);
    let mut documents = filed(
        "Car/Golf",
        1..3,
        &none,
        Some("Kennzeichen C-AB 123\nFahrgestell"),
    );
    documents.extend(filed(
        "Car/Octavia",
        3..5,
        &none,
        Some("Kennzeichen C-XY 987"),
    ));

    let placement = place(&documents, &ordered(), &none, Some("Kennzeichen c-ab 123")).unwrap();

    assert_eq!(placement.folder, Path::new("Car/Golf"));
    assert_eq!(placement.similar, 2);
}

#[test]
fn the_same_iban_decides_when_no_number_does() {
    let none = fields(None, None);
    let documents = filed(
        "Bank/Statements",
        1..3,
        &none,
        Some("IBAN DE12 8705 0000 0712 3456 78"),
    );

    let placement = place(
        &documents,
        &ordered(),
        &none,
        Some("Kontoauszug\nIBAN DE12870500000712345678"),
    )
    .unwrap();

    assert_eq!(placement.signal, Signal::SameIban);
    assert_eq!(placement.folder, Path::new("Bank/Statements"));
}

#[test]
fn the_own_iban_never_matches() {
    let none = fields(None, None);
    let text = format!("Überweisung von {OWN_IBAN}");
    let documents = filed("Misc", 1..5, &none, Some(&text));
    let own_ibans = [OWN_IBAN.to_owned()];
    let own = OwnNames {
        names: &[],
        ibans: &own_ibans,
        tax_ids: &[],
    };

    let index = NeighbourIndex::new(&documents, &own, &ordered());

    assert_eq!(index.place(&none, Some(&text)), None);
    assert_eq!(
        NeighbourIndex::new(&documents, &no_own(), &ordered())
            .place(&none, Some(&text))
            .map(|placement| placement.signal),
        Some(Signal::SameIban)
    );
}

#[test]
fn the_same_sender_and_kind_need_three_documents() {
    let invoice = fields(Some("Firma"), Some("Invoice"));
    let two = filed("Invoices", 1..3, &invoice, None);
    let three = filed("Invoices", 1..4, &invoice, None);

    assert_eq!(place(&two, &ordered(), &invoice, None), None);
    let placement = place(&three, &ordered(), &invoice, None).unwrap();
    assert_eq!(placement.signal, Signal::SameSenderAndKind);
    assert_eq!(placement.folder, Path::new("Invoices"));
}

#[test]
fn the_same_sender_counts_without_a_kind() {
    let documents = filed("Firma", 1..4, &fields(Some("Firma"), None), None);

    let placement = place(
        &documents,
        &ordered(),
        &fields(Some("Firma"), Some("Invoice")),
        None,
    )
    .unwrap();

    assert_eq!(placement.signal, Signal::SameSender);
    assert_eq!(placement.folder, Path::new("Firma"));
    assert!(placement.is_sure());
}

#[test]
fn a_sender_split_five_to_five_is_unsure() {
    let firma = fields(Some("Firma"), None);
    let mut documents = filed("Alpha", 1..6, &firma, None);
    documents.extend(filed("Beta", 6..11, &firma, None));

    let placement = place(&documents, &ordered(), &firma, None).unwrap();

    assert!(!placement.is_sure());
    assert_eq!((placement.agreeing, placement.similar), (5, 10));
    assert_eq!(placement.folder, Path::new("Alpha"));
    assert_eq!(placement.pattern, None);
}

#[test]
fn an_unsure_sender_is_narrowed_to_the_kind_before_giving_up() {
    let mut documents = filed(
        "Invoices",
        1..3,
        &fields(Some("Firma"), Some("Invoice")),
        None,
    );
    documents.extend(filed(
        "Letters",
        3..6,
        &fields(Some("Firma"), Some("Letter")),
        None,
    ));

    let placement = place(
        &documents,
        &ordered(),
        &fields(Some("Firma"), Some("Invoice")),
        None,
    )
    .unwrap();

    assert!(placement.is_sure());
    assert_eq!(placement.signal, Signal::SameSenderAndKind);
    assert_eq!(placement.folder, Path::new("Invoices"));
    assert_eq!((placement.agreeing, placement.similar), (2, 2));
}

#[test]
fn a_weaker_tier_is_not_tried_when_the_stronger_one_is_unsure() {
    let none = fields(None, None);
    let firma = fields(Some("Firma"), None);
    let mut documents = filed("Alpha", 1..3, &firma, Some("Kundennummer 99887766"));
    documents.extend(filed("Beta", 3..5, &firma, Some("Kundennummer 99887766")));
    documents.extend(filed("Gamma", 5..9, &firma, None));

    let placement = place(
        &documents,
        &ordered(),
        &fields(Some("Firma"), None),
        Some("Kundennummer 99887766"),
    )
    .unwrap();
    assert!(!placement.is_sure());
    assert!(matches!(placement.signal, Signal::SameNumber { .. }));
    assert_eq!(place(&[], &ordered(), &none, None), None);
}

#[test]
fn documents_in_unordered_folders_are_never_neighbours() {
    let firma = fields(Some("Firma"), None);
    let mut documents = filed("Scans", 1..9, &firma, Some("Kundennummer 99887766"));
    documents.extend(filed("Firma", 9..11, &firma, None));

    let placement = place(
        &documents,
        &unordered_in("Scans"),
        &firma,
        Some("Kundennummer 99887766"),
    );

    assert_eq!(
        placement, None,
        "two ordered documents of the sender are too few"
    );
}

#[test]
fn a_shared_number_does_not_give_the_name_to_a_document_of_another_kind() {
    let text = Some("Kundennummer 99887766");
    let statement = |kind: &str, month: u32| DocumentFields {
        sender: Some("Bank".to_owned()),
        kind: Some(kind.to_owned()),
        date: NaiveDate::from_ymd_opt(2021, month, 1),
        ..DocumentFields::default()
    };
    let documents: Vec<PlanFile> = (1..=6)
        .map(|number: u8| {
            let month = u32::from(number);
            let path = format!("Statements/2021/2021-{month:02}-01_Statement.pdf");
            file(&path, number, statement("Statement", month), text)
        })
        .collect();

    let same_kind = place(&documents, &ordered(), &statement("Statement", 7), text).unwrap();
    let other_kind = place(&documents, &ordered(), &statement("Letter", 7), text).unwrap();

    assert!(same_kind.pattern.is_some(), "{same_kind:?}");
    assert_eq!(other_kind.pattern, None);
    assert_eq!(other_kind.folder, same_kind.folder);
    assert!(other_kind.is_sure());
}

#[test]
fn copies_of_one_file_count_once() {
    let none = fields(None, None);
    let text = Some("Darlehensnummer 123456");
    let documents = vec![
        file("Loans/a.pdf", 1, none.clone(), text),
        file("Tax/Belege/a.pdf", 1, none.clone(), text),
    ];

    assert_eq!(place(&documents, &ordered(), &none, text), None);
}

#[test]
fn the_pattern_of_the_folders_is_offered_with_the_field_it_needs() {
    let dated = |year: i32, month: u32| DocumentFields {
        sender: Some("Firma".to_owned()),
        kind: Some("Invoice".to_owned()),
        date: NaiveDate::from_ymd_opt(year, month, 1),
        ..DocumentFields::default()
    };
    let documents: Vec<PlanFile> = (1..=6)
        .map(|number: u8| {
            let (year, month) = (2020 + i32::from(number % 2), u32::from(number));
            let path = format!("Invoices/{year}/{year}-{month:02}-01_Invoice.pdf");
            file(&path, number, dated(year, month), None)
        })
        .collect();

    let complete = place(&documents, &ordered(), &dated(2022, 5), None).unwrap();
    let without_date = place(
        &documents,
        &ordered(),
        &fields(Some("Firma"), Some("Invoice")),
        None,
    )
    .unwrap();

    assert_eq!(complete.folder, Path::new("Invoices"));
    assert_eq!(
        complete.pattern.as_deref(),
        Some("Invoices/{year}/{date}_Invoice")
    );
    assert_eq!(complete.missing, None);
    assert_eq!(without_date.pattern, complete.pattern);
    assert_eq!(without_date.missing, Some(Field::Date));
}

fn archive_files() -> (Vec<PlanFile>, Vec<TestFile>) {
    let catalog = KindCatalog::built_in();
    let aliases = SenderAliases::default();
    let names = [OWN_NAME.to_owned()];
    let own = OwnNames {
        names: &names,
        ibans: &[],
        tax_ids: &[],
    };
    let files = test_archive()
        .archive
        .iter()
        .zip(0..)
        .map(|(file, recorded_at)| {
            let found = file.text().map(|text| {
                detect_fields(
                    text,
                    &DocumentFields::default(),
                    &aliases,
                    &[],
                    &own,
                    &catalog,
                )
            });
            PlanFile {
                relative_path: file.relative_path.clone(),
                hash: ContentHash(Sha256::digest(file.bytes()).into()),
                fields: DocumentFields {
                    date: found
                        .as_ref()
                        .and_then(|detected| detected.date)
                        .or(file.date),
                    sender: found.as_ref().and_then(|detected| detected.sender.clone()),
                    kind: found.and_then(|detected| detected.kind),
                    ..DocumentFields::default()
                },
                text: file.text().map(str::to_owned),
                recorded_at,
            }
        })
        .collect();
    (files, test_archive().inbox)
}

fn archive_fields(letter: &str) -> DocumentFields {
    let found = detect_fields(
        letter,
        &DocumentFields::default(),
        &SenderAliases::default(),
        &[],
        &OwnNames {
            names: &[OWN_NAME.to_owned()],
            ibans: &[],
            tax_ids: &[],
        },
        &KindCatalog::built_in(),
    );
    DocumentFields {
        date: found.date,
        sender: found.sender,
        kind: found.kind,
        ..DocumentFields::default()
    }
}

fn inbox_letter_with(inbox: &[TestFile], needle: &str) -> String {
    inbox
        .iter()
        .filter_map(TestFile::text)
        .find(|text| text.contains(needle))
        .unwrap()
        .to_owned()
}

#[test]
fn on_the_test_archive_a_loan_letter_lands_in_the_folder_of_that_loan() {
    let (files, inbox) = archive_files();
    let structure = location_structure(&files, &KindCatalog::built_in());
    let letter = inbox_letter_with(&inbox, "Darlehensnummer 595610814");

    let placement = NeighbourIndex::new(&files, &no_own(), &structure)
        .place(&archive_fields(&letter), Some(&letter))
        .unwrap();

    assert_eq!(
        placement.folder,
        Path::new("Bank/Finanzierung/Chemnitz-Andrestr. 12")
    );
    assert_eq!(
        placement.signal,
        Signal::SameNumber {
            label: "Darlehensnummer".to_owned(),
            value: "595610814".to_owned()
        }
    );
    assert!(placement.is_sure());
}

#[test]
fn on_the_test_archive_a_bank_statement_is_placed_by_its_statements_with_their_pattern() {
    let (files, inbox) = archive_files();
    let structure = location_structure(&files, &KindCatalog::built_in());
    let letter = inbox_letter_with(&inbox, "Kundennummer: 4711002");
    let document = archive_fields(&letter);

    let placement = NeighbourIndex::new(&files, &no_own(), &structure)
        .place(&document, Some(&letter))
        .unwrap();

    assert_eq!(
        placement.folder,
        Path::new("Bank/Sparkasse Chemnitz/Kontoauszüge")
    );
    assert!(placement.is_sure(), "{placement:?}");
    assert!(placement.pattern.is_some(), "{placement:?}");
}

#[test]
fn on_the_test_archive_only_the_scans_folder_knows_the_city_letters() {
    let (files, _) = archive_files();
    let structure = location_structure(&files, &KindCatalog::built_in());
    let city = fields(Some("Stadt Chemnitz"), Some("Bescheid"));

    let placement = NeighbourIndex::new(&files, &no_own(), &structure).place(&city, None);

    assert_eq!(placement, None);
}
