use super::*;

const INVOICE_TEXT: &str = "Invoice 4711 Company A";

fn read_text(text: &str) -> IndexFixture {
    read_text_with_kinds(text, Arc::default())
}

fn read_text_with_kinds(text: &str, kinds: Arc<ncrs_index::KindCatalog>) -> IndexFixture {
    let inbox_dir = TempDir::new().unwrap();
    let inbox = location(&inbox_dir, LocationMode::Inbox);
    put(&inbox, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    fixture.index.lock().unwrap().set_kinds(kinds);
    let chain = TextChain::new().with_source(FixedText(Ok(Extracted::Text(text.to_owned()))));
    let runner = Runner::start(
        settings(workers(1)),
        fixture.steps_reading(chain),
        vec![inbox],
        stepping_clock(),
    );
    let report = runner.wait_for("the file handled", |snapshot| {
        is_idle(snapshot) && snapshot.steps[2].done == 1
    });
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    fixture
}

fn only_document_with(fixture: &IndexFixture, word: &str) -> ncrs_index::Document {
    let index = fixture.index.lock().unwrap();
    let id = index.search_text(word, 10).unwrap()[0];
    index.document(id).unwrap().unwrap()
}

#[test]
fn the_fields_are_stored_and_the_document_stays_new_without_a_review_or_a_plan() {
    let text = "Nordlicht Energie GmbH\nMusterweg 1\nInvoice 4711 Company A";
    let fixture = read_text(text);

    let document = only_document_with(&fixture, "4711");
    assert_eq!(
        document.fields.sender.as_deref(),
        Some("Nordlicht Energie GmbH")
    );
    assert_eq!(document.status, DocumentStatus::New);
    let index = fixture.index.lock().unwrap();
    assert!(index.open_reviews().unwrap().is_empty());
    assert!(index
        .documents_with_status(DocumentStatus::Planned)
        .unwrap()
        .is_empty());
}

#[test]
fn a_rule_that_would_match_does_not_file_the_document_here() {
    let inbox_dir = TempDir::new().unwrap();
    let inbox = location(&inbox_dir, LocationMode::Inbox);
    put(&inbox, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    fixture
        .index
        .lock()
        .unwrap()
        .create_rule(&ncrs_index::RuleSpec {
            name: None,
            conditions: ncrs_index::RuleConditions::all_of(vec![ncrs_index::Condition::new(
                ncrs_index::ConditionKind::Substring,
                "company a",
            )]),
            extract_number: Some(r"Invoice (\d+)".to_owned()),
            extract_date: None,
            extract_amount: None,
            kind: Some("invoice".to_owned()),
            tags: std::collections::BTreeSet::new(),
            filing_pattern: "{number}.pdf".to_owned(),
            confirmed: true,
            enabled: true,
        })
        .unwrap();
    let chain =
        TextChain::new().with_source(FixedText(Ok(Extracted::Text(INVOICE_TEXT.to_owned()))));
    let runner = Runner::start(
        settings(workers(1)),
        fixture.steps_reading(chain),
        vec![inbox],
        stepping_clock(),
    );
    runner.wait_for("the file handled", |snapshot| {
        is_idle(snapshot) && snapshot.steps[2].done == 1
    });

    let document = only_document_with(&fixture, "4711");
    assert_eq!(document.status, DocumentStatus::New);
    assert_eq!(document.fields.number, None);
}

#[test]
fn a_new_document_gets_the_sender_of_its_letterhead_and_offers_no_alias() {
    let text = "Nordlicht Energie GmbH\nMusterweg 1\nIBAN DE89 3704 0044 0532 0130 00\nInvoice 4711 Company A";
    let fixture = read_text(text);

    let document = only_document_with(&fixture, "4711");
    assert_eq!(
        document.fields.sender.as_deref(),
        Some("Nordlicht Energie GmbH")
    );
    let index = fixture.index.lock().unwrap();
    assert_eq!(
        index
            .field_origins(document.id)
            .unwrap()
            .of(ncrs_index::Field::Sender),
        ncrs_index::FieldOrigin::Detected
    );
    assert!(index.sender_aliases().unwrap().is_empty());
}

#[test]
fn a_field_a_person_set_is_not_replaced_when_the_file_is_processed_again() {
    let text = "Nordlicht Energie GmbH\nInvoice 4711 Company A";
    let inbox_dir = TempDir::new().unwrap();
    let inbox = location(&inbox_dir, LocationMode::Inbox);
    let path = put(&inbox, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let chain = TextChain::new().with_source(FixedText(Ok(Extracted::Text(text.to_owned()))));
    let runner = Runner::start(
        settings(workers(1)),
        fixture.steps_reading(chain),
        vec![inbox],
        stepping_clock(),
    );
    runner.wait_for("the file handled", |snapshot| {
        is_idle(snapshot) && snapshot.steps[2].done == 1
    });
    let id = {
        let mut index = fixture.index.lock().unwrap();
        let id = index.search_text("4711", 10).unwrap()[0];
        let mut fields = index.document(id).unwrap().unwrap().fields;
        fields.sender = Some("Own Choice".to_owned());
        fields.tags.insert("steuer".to_owned());
        index
            .set_document_fields(id, &fields, |_| ncrs_index::FieldOrigin::Manual)
            .unwrap();
        id
    };

    assert!(runner.process_again(&path));
    runner.wait_for("the file handled again", |snapshot| {
        is_idle(snapshot) && snapshot.steps[2].done == 2
    });

    let index = fixture.index.lock().unwrap();
    let document = index.document(id).unwrap().unwrap();
    assert_eq!(document.fields.sender.as_deref(), Some("Own Choice"));
    assert!(document.fields.tags.contains("steuer"));
}

#[test]
fn the_recipient_and_the_kind_are_read_from_the_letter_with_their_origins() {
    let text = "Frau Erika Beispiel\nMusterweg 3\n12345 Musterstadt\n\nRechnung 4711";
    let fixture = read_text(text);

    let document = only_document_with(&fixture, "4711");
    assert_eq!(document.fields.recipient.as_deref(), Some("Erika Beispiel"));
    assert_eq!(document.fields.kind.as_deref(), Some("invoice"));
    let origins = fixture
        .index
        .lock()
        .unwrap()
        .field_origins(document.id)
        .unwrap();
    assert_eq!(
        origins.of(ncrs_index::Field::Recipient),
        ncrs_index::FieldOrigin::AddressBlock
    );
    assert_eq!(
        origins.of(ncrs_index::Field::Kind),
        ncrs_index::FieldOrigin::Heading
    );
}

fn custom_kinds() -> Arc<ncrs_index::KindCatalog> {
    let (kinds, skips) = ncrs_index::KindCatalog::built_in().extended(&[
        ncrs_index::CustomKind {
            id: "payslip".to_owned(),
            german: Some("Lohnabrechnung".to_owned()),
            english: Some("Payslip".to_owned()),
            words: vec!["Gehaltsabrechnung".to_owned()],
        },
        ncrs_index::CustomKind {
            id: "invoice".to_owned(),
            words: vec!["Honorarnote".to_owned()],
            ..ncrs_index::CustomKind::default()
        },
    ]);
    assert_eq!(skips, Vec::new());
    Arc::new(kinds)
}

#[test]
fn a_custom_kind_is_read_from_the_heading() {
    let fixture = read_text_with_kinds("Gehaltsabrechnung 4711", custom_kinds());

    let document = only_document_with(&fixture, "4711");
    assert_eq!(document.fields.kind.as_deref(), Some("payslip"));
}

#[test]
fn an_extra_word_of_a_known_kind_is_a_heading() {
    let fixture = read_text_with_kinds("Honorarnote 4711", custom_kinds());

    let document = only_document_with(&fixture, "4711");
    assert_eq!(document.fields.kind.as_deref(), Some("invoice"));
}
