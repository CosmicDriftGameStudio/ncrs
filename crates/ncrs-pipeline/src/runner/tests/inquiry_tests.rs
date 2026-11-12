use ncrs_ai::{FakeProvider, Provider};
use ncrs_index::{DocumentId, ReviewReason};
use ncrs_intake::ImageTextSource;

use super::rule_tests::invoice_rule;
use super::*;
use crate::ai_step::AI_READ_VERSION;

const INVOICE_TEXT: &str = "Invoice 4711 Company A";
const SENDER_REPLY: &str = r#"{"sender": {"value": "Company A", "confidence": 0.95}}"#;

fn text_chain(text: &str) -> TextChain {
    TextChain::new().with_source(FixedText(Ok(Extracted::Text(text.to_owned()))))
}

fn scan(
    fixture: &IndexFixture,
    location: &Location,
    chain: TextChain,
    provider: Option<&Arc<FakeProvider>>,
) -> Snapshot {
    let steps = intake_steps(
        Arc::new(SharedIndex(Arc::clone(&fixture.index))),
        Arc::new(chain),
        provider.map(|fake| Arc::clone(fake) as Arc<dyn Provider>),
        OwnDetails::default(),
    );
    Runner::start(
        settings(workers(1)),
        steps,
        vec![location.clone()],
        stepping_clock(),
    )
    .wait_for("the location scanned", is_idle)
}

fn only_document(fixture: &IndexFixture, location: &Location) -> DocumentId {
    fixture
        .index
        .lock()
        .unwrap()
        .documents_under(&location.path)
        .unwrap()[0]
        .0
}

fn status_of(fixture: &IndexFixture, id: DocumentId) -> DocumentStatus {
    fixture
        .index
        .lock()
        .unwrap()
        .document(id)
        .unwrap()
        .unwrap()
        .status
}

fn open_inquiries(fixture: &IndexFixture) -> u64 {
    fixture
        .index
        .lock()
        .unwrap()
        .hermes_counts()
        .unwrap()
        .open_reviews
}

fn picture_location(dir: &TempDir) -> Location {
    Location::new(dir.path(), LocationMode::Inbox, true).unwrap()
}

fn page() -> PageImage {
    PageImage {
        width: 2,
        height: 2,
        pixels: vec![0, 255, 255, 0],
    }
}

#[derive(Debug)]
struct NothingFound;

impl ImageTextSource for NothingFound {
    fn recognise(&self, _images: &[PageImage]) -> Result<Option<String>, TextError> {
        Ok(None)
    }
}

fn picture_chain(image_source: impl ImageTextSource + 'static) -> TextChain {
    TextChain::new()
        .with_source(FixedText(Ok(Extracted::PageImages(vec![page()]))))
        .with_image_source(image_source)
}

/// An inbox document an old version took in: in review, with no text row.
fn old_inquiry(fixture: &IndexFixture, inbox: &Location) -> DocumentId {
    let missing = TextError::LibraryMissing {
        path: PathBuf::from("pdfium"),
    };
    scan(
        fixture,
        inbox,
        TextChain::new().with_source(FixedText(Err(missing))),
        None,
    );
    let id = only_document(fixture, inbox);
    let mut index = fixture.index.lock().unwrap();
    assert!(!index.has_document_text_row(id).unwrap());
    index.add_review(id, ReviewReason::NoMatchingRule).unwrap();
    id
}

#[test]
fn an_open_inquiry_that_was_never_read_is_read_once_and_never_planned() {
    let inbox_dir = TempDir::new().unwrap();
    let inbox = location(&inbox_dir, LocationMode::Inbox);
    put(&inbox, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let id = old_inquiry(&fixture, &inbox);
    fixture
        .index
        .lock()
        .unwrap()
        .create_rule(&invoice_rule("{sender}/{number}.pdf", true))
        .unwrap();
    let provider = Arc::new(FakeProvider::replying(SENDER_REPLY));

    let second = scan(&fixture, &inbox, text_chain(INVOICE_TEXT), Some(&provider));

    assert!(second.errors.is_empty(), "{:?}", second.errors);
    assert_eq!(provider.requests().len(), 1);
    let index = fixture.index.lock().unwrap();
    assert!(index.has_document_text_row(id).unwrap());
    assert_eq!(index.ai_read_version(id).unwrap(), AI_READ_VERSION);
    assert_eq!(index.open_reviews().unwrap().len(), 1);
    let document = index.document(id).unwrap().unwrap();
    assert_eq!(document.fields.sender.as_deref(), Some("Company A"));
    drop(index);
    assert_eq!(status_of(&fixture, id), DocumentStatus::InReview);

    let third = scan(&fixture, &inbox, text_chain(INVOICE_TEXT), Some(&provider));

    assert_eq!(third.steps[1].done, 0, "not read again");
    assert_eq!(provider.requests().len(), 1, "not asked again");
    assert_eq!(status_of(&fixture, id), DocumentStatus::InReview);
}

#[test]
fn a_picture_whose_text_recognition_found_nothing_is_excluded() {
    let inbox_dir = TempDir::new().unwrap();
    let inbox = picture_location(&inbox_dir);
    let photo = put(&inbox, "photo.png", b"not really a png");
    let fixture = IndexFixture::new();

    scan(&fixture, &inbox, picture_chain(NothingFound), None);

    assert!(fixture.index.lock().unwrap().is_excluded(&photo).unwrap());
    assert_eq!(open_inquiries(&fixture), 0);
}

#[test]
fn a_picture_is_not_excluded_while_the_recognition_engine_is_unavailable() {
    let inbox_dir = TempDir::new().unwrap();
    let models_dir = TempDir::new().unwrap();
    let inbox = picture_location(&inbox_dir);
    let photo = put(&inbox, "photo.png", b"not really a png");
    let fixture = IndexFixture::new();
    let engine = LazyOcrText::new(models_dir.path().join("ocr-models"));

    scan(&fixture, &inbox, picture_chain(engine), None);

    assert!(!fixture.index.lock().unwrap().is_excluded(&photo).unwrap());
    assert_eq!(open_inquiries(&fixture), 1);
}

#[test]
fn a_file_the_provider_is_sure_is_no_document_is_excluded_and_an_unsure_one_stays_a_question() {
    for (confidence, excluded) in [("0.95", true), ("0.6", false)] {
        let inbox_dir = TempDir::new().unwrap();
        let inbox = location(&inbox_dir, LocationMode::Inbox);
        let file = put(&inbox, "main.pdf", INVOICE);
        let fixture = IndexFixture::new();
        let provider = Arc::new(FakeProvider::replying(&format!(
            r#"{{"not_a_document": {{"value": true, "confidence": {confidence}}}}}"#
        )));

        scan(
            &fixture,
            &inbox,
            text_chain("fn main() {}"),
            Some(&provider),
        );

        assert_eq!(
            fixture.index.lock().unwrap().is_excluded(&file).unwrap(),
            excluded,
            "{confidence}"
        );
        assert_eq!(
            open_inquiries(&fixture),
            u64::from(!excluded),
            "{confidence}"
        );
        assert!(provider.requests()[0].judge_document);
    }
}

#[test]
fn a_not_a_document_verdict_never_plans_a_document_and_never_moves_it() {
    let inbox_dir = TempDir::new().unwrap();
    let inbox = location(&inbox_dir, LocationMode::Inbox);
    put(&inbox, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let id = old_inquiry(&fixture, &inbox);
    fixture
        .index
        .lock()
        .unwrap()
        .create_rule(&invoice_rule("{sender}/{number}.pdf", true))
        .unwrap();
    let provider = Arc::new(FakeProvider::replying(
        r#"{"not_a_document": {"value": true, "confidence": 0.95},
            "sender": {"value": "Company A", "confidence": 0.95}}"#,
    ));

    scan(&fixture, &inbox, text_chain(INVOICE_TEXT), Some(&provider));

    assert_eq!(status_of(&fixture, id), DocumentStatus::InReview);
    assert_eq!(
        fixture
            .index
            .lock()
            .unwrap()
            .document(id)
            .unwrap()
            .unwrap()
            .fields
            .sender,
        None
    );
}

#[test]
fn a_planned_document_is_not_asked_whether_it_is_a_document() {
    let inbox_dir = TempDir::new().unwrap();
    let inbox = location(&inbox_dir, LocationMode::Inbox);
    put(&inbox, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    fixture
        .index
        .lock()
        .unwrap()
        .create_rule(&invoice_rule("{number}.pdf", true))
        .unwrap();
    let provider = Arc::new(FakeProvider::replying(SENDER_REPLY));

    scan(&fixture, &inbox, text_chain(INVOICE_TEXT), Some(&provider));

    let id = only_document(&fixture, &inbox);
    assert_eq!(status_of(&fixture, id), DocumentStatus::Planned);
    assert!(provider.requests().is_empty());
}

#[test]
fn an_archive_document_is_not_asked_whether_it_is_a_document() {
    let archive_dir = TempDir::new().unwrap();
    let archive = location(&archive_dir, LocationMode::Archive);
    put(&archive, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let provider = Arc::new(FakeProvider::replying(SENDER_REPLY));

    scan(
        &fixture,
        &archive,
        text_chain(INVOICE_TEXT),
        Some(&provider),
    );

    let asked = provider.requests();
    assert_eq!(asked.len(), 1);
    assert!(!asked[0].judge_document);
}
