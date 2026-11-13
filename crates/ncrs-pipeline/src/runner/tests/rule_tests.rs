use super::*;

pub fn invoice_rule(filing_pattern: &str, confirmed: bool) -> ncrs_index::RuleSpec {
    ncrs_index::RuleSpec {
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
        filing_pattern: filing_pattern.to_owned(),
        confirmed,
        enabled: true,
    }
}

const INVOICE_TEXT: &str = "Invoice 4711 Company A";

fn run_with_rule(spec: &ncrs_index::RuleSpec) -> IndexFixture {
    run_text_with_rule(spec, INVOICE_TEXT)
}

fn run_text_with_rule(spec: &ncrs_index::RuleSpec, text: &str) -> IndexFixture {
    run_text_with_rules(std::slice::from_ref(spec), text)
}

fn run_text_with_rules(specs: &[ncrs_index::RuleSpec], text: &str) -> IndexFixture {
    run_text_with_kinds(specs, text, Arc::default())
}

fn run_text_with_kinds(
    specs: &[ncrs_index::RuleSpec],
    text: &str,
    kinds: Arc<ncrs_index::KindCatalog>,
) -> IndexFixture {
    let inbox_dir = TempDir::new().unwrap();
    let inbox = location(&inbox_dir, LocationMode::Inbox);
    put(&inbox, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    fixture.index.lock().unwrap().set_kinds(kinds);
    for spec in specs {
        fixture.index.lock().unwrap().create_rule(spec).unwrap();
    }
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

#[test]
fn a_document_matching_a_confirmed_rule_gets_its_fields_and_no_review() {
    let fixture = run_with_rule(&invoice_rule("{number}.pdf", true));

    let index = fixture.index.lock().unwrap();
    assert!(index.open_reviews().unwrap().is_empty());
    let documents = index.search_text("4711", 10).unwrap();
    let document = index.document(documents[0]).unwrap().unwrap();
    assert_eq!(document.fields.number.as_deref(), Some("4711"));
    assert_eq!(document.fields.kind.as_deref(), Some("invoice"));
    assert_eq!(document.status, DocumentStatus::Planned);
}

#[test]
fn a_new_document_gets_the_sender_of_its_letterhead_and_offers_its_iban() {
    let text = "Nordlicht Energie GmbH\nMusterweg 1\nIBAN DE89 3704 0044 0532 0130 00\nInvoice 4711 Company A";
    let fixture = run_text_with_rule(&invoice_rule("{sender}/{number}.pdf", false), text);

    let index = fixture.index.lock().unwrap();
    let documents = index.search_text("4711", 10).unwrap();
    let document = index.document(documents[0]).unwrap().unwrap();
    assert_eq!(
        document.fields.sender.as_deref(),
        Some("Nordlicht Energie GmbH")
    );
    assert_eq!(
        index
            .field_origins(document.id)
            .unwrap()
            .of(ncrs_index::Field::Sender),
        ncrs_index::FieldOrigin::Rule(None)
    );
    let offers = index.sender_aliases().unwrap();
    assert_eq!(offers.len(), 1);
    assert_eq!(offers[0].sender, "Nordlicht Energie GmbH");
    assert_eq!(offers[0].kind, ncrs_index::SenderAliasKind::Iban);
    assert_eq!(offers[0].value, "DE89370400440532013000");
    assert_eq!(offers[0].state, ncrs_index::SenderAliasState::Offered);
}

#[test]
fn a_sender_that_is_already_set_is_not_replaced_by_the_detection() {
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
        index
            .set_document_fields(id, &fields, |_| ncrs_index::FieldOrigin::Manual)
            .unwrap();
        index.set_document_status(id, DocumentStatus::New).unwrap();
        id
    };

    assert!(runner.process_again(&path));
    runner.wait_for("the file handled again", |snapshot| {
        is_idle(snapshot) && snapshot.steps[2].done == 2
    });

    let index = fixture.index.lock().unwrap();
    let document = index.document(id).unwrap().unwrap();
    assert_eq!(document.fields.sender.as_deref(), Some("Own Choice"));
}

#[test]
fn the_recipient_and_the_kind_are_read_from_the_letter_with_their_origins() {
    let text = "Frau Erika Beispiel\nMusterweg 3\n12345 Musterstadt\n\nRechnung 4711";
    let fixture = run_text_with_rules(&[], text);

    let index = fixture.index.lock().unwrap();
    let id = index.search_text("4711", 10).unwrap()[0];
    let document = index.document(id).unwrap().unwrap();
    assert_eq!(document.fields.recipient.as_deref(), Some("Erika Beispiel"));
    assert_eq!(document.fields.kind.as_deref(), Some("invoice"));
    let origins = index.field_origins(id).unwrap();
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
fn a_custom_kind_is_read_from_the_heading_and_filed_by_a_rule_on_its_word() {
    let fixture = run_text_with_kinds(
        &[ncrs_index::RuleSpec {
            kind: None,
            ..tag_rule(&["lohnabrechnung"])
        }],
        "Gehaltsabrechnung 4711",
        custom_kinds(),
    );

    let index = fixture.index.lock().unwrap();
    let id = index.search_text("4711", 10).unwrap()[0];
    let document = index.document(id).unwrap().unwrap();
    assert_eq!(document.fields.kind.as_deref(), Some("payslip"));
    assert_eq!(document.status, DocumentStatus::Planned);
}

#[test]
fn an_extra_word_of_a_known_kind_is_a_heading_and_matches_its_rules() {
    let fixture = run_text_with_kinds(
        &[tag_rule(&["rechnung"])],
        "Honorarnote 4711",
        custom_kinds(),
    );

    let index = fixture.index.lock().unwrap();
    let id = index.search_text("4711", 10).unwrap()[0];
    let document = index.document(id).unwrap().unwrap();
    assert_eq!(document.fields.kind.as_deref(), Some("invoice"));
    assert_eq!(document.status, DocumentStatus::Planned);
}

#[test]
fn an_own_tag_survives_reading_and_planning_again() {
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
        fields.tags.insert("steuer".to_owned());
        index
            .set_document_fields(id, &fields, |_| ncrs_index::FieldOrigin::Manual)
            .unwrap();
        index.set_document_status(id, DocumentStatus::New).unwrap();
        id
    };

    assert!(runner.process_again(&path));
    runner.wait_for("the file handled again", |snapshot| {
        is_idle(snapshot) && snapshot.steps[2].done == 2
    });

    let index = fixture.index.lock().unwrap();
    let document = index.document(id).unwrap().unwrap();
    assert!(document.fields.tags.contains("steuer"));
    assert!(document
        .fields
        .all_tags(index.kinds())
        .contains("nordlicht-energie-gmbh"));
}

#[test]
fn a_document_the_rules_cannot_place_stays_new_until_it_is_reviewed() {
    let fixture = run_with_rule(&invoice_rule("{number}.pdf", false));

    let index = fixture.index.lock().unwrap();
    let documents = index.search_text("4711", 10).unwrap();
    let document = index.document(documents[0]).unwrap().unwrap();
    assert_eq!(document.status, DocumentStatus::InReview);
}

#[test]
fn a_reviewed_document_that_a_new_rule_places_leaves_the_review_list_when_processed_again() {
    let inbox_dir = TempDir::new().unwrap();
    let inbox = location(&inbox_dir, LocationMode::Inbox);
    let path = put(&inbox, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let reads = Arc::new(AtomicUsize::new(0));
    let runner = reviewed_once(&inbox, &fixture, &reads);
    assert_eq!(
        fixture.index.lock().unwrap().open_reviews().unwrap().len(),
        1
    );
    fixture
        .index
        .lock()
        .unwrap()
        .create_rule(&invoice_rule("{number}.pdf", true))
        .unwrap();

    assert!(runner.process_again(&path));

    runner.wait_for("the file passed again", |snapshot| {
        is_idle(snapshot) && snapshot.steps[2].done == 2
    });
    assert!(fixture
        .index
        .lock()
        .unwrap()
        .open_reviews()
        .unwrap()
        .is_empty());
    assert_eq!(
        first_document_status(&fixture, &inbox),
        DocumentStatus::Planned
    );
}

#[test]
fn a_document_with_a_missing_field_is_reviewed_with_that_field() {
    let fixture = run_with_rule(&invoice_rule("{year}/{number}.pdf", true));

    let index = fixture.index.lock().unwrap();
    let reviews = index.open_reviews().unwrap();
    assert_eq!(reviews.len(), 1);
    assert_eq!(
        reviews[0].reason,
        ncrs_index::ReviewReason::MissingField(ncrs_index::Field::Date)
    );
}

#[test]
fn an_unconfirmed_rule_sends_the_document_to_review_as_such() {
    let fixture = run_with_rule(&invoice_rule("{number}.pdf", false));

    let index = fixture.index.lock().unwrap();
    let reviews = index.open_reviews().unwrap();
    assert_eq!(reviews.len(), 1);
    assert_eq!(reviews[0].reason, ncrs_index::ReviewReason::UnconfirmedRule);
}

#[test]
fn two_confirmed_rules_with_different_results_file_by_the_first() {
    let fixture = run_text_with_rules(
        &[
            invoice_rule("{number}.pdf", true),
            invoice_rule("Invoices/{number}.pdf", true),
        ],
        INVOICE_TEXT,
    );

    let index = fixture.index.lock().unwrap();
    assert!(index.open_reviews().unwrap().is_empty());
    let documents = index.search_text("4711", 10).unwrap();
    let document = index.document(documents[0]).unwrap().unwrap();
    assert_eq!(document.status, DocumentStatus::Planned);
}

const LETTER_TEXT: &str = "Firma A GmbH\nMusterweg 3\n12345 Musterstadt\n\nRechnung 4711";

fn tag_rule(tags: &[&str]) -> ncrs_index::RuleSpec {
    ncrs_index::RuleSpec {
        conditions: ncrs_index::RuleConditions::all_of(
            tags.iter()
                .map(|tag| ncrs_index::Condition::new(ncrs_index::ConditionKind::Tag, *tag))
                .collect(),
        ),
        extract_number: None,
        ..invoice_rule("Tags/{kind}", true)
    }
}

#[test]
fn a_rule_on_the_tags_of_the_document_files_it() {
    let fixture = run_text_with_rules(&[tag_rule(&["rechnung", "firma-a"])], LETTER_TEXT);

    let index = fixture.index.lock().unwrap();
    assert!(
        index.open_reviews().unwrap().is_empty(),
        "{:?}",
        index.open_reviews().unwrap()
    );
    let documents = index.search_text("4711", 10).unwrap();
    let document = index.document(documents[0]).unwrap().unwrap();
    assert_eq!(document.status, DocumentStatus::Planned);
}

#[test]
fn a_rule_on_the_english_kind_files_a_document_whose_heading_is_german() {
    let fixture = run_text_with_rules(&[tag_rule(&["invoice", "firma-a"])], LETTER_TEXT);

    let index = fixture.index.lock().unwrap();
    assert!(index.open_reviews().unwrap().is_empty());
    let documents = index.search_text("4711", 10).unwrap();
    let document = index.document(documents[0]).unwrap().unwrap();
    assert_eq!(document.status, DocumentStatus::Planned);
    assert_eq!(document.fields.kind.as_deref(), Some("invoice"));
}

#[test]
fn a_rule_on_a_tag_the_document_lacks_leaves_it_to_review() {
    let fixture = run_text_with_rules(&[tag_rule(&["rechnung", "stadtwerke"])], LETTER_TEXT);

    let index = fixture.index.lock().unwrap();
    let reviews = index.open_reviews().unwrap();
    assert_eq!(reviews.len(), 1);
    assert_eq!(reviews[0].reason, ncrs_index::ReviewReason::NoMatchingRule);
}
