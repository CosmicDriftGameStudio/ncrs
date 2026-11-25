//! The model fills fields after everything else is read: it never holds up a
//! file that has no model call in front of it.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use ncrs_ai::{Extraction, ExtractionRequest, FakeProvider, Provider};
use ncrs_index::{DocumentId, Field, FieldOrigin};

use super::*;
use crate::ai_step::AI_READ_VERSION;

const LETTER: &str = "Nordlicht Energie GmbH\nMusterweg 1\nInvoice 4711 Company A";
/// Long enough for a loaded machine; only a hang ever reaches it.
const HANG_GUARD: Duration = Duration::from_secs(30);

fn extracted(field: Field, value: &str) -> ncrs_ai::ExtractedField {
    ncrs_ai::ExtractedField {
        field,
        value: value.to_owned(),
        confidence: 0.95,
    }
}

/// Blocks every call until the gate is dropped, and says when one started.
#[derive(Debug)]
struct GatedProvider {
    started: Mutex<Sender<()>>,
    gate: Mutex<Receiver<()>>,
    reply: Extraction,
    calls: AtomicUsize,
    ran_through: AtomicBool,
}

impl GatedProvider {
    fn new(reply: Extraction) -> (Arc<Self>, Receiver<()>, Sender<()>) {
        let (started, started_receiver) = mpsc::channel();
        let (open, gate) = mpsc::channel();
        let provider = Arc::new(Self {
            started: Mutex::new(started),
            gate: Mutex::new(gate),
            reply,
            calls: AtomicUsize::new(0),
            ran_through: AtomicBool::new(false),
        });
        (provider, started_receiver, open)
    }
}

impl Provider for GatedProvider {
    fn extract(&self, _request: &ExtractionRequest) -> Result<Extraction, ncrs_ai::AiError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let _ = self
            .started
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .send(());
        // Returns as soon as the sender is dropped.
        let _ = self
            .gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .recv();
        self.ran_through.store(true, Ordering::SeqCst);
        Ok(self.reply.clone())
    }
}

fn start_with_ai(
    fixture: &IndexFixture,
    inbox: &Location,
    provider: Arc<dyn Provider>,
    worker_count: usize,
) -> Runner {
    let chain = TextChain::new().with_source(FixedText(Ok(Extracted::Text(LETTER.to_owned()))));
    let steps = intake_steps(
        Arc::new(SharedIndex(Arc::clone(&fixture.index))),
        Arc::new(chain),
        Some(provider),
        OwnDetails::default(),
    );
    Runner::start(
        settings(workers(worker_count)),
        steps,
        vec![inbox.clone()],
        stepping_clock(),
    )
}

fn document_ids(fixture: &IndexFixture, inbox: &Location) -> Vec<DocumentId> {
    let index = fixture.index.lock().unwrap();
    index
        .documents_under(&inbox.path)
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect()
}

fn count(report: &Snapshot, outcome: ChangeOutcome) -> usize {
    report
        .changed_paths
        .iter()
        .filter(|change| change.outcome == outcome)
        .count()
}

#[test]
fn the_model_waits_for_the_fields_of_every_other_file() {
    let dir = TempDir::new().unwrap();
    let inbox = location(&dir, LocationMode::Inbox);
    put(&inbox, "a.pdf", b"first file");
    put(&inbox, "b.pdf", b"second file");
    let fixture = IndexFixture::new();
    let (provider, started, gate) = GatedProvider::new(Extraction {
        fields: vec![extracted(Field::Number, "4711")],
        not_a_document: None,
    });
    let runner = start_with_ai(&fixture, &inbox, provider, 1);

    started.recv_timeout(HANG_GUARD).unwrap();

    // The only worker is inside the model call for one file, so the other
    // file got its fields before that call began.
    let report = runner.snapshot();
    assert_eq!(report.steps[2].name, "fields");
    assert_eq!(report.steps[2].done, 2);
    assert_eq!(report.steps[3].name, "ai");
    assert_eq!(report.steps[3].done, 0);
    let ids = document_ids(&fixture, &inbox);
    assert_eq!(ids.len(), 2);
    {
        let index = fixture.index.lock().unwrap();
        for id in ids {
            let document = index.document(id).unwrap().unwrap();
            assert_eq!(
                document.fields.sender.as_deref(),
                Some("Nordlicht Energie GmbH")
            );
            assert_eq!(document.status, DocumentStatus::New);
        }
    }
    drop(gate);
    let finished = runner.wait_for("both files asked", |snapshot| {
        is_idle(snapshot) && snapshot.steps[3].done == 2
    });
    assert_eq!(count(&finished, ChangeOutcome::Finished), 2);
    assert_eq!(count(&finished, ChangeOutcome::FilledByAi), 2);
}

#[test]
fn a_new_file_is_not_held_up_by_a_running_model_call_when_a_worker_is_free() {
    let dir = TempDir::new().unwrap();
    let inbox = location(&dir, LocationMode::Inbox);
    let first = put(&inbox, "a.pdf", b"first file");
    let fixture = IndexFixture::new();
    let (provider, started, gate) = GatedProvider::new(Extraction::default());
    let runner = start_with_ai(&fixture, &inbox, provider, 2);
    started.recv_timeout(HANG_GUARD).unwrap();

    let second = put(&inbox, "b.pdf", b"second file");
    runner.shared.on_watch_event(WatchEvent::File(second));

    runner.wait_for("the new file read while the call runs", |snapshot| {
        snapshot.steps[2].done == 2
    });
    assert!(first.exists());
    drop(gate);
    runner.wait_for("everything asked", |snapshot| {
        is_idle(snapshot) && snapshot.steps[3].done + snapshot.steps[3].unchanged == 2
    });
}

fn found_nothing() -> Arc<FakeProvider> {
    Arc::new(FakeProvider::replying("{}"))
}

#[test]
fn a_document_is_asked_once_per_version_even_when_nothing_was_found_and_after_a_restart() {
    let dir = TempDir::new().unwrap();
    let inbox = location(&dir, LocationMode::Inbox);
    put(&inbox, "a.pdf", b"first file");
    let fixture = IndexFixture::new();
    let provider = found_nothing();

    let first = start_with_ai(&fixture, &inbox, Arc::clone(&provider) as _, 1)
        .wait_for("asked", |snapshot| is_idle(snapshot) && snapshot.steps[3].unchanged == 1);
    assert_eq!(count(&first, ChangeOutcome::FilledByAi), 0);
    assert_eq!(provider.requests().len(), 1);
    let id = document_ids(&fixture, &inbox)[0];
    assert_eq!(
        fixture.index.lock().unwrap().ai_read_version(id).unwrap(),
        AI_READ_VERSION
    );

    let second = start_with_ai(&fixture, &inbox, Arc::clone(&provider) as _, 1)
        .wait_for("scanned again", |snapshot| {
            is_idle(snapshot) && snapshot.steps[0].unchanged == 1
        });

    assert_eq!(second.steps[1].done, 0);
    assert_eq!(provider.requests().len(), 1, "a restart does not ask again");
}

#[test]
fn a_value_from_the_model_adds_exactly_one_change_and_none_adds_nothing() {
    let dir = TempDir::new().unwrap();
    let inbox = location(&dir, LocationMode::Inbox);
    put(&inbox, "a.pdf", b"first file");
    let fixture = IndexFixture::new();
    let provider = Arc::new(FakeProvider::replying(
        r#"{"number": {"value": "4711", "confidence": 0.95}}"#,
    ));

    let report = start_with_ai(&fixture, &inbox, provider, 1).wait_for("asked", |snapshot| {
        is_idle(snapshot) && snapshot.steps[3].done == 1
    });

    assert_eq!(count(&report, ChangeOutcome::Finished), 1);
    assert_eq!(count(&report, ChangeOutcome::FilledByAi), 1);
    let id = document_ids(&fixture, &inbox)[0];
    let index = fixture.index.lock().unwrap();
    let document = index.document(id).unwrap().unwrap();
    assert_eq!(document.fields.number.as_deref(), Some("4711"));
    assert_eq!(document.status, DocumentStatus::New);
    assert_eq!(
        index.field_origins(id).unwrap().of(Field::Number),
        FieldOrigin::Ai {
            confidence_percent: 95
        }
    );
}

#[test]
fn a_field_a_person_sets_while_the_model_runs_is_kept() {
    let dir = TempDir::new().unwrap();
    let inbox = location(&dir, LocationMode::Inbox);
    put(&inbox, "a.pdf", b"first file");
    let fixture = IndexFixture::new();
    let (provider, started, gate) = GatedProvider::new(Extraction {
        fields: vec![
            extracted(Field::Sender, "Zeta GmbH"),
            extracted(Field::Number, "4711"),
        ],
        not_a_document: None,
    });
    let runner = start_with_ai(&fixture, &inbox, provider, 1);
    started.recv_timeout(HANG_GUARD).unwrap();
    let id = document_ids(&fixture, &inbox)[0];
    {
        let mut index = fixture.index.lock().unwrap();
        let mut fields = index.document(id).unwrap().unwrap().fields;
        fields.sender = Some("Own Choice".to_owned());
        index
            .set_document_fields(id, &fields, |_| FieldOrigin::Manual)
            .unwrap();
    }
    drop(gate);

    runner.wait_for("asked", |snapshot| {
        is_idle(snapshot) && snapshot.steps[3].done == 1
    });

    let document = fixture
        .index
        .lock()
        .unwrap()
        .document(id)
        .unwrap()
        .unwrap();
    assert_eq!(document.fields.sender.as_deref(), Some("Own Choice"));
    assert_eq!(document.fields.number.as_deref(), Some("4711"));
}

#[test]
fn a_file_replaced_while_the_model_runs_gets_no_answer_meant_for_the_old_bytes() {
    let dir = TempDir::new().unwrap();
    let inbox = location(&dir, LocationMode::Inbox);
    let path = put(&inbox, "a.pdf", b"first file");
    let fixture = IndexFixture::new();
    let (provider, started, gate) = GatedProvider::new(Extraction {
        fields: vec![extracted(Field::Number, "4711")],
        not_a_document: None,
    });
    let runner = start_with_ai(&fixture, &inbox, provider, 1);
    started.recv_timeout(HANG_GUARD).unwrap();
    let id = document_ids(&fixture, &inbox)[0];
    {
        // The index now holds other bytes under the document, as a replaced
        // file would leave it.
        let mut index = fixture.index.lock().unwrap();
        let stamp = ncrs_index::FileStamp {
            size: 11,
            mtime: UNIX_EPOCH,
        };
        index
            .replace_document_content(id, &ncrs_index::ContentHash([7; 32]), stamp)
            .unwrap();
    }
    drop(gate);

    runner.wait_for("asked", |snapshot| {
        is_idle(snapshot) && snapshot.steps[3].unchanged == 1
    });

    assert!(path.exists());
    let document = fixture
        .index
        .lock()
        .unwrap()
        .document(id)
        .unwrap()
        .unwrap();
    assert_eq!(document.fields.number, None);
}

#[derive(Debug)]
struct Enriching;

impl Step for Enriching {
    fn name(&self) -> &'static str {
        "enriching"
    }

    fn run(&self, _item: &mut Item, _now: SystemTime) -> Result<StepOutcome, String> {
        Ok(StepOutcome::Unchanged)
    }

    fn waits_for_the_rest(&self) -> bool {
        true
    }
}

#[test]
fn an_item_queued_for_a_step_that_waits_for_the_rest_does_not_keep_its_text() {
    let (_dir, location) = numbered_files(1);
    let (step, _started) = probe("probe", None, continue_always);
    let runner = start(
        workers(1),
        vec![step, Box::new(Enriching)],
        &location,
        Arc::new(FixedClock),
    );
    runner.pause();
    let path = location.path.join("file-1.pdf");
    let mut item = Item::at_first_step(&location, &path, false);
    item.text = Some("the whole text".to_owned());

    let (late, continued) = runner.shared.update(|state| {
        state.queue_continuation(item.clone(), 1);
        state.queue_continuation(item.clone(), 0);
        (
            state.late.front().map(|queued| queued.item.text.clone()),
            state
                .continuations
                .front()
                .map(|queued| queued.item.text.clone()),
        )
    });

    assert_eq!(late, Some(None));
    assert_eq!(continued, Some(Some("the whole text".to_owned())));
}
