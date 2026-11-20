use super::app_simulator as simulator;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use ncrs_pipeline::{ErrorEntry, RunnerState, Snapshot, StepSnapshot, Task};

use super::*;
use crate::messages::FilingStatusMessage;

fn taken_at() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
}

fn snapshot(state: RunnerState, waiting: usize, left: usize) -> Snapshot {
    let mut snapshot = Snapshot::off();
    snapshot.state = state;
    snapshot.worker_target = 2;
    snapshot.left_pipeline = left;
    snapshot.steps = vec![StepSnapshot {
        name: "probe",
        waiting,
        done: 0,
        failed: 0,
        unchanged: 0,
    }];
    snapshot
}

fn page(snapshot: Snapshot) -> App {
    App::with_filing_status_page(snapshot, taken_at())
}

fn shows(app: &App, label: &str) -> bool {
    simulator(app).find(label).is_ok()
}

fn two_errors() -> Vec<ErrorEntry> {
    [
        ("scan-0042.pdf", "no text could be read"),
        ("company-a-invoice.pdf", "the step stopped unexpectedly"),
    ]
    .into_iter()
    .enumerate()
    .map(|(sequence, (name, message))| ErrorEntry {
        file: PathBuf::from(format!("/tmp/inbox/{name}")),
        task: Task::Step("probe"),
        message: message.to_owned(),
        at: taken_at() - Duration::from_secs(60 * (2 - sequence as u64)),
        sequence: sequence as u64,
    })
    .collect()
}

fn messages_after_click(app: &App, label: &str) -> Vec<Message> {
    let mut sim = simulator(app);
    sim.click(label).expect("the clicked label");
    sim.into_messages()
        .filter(is_action)
        .map(without_focus)
        .collect()
}

fn page_message(message: FilingStatusMessage) -> Message {
    Message::FilingStatus(message)
}

#[test]
fn a_missing_pdf_library_is_a_box_with_the_searched_path_cut_in_the_middle() {
    let long = PathBuf::from(format!(
        "/Applications/Some Application/Contents/Frameworks/{}/lib/libpdfium.dylib",
        "deeply-nested-folder/".repeat(6)
    ));
    for (language, name) in [
        (Language::English, "inspection_pdf_missing_en"),
        (Language::German, "inspection_pdf_missing_de"),
    ] {
        let app = page(snapshot(RunnerState::Idle, 0, 0))
            .in_language(language)
            .with_pdf_library_missing(long.clone());
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_reference_image(format!("tests/snapshots/{name}.png"))
            .expect("snapshot comparison");
        assert!(
            matches,
            "the page does not match tests/snapshots/{name}.png"
        );
    }
}

#[test]
fn the_read_errors_match_their_snapshots() {
    for (language, name, errors) in [
        (Language::English, "read_errors_en", two_errors()),
        (Language::German, "read_errors_de", two_errors()),
        (Language::English, "read_errors_none_en", Vec::new()),
        (Language::German, "read_errors_none_de", Vec::new()),
    ] {
        let mut with_errors = snapshot(RunnerState::Idle, 0, 0);
        with_errors.errors = errors;
        let app = page(with_errors).in_language(language);
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_reference_image(format!("tests/snapshots/{name}.png"))
            .expect("snapshot comparison");
        assert!(
            matches,
            "the page does not match tests/snapshots/{name}.png"
        );
    }
}

#[test]
fn an_error_is_a_row_with_its_reason_and_two_buttons_in_the_row() {
    let mut with_error = snapshot(RunnerState::Idle, 0, 1);
    with_error.errors = two_errors();
    let app = page(with_error);

    assert!(shows(&app, "the step stopped unexpectedly"));
    assert!(!shows(&app, "No errors"));
    assert_eq!(
        messages_after_click(&app, "Show file"),
        [page_message(FilingStatusMessage::ErrorActivated(
            PathBuf::from("/tmp/inbox/company-a-invoice.pdf")
        ))]
    );
    assert_eq!(
        messages_after_click(&app, "Process again"),
        [page_message(FilingStatusMessage::ProcessOneAgain(
            PathBuf::from("/tmp/inbox/company-a-invoice.pdf")
        ))]
    );
}

#[test]
fn without_errors_the_page_is_not_missing_but_says_so_in_a_row() {
    let app = page(snapshot(RunnerState::Idle, 0, 0));

    assert!(shows(&app, "No errors"));
    assert!(!shows(&app, "Show file"));
}

#[test]
fn the_status_bar_shows_progress_while_there_is_work_and_opens_the_page() {
    let app = App::with_filing_progress(
        snapshot(RunnerState::Running, 340, 100),
        snapshot(RunnerState::Running, 328, 112),
        taken_at(),
    );

    assert!(shows(&app, "running \u{b7} 328 open"));
    assert_eq!(
        messages_after_click(&app, "running \u{b7} 328 open"),
        [page_message(FilingStatusMessage::Open)]
    );
}

#[test]
fn the_status_bar_says_when_the_pipeline_is_paused() {
    let app = App::with_filing_snapshot(snapshot(RunnerState::Paused, 5, 0), taken_at());

    assert!(shows(&app, "paused \u{b7} 5 open"));
}

#[test]
fn the_status_bar_names_reviews_and_failures() {
    let mut failing = snapshot(RunnerState::Running, 0, 0);
    failing.steps[0].failed = 2;
    let counts = ncrs_index::HermesCounts {
        open_reviews: 2421,
        ..ncrs_index::HermesCounts::default()
    };
    let app = App::with_filing_snapshot(failing, taken_at()).with_hermes_counts(counts);
    assert!(shows(
        &app,
        "idle \u{b7} 2\u{a0}421 to review \u{b7} 2 failed"
    ));
    let german = app.in_language(Language::German);
    assert!(shows(
        &german,
        "ruht \u{b7} 2\u{a0}421 R\u{fc}ckfragen \u{b7} 2 Fehler"
    ));
}

#[test]
fn a_missing_pdf_library_is_the_whole_status_line() {
    let app = App::with_filing_snapshot(snapshot(RunnerState::Running, 5, 0), taken_at())
        .with_pdf_library_missing(PathBuf::from("/opt/pdfium/lib"));
    assert!(shows(&app, "PDF library missing, PDFs are not read"));
    assert_eq!(
        messages_after_click(&app, "PDF library missing, PDFs are not read"),
        [page_message(FilingStatusMessage::Open)]
    );
}

#[test]
fn the_status_bar_says_idle_when_nothing_waits_and_nothing_when_off() {
    let idle = App::with_filing_snapshot(snapshot(RunnerState::Idle, 0, 0), taken_at());
    assert!(shows(&idle, "idle"));
    let off = App::with_filing_snapshot(snapshot(RunnerState::Off, 0, 40), taken_at());
    assert!(!shows(&off, "idle"));
    assert!(!shows(&off, "running \u{b7} 40 open"));
}
