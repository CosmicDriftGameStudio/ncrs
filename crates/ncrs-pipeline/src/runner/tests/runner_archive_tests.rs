//! Reading the fields of a whole archive: letters get their fields and their
//! date, photos are read without text, nothing is planned or reviewed.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::NaiveDate;
use ncrs_index::{DocumentFields, DocumentId, Field, FieldOrigin};
use ncrs_intake::test_archive::{test_archive, TestArchive, TestFile};
use ncrs_intake::FIELDS_READ_VERSION;

use super::*;

/// The content of the file as text when it is UTF-8, as the PDF source would
/// give the text layer of a letter.
#[derive(Debug)]
struct FileAsText(Arc<AtomicUsize>);

impl FileAsText {
    fn uncounted() -> Self {
        Self(Arc::default())
    }
}

impl TextSource for FileAsText {
    fn extract(&self, path: &Path) -> Result<Extracted, TextError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let bytes = fs::read(path).map_err(|err| TextError::Unreadable {
            path: path.to_path_buf(),
            reason: err.to_string(),
        })?;
        Ok(String::from_utf8(bytes).map_or(Extracted::NotApplicable, Extracted::Text))
    }
}

/// The first files of every area of the test archive, without the inbox:
/// letters, photos and copies, few enough for a scan within the runner's hang
/// guard on a slow machine (a CI runner on Windows takes about half a second
/// per file). The whole archive is checked without the runner in ncrs-intake.
fn sampled_archive() -> TestArchive {
    const FILES_PER_AREA: usize = 2;
    let mut per_area: BTreeMap<PathBuf, usize> = BTreeMap::new();
    let archive = test_archive()
        .archive
        .into_iter()
        .filter(|file| {
            let area = file
                .relative_path
                .components()
                .next()
                .map(|component| PathBuf::from(component.as_os_str()))
                .unwrap_or_default();
            let taken = per_area.entry(area).or_default();
            *taken += 1;
            *taken <= FILES_PER_AREA
        })
        .collect();
    TestArchive {
        archive,
        inbox: Vec::new(),
    }
}

fn scan_with_files(
    fixture: &IndexFixture,
    archive: &Location,
    calls: &Arc<AtomicUsize>,
) -> Snapshot {
    let chain = TextChain::new().with_source(FileAsText(Arc::clone(calls)));
    archive_scan(fixture, archive, chain)
}

fn first_letter() -> TestFile {
    test_archive()
        .archive
        .into_iter()
        .find(|file| file.text().is_some() && file.date.is_some())
        .unwrap()
}

fn archive_with_one_letter(root: &TempDir) -> (Location, TestFile) {
    let letter = first_letter();
    let location = location(root, LocationMode::Archive);
    fs::write(location.path.join("letter.pdf"), letter.bytes()).unwrap();
    (location, letter)
}

fn only_document(fixture: &IndexFixture, location: &Location) -> DocumentId {
    let index = fixture.index.lock().unwrap();
    let documents = index.documents_under(&location.path).unwrap();
    assert_eq!(documents.len(), 1);
    documents[0].0
}

/// Sets the fields by hand and the document back to "read by an older version".
fn read_again_with(fixture: &IndexFixture, id: DocumentId, fields: &DocumentFields) {
    let mut index = fixture.index.lock().unwrap();
    index
        .set_document_fields(id, fields, |_| FieldOrigin::Manual)
        .unwrap();
    index.set_fields_read_version(id, 0).unwrap();
}

fn fields_of(fixture: &IndexFixture, location: &Location) -> Vec<(PathBuf, DocumentFields, u32)> {
    let index = fixture.index.lock().unwrap();
    index
        .documents_under(&location.path)
        .unwrap()
        .into_iter()
        .map(|(id, path)| {
            let document = index.document(id).unwrap().unwrap();
            (
                path,
                document.fields,
                index.fields_read_version(id).unwrap(),
            )
        })
        .collect()
}

#[test]
fn a_whole_archive_gets_fields_and_dates_without_plans_or_reviews() {
    let root = TempDir::new().unwrap();
    let archive = sampled_archive();
    archive.write_to(root.path()).unwrap();
    let location =
        Location::new(&root.path().join("archive"), LocationMode::Archive, true).unwrap();
    let fixture = IndexFixture::new();
    let calls = Arc::new(AtomicUsize::new(0));

    let first = scan_with_files(&fixture, &location, &calls);

    assert!(first.errors.is_empty(), "{:?}", first.errors);
    assert_eq!(first.plans_made, 0);
    let index = fixture.index.lock().unwrap();
    assert!(index.open_reviews().unwrap().is_empty());
    let documents = index.documents_under(&location.path).unwrap();
    assert!(
        documents.len() * 2 > archive.archive.len(),
        "{}",
        documents.len()
    );
    let mut letters = 0;
    let mut photos = 0;
    for (id, path) in documents {
        let document = index.document(id).unwrap().unwrap();
        assert_eq!(document.status, DocumentStatus::Filed, "{}", path.display());
        let file = archive
            .archive
            .iter()
            .find(|file| path == location.path.join(&file.relative_path))
            .unwrap();
        if file.text().is_some() {
            letters += 1;
            assert_eq!(document.fields.date, file.date, "{}", path.display());
        } else {
            photos += 1;
            assert_eq!(index.document_text(id).unwrap(), Some(String::new()));
            assert_eq!(document.fields, DocumentFields::default());
        }
        assert_eq!(index.fields_read_version(id).unwrap(), FIELDS_READ_VERSION);
    }
    assert!(
        letters > 0 && photos > 0,
        "{letters} letters, {photos} photos"
    );
    let tally = index.tag_tally().unwrap();
    assert!(!tally.kind.is_empty() && !tally.sender.is_empty() && !tally.year.is_empty());
    assert!(!tally.recipient.is_empty());
    assert!(
        tally.own.is_empty(),
        "reading an archive sets no tag of the user"
    );
}

#[test]
fn a_second_scan_reads_no_text_and_changes_no_field() {
    let root = TempDir::new().unwrap();
    sampled_archive().write_to(root.path()).unwrap();
    let location =
        Location::new(&root.path().join("archive"), LocationMode::Archive, true).unwrap();
    let fixture = IndexFixture::new();
    let calls = Arc::new(AtomicUsize::new(0));
    scan_with_files(&fixture, &location, &calls);
    let before = fields_of(&fixture, &location);
    let read_before = calls.load(Ordering::SeqCst);

    let second = scan_with_files(&fixture, &location, &calls);

    assert!(second.errors.is_empty(), "{:?}", second.errors);
    assert_eq!(second.steps[1].done, 0);
    assert_eq!(calls.load(Ordering::SeqCst), read_before);
    assert_eq!(fields_of(&fixture, &location), before);
}

#[test]
fn a_document_read_by_an_older_detection_is_detected_again_from_the_stored_text() {
    let root = TempDir::new().unwrap();
    let (location, letter) = archive_with_one_letter(&root);
    let fixture = IndexFixture::new();
    let calls = Arc::new(AtomicUsize::new(0));
    scan_with_files(&fixture, &location, &calls);
    let id = only_document(&fixture, &location);
    let first_fields = fields_of(&fixture, &location)[0].1.clone();
    assert_eq!(first_fields.date, letter.date);
    read_again_with(&fixture, id, &DocumentFields::default());
    let read_before = calls.load(Ordering::SeqCst);

    let second = scan_with_files(&fixture, &location, &calls);

    assert!(second.errors.is_empty(), "{:?}", second.errors);
    assert_eq!(calls.load(Ordering::SeqCst), read_before);
    assert_eq!(second.steps[2].done, 1);
    let index = fixture.index.lock().unwrap();
    let document = index.document(id).unwrap().unwrap();
    assert_eq!(document.fields, first_fields);
    assert_eq!(index.fields_read_version(id).unwrap(), FIELDS_READ_VERSION);
}

#[test]
fn a_sender_set_by_hand_stays_and_the_empty_date_is_filled() {
    let root = TempDir::new().unwrap();
    let (location, letter) = archive_with_one_letter(&root);
    let fixture = IndexFixture::new();
    let calls = Arc::new(AtomicUsize::new(0));
    scan_with_files(&fixture, &location, &calls);
    let id = only_document(&fixture, &location);
    let by_hand = DocumentFields {
        sender: Some("Von Hand".to_owned()),
        ..DocumentFields::default()
    };
    read_again_with(&fixture, id, &by_hand);

    scan_with_files(&fixture, &location, &calls);

    let index = fixture.index.lock().unwrap();
    let document = index.document(id).unwrap().unwrap();
    assert_eq!(document.fields.sender.as_deref(), Some("Von Hand"));
    assert_eq!(document.fields.date, letter.date);
    assert_eq!(
        index.field_origins(id).unwrap().of(Field::Sender),
        FieldOrigin::Manual
    );
    assert_eq!(document.status, DocumentStatus::Filed);
}

#[test]
fn a_guessed_date_is_read_again_and_a_date_set_by_hand_stays() {
    for (origin, replaced) in [
        (FieldOrigin::Rule(None), true),
        (FieldOrigin::Manual, false),
    ] {
        let root = TempDir::new().unwrap();
        let (location, letter) = archive_with_one_letter(&root);
        let fixture = IndexFixture::new();
        let calls = Arc::new(AtomicUsize::new(0));
        scan_with_files(&fixture, &location, &calls);
        let id = only_document(&fixture, &location);
        let birth_date = NaiveDate::from_ymd_opt(1978, 1, 2);
        let mut index = fixture.index.lock().unwrap();
        let mut fields = index.document(id).unwrap().unwrap().fields;
        fields.date = birth_date;
        index
            .set_document_fields(id, &fields, |_| origin.clone())
            .unwrap();
        index.set_fields_read_version(id, 1).unwrap();
        drop(index);

        scan_with_files(&fixture, &location, &calls);

        let reread_index = fixture.index.lock().unwrap();
        let document = reread_index.document(id).unwrap().unwrap();
        let expected = if replaced { letter.date } else { birth_date };
        assert_eq!(document.fields.date, expected, "{origin:?}");
        assert_eq!(
            reread_index.fields_read_version(id).unwrap(),
            FIELDS_READ_VERSION
        );
    }
}

#[test]
fn an_inbox_letter_gets_its_letter_date_and_still_goes_to_review() {
    let root = TempDir::new().unwrap();
    let letter = first_letter();
    let inbox = location(&root, LocationMode::Inbox);
    put(&inbox, "letter.pdf", letter.bytes());
    let fixture = IndexFixture::new();
    let chain = TextChain::new().with_source(FileAsText::uncounted());
    let runner = Runner::start(
        settings(workers(1)),
        fixture.steps_reading(chain),
        vec![inbox.clone()],
        stepping_clock(),
    );
    let report = runner.wait_for("the letter handled", |snapshot| {
        is_idle(snapshot) && snapshot.steps[2].done == 1
    });

    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.plans_made, 0);
    let id = only_document(&fixture, &inbox);
    let index = fixture.index.lock().unwrap();
    let document = index.document(id).unwrap().unwrap();
    assert_eq!(document.fields.date, letter.date);
    assert_eq!(
        index.field_origins(id).unwrap().of(Field::Date),
        FieldOrigin::Rule(None)
    );
    assert_eq!(document.status, DocumentStatus::InReview);
    assert_eq!(index.open_reviews().unwrap().len(), 1);
}
