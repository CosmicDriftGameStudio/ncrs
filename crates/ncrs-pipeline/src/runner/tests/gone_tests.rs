//! Files that are deleted or moved out while the app runs.

use ncrs_index::{DocumentId, FileStamp};

use super::*;

fn gone_settings() -> Settings {
    Settings {
        gone_settle_after: Duration::ZERO,
        ..settings(workers(2))
    }
}

fn started_archive(files: &[(&str, &[u8])]) -> (TempDir, Location, IndexFixture, Runner) {
    let dir = TempDir::new().unwrap();
    let archive = location(&dir, LocationMode::Archive);
    for (name, bytes) in files {
        let path = archive.path.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let fixture = IndexFixture::new();
    let runner = Runner::start(
        gone_settings(),
        fixture.steps(),
        vec![archive.clone()],
        stepping_clock(),
    );
    runner.wait_for("the archive taken in", |snapshot| {
        is_idle(snapshot) && snapshot.steps[0].done + snapshot.steps[0].unchanged == files.len()
    });
    (dir, archive, fixture, runner)
}

fn document_at(fixture: &IndexFixture, path: &Path) -> DocumentId {
    let index = fixture.index.lock().unwrap();
    index.document_by_path(path).unwrap().unwrap().id
}

fn is_missing(fixture: &IndexFixture, id: DocumentId) -> bool {
    let index = fixture.index.lock().unwrap();
    index.document(id).unwrap().unwrap().missing_since.is_some()
}

#[test]
fn a_deleted_file_is_marked_missing_without_a_scan() {
    let (_dir, archive, fixture, runner) =
        started_archive(&[("a.pdf", INVOICE), ("b.pdf", REMINDER)]);
    let deleted = archive.path.join("a.pdf");
    let id = document_at(&fixture, &deleted);
    let kept = document_at(&fixture, &archive.path.join("b.pdf"));

    fs::remove_file(&deleted).unwrap();
    runner.shared.on_watch_event(WatchEvent::Gone(deleted));
    runner.wait_for("the deleted file settled", is_idle);

    assert!(is_missing(&fixture, id));
    assert!(!is_missing(&fixture, kept));
}

#[test]
fn a_removed_folder_marks_every_document_below_it_missing() {
    let (_dir, archive, fixture, runner) = started_archive(&[
        ("keep.pdf", CONTRACT),
        ("old/a.pdf", INVOICE),
        ("old/deep/b.pdf", REMINDER),
    ]);
    let below = [
        document_at(&fixture, &archive.path.join("old/a.pdf")),
        document_at(&fixture, &archive.path.join("old/deep/b.pdf")),
    ];
    let kept = document_at(&fixture, &archive.path.join("keep.pdf"));

    fs::remove_dir_all(archive.path.join("old")).unwrap();
    runner
        .shared
        .on_watch_event(WatchEvent::Gone(archive.path.join("old")));
    runner.wait_for("the folder settled", is_idle);

    assert!(below.iter().all(|id| is_missing(&fixture, *id)));
    assert!(!is_missing(&fixture, kept));
}

#[test]
fn a_deleted_copy_is_forgotten_and_the_document_stays() {
    let (_dir, archive, fixture, runner) =
        started_archive(&[("a.pdf", INVOICE), ("b.pdf", INVOICE)]);
    let (id, original) = {
        let index = fixture.index.lock().unwrap();
        index.documents_under(&archive.path).unwrap().remove(0)
    };
    let copy = ["a.pdf", "b.pdf"]
        .map(|name| archive.path.join(name))
        .into_iter()
        .find(|path| *path != original)
        .unwrap();

    fs::remove_file(&copy).unwrap();
    runner.shared.on_watch_event(WatchEvent::Gone(copy));
    runner.wait_for("the copy settled", is_idle);

    let index = fixture.index.lock().unwrap();
    assert!(index
        .duplicate_paths_under(&archive.path)
        .unwrap()
        .is_empty());
    assert!(index.document(id).unwrap().unwrap().missing_since.is_none());
}

#[test]
fn a_deleted_document_is_replaced_by_its_copy() {
    let (_dir, archive, fixture, runner) =
        started_archive(&[("a.pdf", INVOICE), ("b.pdf", INVOICE)]);
    let (id, original) = {
        let index = fixture.index.lock().unwrap();
        index.documents_under(&archive.path).unwrap().remove(0)
    };
    let copy = ["a.pdf", "b.pdf"]
        .map(|name| archive.path.join(name))
        .into_iter()
        .find(|path| *path != original)
        .unwrap();

    fs::remove_file(&original).unwrap();
    runner.shared.on_watch_event(WatchEvent::Gone(original));
    runner.wait_for("the document settled", is_idle);

    let index = fixture.index.lock().unwrap();
    let document = index.document(id).unwrap().unwrap();
    assert!(document.missing_since.is_none());
    assert_eq!(document.path, copy);
}

#[test]
fn a_file_moved_by_the_app_is_not_marked_missing() {
    let (_dir, archive, fixture, runner) = started_archive(&[("a.pdf", INVOICE)]);
    let old = archive.path.join("a.pdf");
    let new = archive.path.join("filed.pdf");
    let id = document_at(&fixture, &old);

    fs::rename(&old, &new).unwrap();
    let metadata = fs::metadata(&new).unwrap();
    fixture
        .index
        .lock()
        .unwrap()
        .set_document_path(
            id,
            &new,
            FileStamp {
                size: metadata.len(),
                mtime: metadata.modified().unwrap(),
            },
        )
        .unwrap();
    runner.shared.on_watch_event(WatchEvent::Gone(old));
    runner.wait_for("the old path settled", is_idle);

    assert!(!is_missing(&fixture, id));
    let index = fixture.index.lock().unwrap();
    assert_eq!(index.document(id).unwrap().unwrap().path, new);
}

#[test]
fn a_path_that_comes_back_before_it_is_settled_stays_a_document() {
    let dir = TempDir::new().unwrap();
    let archive = location(&dir, LocationMode::Archive);
    put(&archive, "a.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let runner = Runner::start(
        Settings {
            gone_settle_after: Duration::from_secs(3600),
            ..settings(workers(2))
        },
        fixture.steps(),
        vec![archive.clone()],
        stepping_clock(),
    );
    runner.wait_for("taken in", |snapshot| {
        is_idle(snapshot) && snapshot.steps[0].done == 1
    });
    let path = archive.path.join("a.pdf");

    runner.shared.on_watch_event(WatchEvent::Gone(path.clone()));
    runner.shared.on_watch_event(WatchEvent::File(path));

    assert!(runner.shared.update(|state| state.gone.is_empty()));
}

#[test]
fn a_body_text_taken_in_before_its_mail_arrived_is_dropped_when_the_mail_does() {
    let (_dir, archive, fixture, runner) = started_archive(&[("mail/body.txt", b"Dear Anna")]);
    let body = archive.path.join("mail/body.txt");
    let id = document_at(&fixture, &body);

    put(
        &archive,
        "mail/mail.eml",
        b"From: a@example.org\r\n\r\nDear Anna",
    );
    runner.shared.on_watch_event(WatchEvent::Gone(body.clone()));
    runner.wait_for("the body text settled", is_idle);

    let index = fixture.index.lock().unwrap();
    assert!(index.document(id).unwrap().is_none());
    assert!(body.exists());
}
