use super::*;

#[test]
fn the_backlog_is_taken_in_and_only_inbox_documents_are_reviewed() {
    let inbox_dir = TempDir::new().unwrap();
    let archive_dir = TempDir::new().unwrap();
    let inbox = location(&inbox_dir, LocationMode::Inbox);
    let archive = location(&archive_dir, LocationMode::Archive);
    put(&inbox, "invoice.pdf", INVOICE);
    put(&inbox, "reminder.pdf", REMINDER);
    put(&archive, "contract.pdf", CONTRACT);
    let fixture = IndexFixture::new();
    let runner = Runner::start(
        settings(workers(2)),
        fixture.steps(),
        vec![inbox.clone(), archive.clone()],
        stepping_clock(),
    );

    let report = runner.wait_for("the backlog taken in", |snapshot| {
        is_idle(snapshot) && snapshot.steps[3].done == 3
    });

    assert_eq!(report.steps[0].name, "intake");
    assert_eq!(report.steps[0].done, 3);
    assert_eq!(report.steps[1].name, "text");
    assert_eq!(report.steps[2].name, "rules");
    assert_eq!(report.steps[3].name, "review");
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let index = fixture.index.lock().unwrap();
    assert_eq!(index.documents_under(&inbox.path).unwrap().len(), 2);
    let archived = index.documents_under(&archive.path).unwrap();
    assert_eq!(archived.len(), 1);
    let reviews = index.open_reviews().unwrap();
    assert_eq!(reviews.len(), 2);
    assert!(reviews
        .iter()
        .all(|review| review.document_id != archived[0].0));
    let document = index.document(archived[0].0).unwrap().unwrap();
    assert_eq!(document.status, DocumentStatus::Filed);
}

#[test]
fn a_file_the_watcher_reports_reaches_the_review_list() {
    let inbox_dir = TempDir::new().unwrap();
    let inbox = location(&inbox_dir, LocationMode::Inbox);
    put(&inbox, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let runner = Runner::start(
        settings(workers(2)),
        fixture.steps(),
        vec![inbox.clone()],
        stepping_clock(),
    );
    runner.wait_for("the backlog reviewed", |snapshot| {
        is_idle(snapshot) && snapshot.steps[3].done == 1
    });

    let reminder = put(&inbox, "reminder.pdf", REMINDER);
    // The OS delivers events with no upper bound on the delay under load;
    // the watcher tests in ncrs-intake cover the backend itself.
    runner.shared.on_watch_event(WatchEvent::File(reminder));

    runner.wait_for("the new file reviewed", |snapshot| {
        snapshot.steps[3].done == 2
    });
    let index = fixture.index.lock().unwrap();
    assert_eq!(index.open_reviews().unwrap().len(), 2);
}

#[test]
fn a_watcher_error_rescans_the_location_for_missed_files() {
    let inbox_dir = TempDir::new().unwrap();
    let inbox = location(&inbox_dir, LocationMode::Inbox);
    put(&inbox, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let runner = Runner::start(
        settings(workers(2)),
        fixture.steps(),
        vec![inbox.clone()],
        stepping_clock(),
    );
    runner.wait_for("the backlog reviewed", |snapshot| {
        is_idle(snapshot) && snapshot.steps[3].done == 1
    });

    put(&inbox, "reminder.pdf", REMINDER);
    runner.shared.on_watch_event(WatchEvent::Error {
        location: inbox.path.clone(),
        message: "the system dropped file events".to_owned(),
    });

    runner.wait_for("the missed file reviewed", |snapshot| {
        snapshot.steps[3].done == 2
    });
    let index = fixture.index.lock().unwrap();
    assert_eq!(index.open_reviews().unwrap().len(), 2);
}

#[derive(Debug)]
pub struct FixedText(pub Result<Extracted, TextError>);

impl TextSource for FixedText {
    fn extract(&self, _path: &Path) -> Result<Extracted, TextError> {
        self.0.clone()
    }
}

#[test]
fn the_text_of_a_file_is_stored_in_the_index() {
    let inbox_dir = TempDir::new().unwrap();
    let inbox = location(&inbox_dir, LocationMode::Inbox);
    put(&inbox, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let chain = TextChain::new().with_source(FixedText(Ok(Extracted::Text(
        "Invoice 4711 Company A".to_owned(),
    ))));
    let runner = Runner::start(
        settings(workers(1)),
        fixture.steps_reading(chain),
        vec![inbox],
        stepping_clock(),
    );

    let report = runner.wait_for("the file reviewed", |snapshot| {
        is_idle(snapshot) && snapshot.steps[3].done == 1
    });

    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let index = fixture.index.lock().unwrap();
    assert_eq!(index.search_text("4711 Company", 10).unwrap().len(), 1);
}

#[derive(Debug)]
struct CountingText(Arc<std::sync::atomic::AtomicUsize>);

impl TextSource for CountingText {
    fn extract(&self, _path: &Path) -> Result<Extracted, TextError> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(Extracted::Text("Invoice 4711 Company A".to_owned()))
    }
}

#[test]
fn a_known_file_processed_again_passes_the_text_step_again() {
    let inbox_dir = TempDir::new().unwrap();
    let inbox = location(&inbox_dir, LocationMode::Inbox);
    put(&inbox, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let chain = TextChain::new().with_source(CountingText(Arc::clone(&reads)));
    let runner = Runner::start(
        settings(workers(1)),
        fixture.steps_reading(chain),
        vec![inbox.clone()],
        stepping_clock(),
    );
    runner.wait_for("the file reviewed", |snapshot| {
        is_idle(snapshot) && snapshot.steps[3].done == 1
    });
    assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 1);

    assert!(runner.process_again(&inbox.path.join("invoice.pdf")));

    runner.wait_for("the file reviewed again", |snapshot| {
        is_idle(snapshot) && snapshot.steps[3].done == 2
    });
    assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert!(!runner.process_again(Path::new("/elsewhere/invoice.pdf")));
    let index = fixture.index.lock().unwrap();
    assert_eq!(index.open_reviews().unwrap().len(), 1);
}

pub fn first_document_status(fixture: &IndexFixture, location: &Location) -> DocumentStatus {
    let index = fixture.index.lock().unwrap();
    let id = index.documents_under(&location.path).unwrap()[0].0;
    index.document(id).unwrap().unwrap().status
}

fn mark_first_document_filed(fixture: &IndexFixture, location: &Location) {
    let index = fixture.index.lock().unwrap();
    let id = index.documents_under(&location.path).unwrap()[0].0;
    index
        .set_document_status(id, DocumentStatus::Filed)
        .unwrap();
}

pub fn reviewed_once(
    location: &Location,
    fixture: &IndexFixture,
    reads: &Arc<AtomicUsize>,
) -> Runner {
    let chain = TextChain::new().with_source(CountingText(Arc::clone(reads)));
    let runner = Runner::start(
        settings(workers(1)),
        fixture.steps_reading(chain),
        vec![location.clone()],
        stepping_clock(),
    );
    runner.wait_for("the file taken in", |snapshot| {
        is_idle(snapshot) && snapshot.steps[3].done == 1
    });
    runner
}

#[test]
fn a_filed_document_in_an_inbox_location_stays_filed_when_processed_again() {
    let inbox_dir = TempDir::new().unwrap();
    let inbox = location(&inbox_dir, LocationMode::Inbox);
    let path = put(&inbox, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let reads = Arc::new(AtomicUsize::new(0));
    let runner = reviewed_once(&inbox, &fixture, &reads);
    mark_first_document_filed(&fixture, &inbox);

    assert!(runner.process_again(&path));

    let report = runner.wait_for("the file passed again", |snapshot| {
        is_idle(snapshot) && snapshot.steps[3].done == 2
    });
    assert_eq!(report.plans_made, 0);
    assert_eq!(
        first_document_status(&fixture, &inbox),
        DocumentStatus::Filed
    );
    assert!(path.exists());
}

#[test]
fn a_document_in_an_archive_location_is_never_planned_when_processed_again() {
    let archive_dir = TempDir::new().unwrap();
    let archive = location(&archive_dir, LocationMode::Archive);
    let path = put(&archive, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let reads = Arc::new(AtomicUsize::new(0));
    let runner = reviewed_once(&archive, &fixture, &reads);

    assert!(runner.process_again(&path));

    let report = runner.wait_for("the file passed again", |snapshot| {
        is_idle(snapshot) && snapshot.steps[3].done == 2
    });
    assert_eq!(report.plans_made, 0);
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    assert_eq!(
        first_document_status(&fixture, &archive),
        DocumentStatus::Filed
    );
    assert!(fixture
        .index
        .lock()
        .unwrap()
        .open_reviews()
        .unwrap()
        .is_empty());
    assert!(path.exists());
}

#[test]
fn an_excluded_file_is_skipped_even_when_processed_again() {
    let inbox_dir = TempDir::new().unwrap();
    let inbox = location(&inbox_dir, LocationMode::Inbox);
    let path = put(&inbox, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let reads = Arc::new(AtomicUsize::new(0));
    let runner = reviewed_once(&inbox, &fixture, &reads);
    fixture.index.lock().unwrap().exclude_path(&path).unwrap();

    assert!(runner.process_again(&path));

    let report = runner.wait_for("the file passed the first step", |snapshot| {
        is_idle(snapshot) && snapshot.steps[0].unchanged == 1
    });
    assert_eq!(report.steps[1].done, 1);
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[test]
fn a_file_included_again_is_found_by_a_scan_without_waiting_for_the_rescan() {
    let inbox_dir = TempDir::new().unwrap();
    let inbox = location(&inbox_dir, LocationMode::Inbox);
    let path = put(&inbox, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    fixture.index.lock().unwrap().exclude_path(&path).unwrap();
    let runner = Runner::start(
        settings(workers(1)),
        fixture.steps(),
        vec![inbox.clone()],
        stepping_clock(),
    );
    runner.wait_for("the excluded file skipped", |snapshot| {
        is_idle(snapshot) && snapshot.steps[0].unchanged == 1
    });
    fixture.index.lock().unwrap().include_path(&path).unwrap();

    assert!(runner.rescan_around(&path));
    assert!(!runner.rescan_around(Path::new("/elsewhere/invoice.pdf")));

    runner.wait_for("the file reviewed", |snapshot| {
        is_idle(snapshot) && snapshot.steps[3].done == 1
    });
    assert_eq!(
        fixture.index.lock().unwrap().open_reviews().unwrap().len(),
        1
    );
}

#[test]
fn a_file_without_text_still_reaches_the_review_list() {
    let inbox_dir = TempDir::new().unwrap();
    let inbox = location(&inbox_dir, LocationMode::Inbox);
    put(&inbox, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let chain = TextChain::new().with_source(FixedText(Ok(Extracted::NotApplicable)));
    let runner = Runner::start(
        settings(workers(1)),
        fixture.steps_reading(chain),
        vec![inbox],
        stepping_clock(),
    );

    runner.wait_for("the file reviewed", |snapshot| {
        is_idle(snapshot) && snapshot.steps[3].done == 1
    });

    assert_eq!(
        fixture.index.lock().unwrap().open_reviews().unwrap().len(),
        1
    );
}

#[test]
fn a_scan_without_the_ocr_models_goes_to_the_review_list_without_text() {
    let inbox_dir = TempDir::new().unwrap();
    let models_dir = TempDir::new().unwrap();
    let inbox = location(&inbox_dir, LocationMode::Inbox);
    put(&inbox, "scan.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let page = PageImage {
        width: 2,
        height: 2,
        pixels: vec![0, 255, 255, 0],
    };
    let chain = TextChain::new()
        .with_source(FixedText(Ok(Extracted::PageImages(vec![page]))))
        .with_image_source(LazyOcrText::new(models_dir.path().join("ocr-models")));
    let runner = Runner::start(
        settings(workers(1)),
        fixture.steps_reading(chain),
        vec![inbox],
        stepping_clock(),
    );

    let report = runner.wait_for("the scan reviewed", |snapshot| {
        is_idle(snapshot) && snapshot.steps[3].done == 1
    });

    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let index = fixture.index.lock().unwrap();
    let reviews = index.open_reviews().unwrap();
    assert_eq!(reviews.len(), 1);
    assert_eq!(reviews[0].reason, ncrs_index::ReviewReason::NoMatchingRule);
    assert!(index.search_text("4711", 10).unwrap().is_empty());
    assert!(
        !models_dir.path().join("ocr-models").exists(),
        "nothing is created or downloaded"
    );
}

#[test]
fn a_missing_pdf_library_holds_the_file_back_without_an_error_or_a_review() {
    let inbox_dir = TempDir::new().unwrap();
    let inbox = location(&inbox_dir, LocationMode::Inbox);
    put(&inbox, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let missing = TextError::LibraryMissing {
        path: PathBuf::from("pdfium"),
    };
    let chain = TextChain::new().with_source(FixedText(Err(missing)));
    let runner = Runner::start(
        settings(workers(1)),
        fixture.steps_reading(chain),
        vec![inbox],
        stepping_clock(),
    );

    let report = runner.wait_for("the file held back", |snapshot| {
        is_idle(snapshot) && snapshot.steps[1].unchanged == 1
    });

    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.steps[1].failed, 0);
    assert_eq!(report.steps[2].done, 0);
    assert_eq!(report.steps[3].done, 0);
    let index = fixture.index.lock().unwrap();
    assert!(index.open_reviews().unwrap().is_empty());
    let statuses = index.documents_with_status(DocumentStatus::New).unwrap();
    assert_eq!(statuses.len(), 1);
}

#[test]
fn an_unreadable_file_is_still_an_error_of_the_text_step() {
    let inbox_dir = TempDir::new().unwrap();
    let inbox = location(&inbox_dir, LocationMode::Inbox);
    put(&inbox, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let broken = TextError::Unreadable {
        path: PathBuf::from("invoice.pdf"),
        reason: "broken".to_owned(),
    };
    let chain = TextChain::new().with_source(FixedText(Err(broken.clone())));
    let runner = Runner::start(
        settings(workers(1)),
        fixture.steps_reading(chain),
        vec![inbox],
        stepping_clock(),
    );

    let report = runner.wait_for("the failure shown", |snapshot| {
        is_idle(snapshot) && snapshot.steps[1].failed == 1
    });

    assert_eq!(report.errors[0].message, broken.to_string());
    assert!(fixture
        .index
        .lock()
        .unwrap()
        .open_reviews()
        .unwrap()
        .is_empty());
}

#[derive(Debug)]
struct SwitchableText(Arc<Mutex<Result<Extracted, TextError>>>);

impl TextSource for SwitchableText {
    fn extract(&self, _path: &Path) -> Result<Extracted, TextError> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

pub fn archive_scan(fixture: &IndexFixture, archive: &Location, chain: TextChain) -> Snapshot {
    let runner = Runner::start(
        settings(workers(1)),
        fixture.steps_reading(chain),
        vec![archive.clone()],
        stepping_clock(),
    );
    runner.wait_for("the archive scanned", is_idle)
}

#[test]
fn a_broken_archive_file_is_reported_once_and_not_read_again_on_every_scan() {
    let archive_dir = TempDir::new().unwrap();
    let archive = location(&archive_dir, LocationMode::Archive);
    put(&archive, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let broken = TextError::Unreadable {
        path: PathBuf::from("invoice.pdf"),
        reason: "broken".to_owned(),
    };
    let chain = || TextChain::new().with_source(FixedText(Err(broken.clone())));

    let first = archive_scan(&fixture, &archive, chain());
    assert_eq!(first.errors.len(), 1, "{:?}", first.errors);
    archive_scan(&fixture, &archive, chain());
    let third = archive_scan(&fixture, &archive, chain());

    assert!(third.errors.is_empty(), "{:?}", third.errors);
}

#[test]
fn an_archive_document_read_while_the_models_were_broken_gets_its_text_once_they_work() {
    let archive_dir = TempDir::new().unwrap();
    let archive = location(&archive_dir, LocationMode::Archive);
    put(&archive, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let source = Arc::new(Mutex::new(Err(TextError::EngineUnavailable {
        path: PathBuf::from("models"),
        reason: "broken".to_owned(),
    })));
    let chain = || TextChain::new().with_source(SwitchableText(Arc::clone(&source)));

    let first = archive_scan(&fixture, &archive, chain());
    assert_eq!(first.errors.len(), 1, "{:?}", first.errors);

    *source.lock().unwrap() = Ok(Extracted::Text("Invoice 4711 Company A".to_owned()));
    archive_scan(&fixture, &archive, chain());

    let index = fixture.index.lock().unwrap();
    assert_eq!(index.search_text("4711", 10).unwrap().len(), 1);
}

#[test]
fn an_archive_document_whose_text_could_not_be_read_gets_it_on_a_later_scan() {
    let archive_dir = TempDir::new().unwrap();
    let archive = location(&archive_dir, LocationMode::Archive);
    put(&archive, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let source = Arc::new(Mutex::new(Err(TextError::LibraryMissing {
        path: PathBuf::from("pdfium"),
    })));
    let chain = || TextChain::new().with_source(SwitchableText(Arc::clone(&source)));

    let first = archive_scan(&fixture, &archive, chain());
    assert!(first.errors.is_empty(), "{:?}", first.errors);
    assert!(fixture
        .index
        .lock()
        .unwrap()
        .search_text("4711", 10)
        .unwrap()
        .is_empty());

    *source.lock().unwrap() = Ok(Extracted::Text("Invoice 4711 Company A".to_owned()));
    archive_scan(&fixture, &archive, chain());

    let index = fixture.index.lock().unwrap();
    assert_eq!(index.search_text("4711", 10).unwrap().len(), 1);
}

#[test]
fn an_archive_document_without_text_is_read_once_and_not_again() {
    let archive_dir = TempDir::new().unwrap();
    let archive = location(&archive_dir, LocationMode::Archive);
    put(&archive, "scan.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let source = Arc::new(Mutex::new(Ok(Extracted::NotApplicable)));
    let chain = || TextChain::new().with_source(SwitchableText(Arc::clone(&source)));

    archive_scan(&fixture, &archive, chain());
    let id = fixture
        .index
        .lock()
        .unwrap()
        .documents_under(&archive.path)
        .unwrap()[0]
        .0;
    assert_eq!(
        fixture.index.lock().unwrap().document_text(id).unwrap(),
        Some(String::new())
    );

    *source.lock().unwrap() = Ok(Extracted::Text("Invoice 4711".to_owned()));
    let second = archive_scan(&fixture, &archive, chain());

    assert_eq!(second.steps[1].done, 0);
    let index = fixture.index.lock().unwrap();
    assert_eq!(index.document_text(id).unwrap(), Some(String::new()));
}

#[test]
fn an_unasked_archive_document_waits_while_the_provider_cannot_answer_and_is_asked_when_it_can() {
    let archive_dir = TempDir::new().unwrap();
    let archive = location(&archive_dir, LocationMode::Archive);
    put(&archive, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let source = Arc::new(Mutex::new(Ok(Extracted::Text(
        "Invoice 4711 Company A".to_owned(),
    ))));
    let chain = || TextChain::new().with_source(SwitchableText(Arc::clone(&source)));
    archive_scan(&fixture, &archive, chain());

    let provider = Arc::new(ncrs_ai::FakeProvider::replying(
        r#"{"sender": {"value": "Company A", "confidence": 0.9}}"#,
    ));
    let scan_with_ai = || {
        let steps = intake_steps(
            Arc::new(SharedIndex(Arc::clone(&fixture.index))),
            Arc::new(chain()),
            Some(Arc::clone(&provider) as Arc<dyn ncrs_ai::Provider>),
            OwnDetails::default(),
        );
        Runner::start(
            settings(workers(1)),
            steps,
            vec![archive.clone()],
            stepping_clock(),
        )
        .wait_for("the archive scanned", is_idle)
    };

    provider.set_answers_now(false);
    let waiting = scan_with_ai();
    assert_eq!(waiting.steps[1].done, 0);
    assert!(provider.requests().is_empty());
    assert!(fixture
        .index
        .lock()
        .unwrap()
        .open_reviews()
        .unwrap()
        .is_empty());

    provider.set_answers_now(true);
    scan_with_ai();
    assert_eq!(provider.requests().len(), 1);
}

#[test]
fn an_archive_document_asked_by_an_older_ai_reading_is_asked_again_and_a_current_one_is_not() {
    let archive_dir = TempDir::new().unwrap();
    let archive = location(&archive_dir, LocationMode::Archive);
    put(&archive, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let source = Arc::new(Mutex::new(Ok(Extracted::Text(
        "Invoice 4711 Company A".to_owned(),
    ))));
    let chain = || TextChain::new().with_source(SwitchableText(Arc::clone(&source)));
    archive_scan(&fixture, &archive, chain());
    let provider = Arc::new(ncrs_ai::FakeProvider::replying(
        r#"{"sender": {"value": "Company A", "confidence": 0.9}}"#,
    ));
    let scan_with_ai = || {
        let steps = intake_steps(
            Arc::new(SharedIndex(Arc::clone(&fixture.index))),
            Arc::new(chain()),
            Some(Arc::clone(&provider) as Arc<dyn ncrs_ai::Provider>),
            OwnDetails::default(),
        );
        Runner::start(
            settings(workers(1)),
            steps,
            vec![archive.clone()],
            stepping_clock(),
        )
        .wait_for("the archive scanned", is_idle)
    };
    let id = fixture
        .index
        .lock()
        .unwrap()
        .documents_under(&archive.path)
        .unwrap()[0]
        .0;

    scan_with_ai();
    assert_eq!(provider.requests().len(), 1);
    scan_with_ai();
    assert_eq!(
        provider.requests().len(),
        1,
        "a current reading is not asked"
    );

    fixture
        .index
        .lock()
        .unwrap()
        .set_ai_read_version(id, 1)
        .unwrap();
    scan_with_ai();
    assert_eq!(provider.requests().len(), 2);
}

#[test]
fn an_archive_document_with_an_io_error_stores_no_text_and_gets_it_on_a_later_scan() {
    let archive_dir = TempDir::new().unwrap();
    let archive = location(&archive_dir, LocationMode::Archive);
    put(&archive, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let source = Arc::new(Mutex::new(Err(TextError::Io {
        path: PathBuf::from("invoice.pdf"),
        reason: "the share is not mounted".to_owned(),
    })));
    let chain = || TextChain::new().with_source(SwitchableText(Arc::clone(&source)));

    let first = archive_scan(&fixture, &archive, chain());
    assert_eq!(first.errors.len(), 1, "{:?}", first.errors);

    *source.lock().unwrap() = Ok(Extracted::Text("Invoice 4711 Company A".to_owned()));
    archive_scan(&fixture, &archive, chain());

    let index = fixture.index.lock().unwrap();
    assert_eq!(index.search_text("4711", 10).unwrap().len(), 1);
}

#[cfg(unix)]
#[test]
fn an_unreadable_archive_text_file_stores_no_text_and_is_read_on_a_later_scan() {
    use std::os::unix::fs::PermissionsExt as _;

    let archive_dir = TempDir::new().unwrap();
    let archive = location(&archive_dir, LocationMode::Archive);
    let note = put(&archive, "note.txt", b"Honorar 4711 Company A");
    let fixture = IndexFixture::new();
    let chain = || TextChain::new().with_source(ncrs_intake::PlainText);

    fs::set_permissions(&note, fs::Permissions::from_mode(0o000)).unwrap();
    archive_scan(&fixture, &archive, chain());
    fs::set_permissions(&note, fs::Permissions::from_mode(0o644)).unwrap();
    archive_scan(&fixture, &archive, chain());

    let index = fixture.index.lock().unwrap();
    assert_eq!(index.search_text("4711", 10).unwrap().len(), 1);
}
