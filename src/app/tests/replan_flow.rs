use iced_test::futures::futures::StreamExt as _;
use iced_test::runtime::{task, Action};
use ncrs_index::ReviewReason;

use crate::app::*;
use crate::replan_work::fixtures::Setup;
use crate::replan_work::{ReplanAfter, ReplanItem};
use crate::review_list::ReplanStep;

/// Runs the tasks `message` starts and hands their answers back to `update`.
async fn settle(app: &mut App, message: Message) {
    let first = app.update(message);
    settle_task(app, first).await;
}

async fn settle_task(app: &mut App, first: Task<Message>) {
    let mut queue = vec![first];
    while let Some(next) = queue.pop() {
        let Some(mut stream) = task::into_stream(next) else {
            continue;
        };
        while let Some(action) = stream.next().await {
            if let Action::Output(reported) = action {
                queue.push(app.update(reported));
            }
        }
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    inbox: PathBuf,
    archive: PathBuf,
    document: PathBuf,
    app: App,
}

/// A document in review that the one rule would file, a review page that
/// is up and loaded, and a ready Hermes over the index.
fn fixture() -> Fixture {
    let mut setup = Setup::new();
    let document = setup.in_review("scan.pdf", "Invoice from Company A");
    setup.add_rule();
    let (dir, inbox, archive, index) = setup.into_parts();
    let locations = vec![
        config::FilingLocation {
            path: inbox.clone(),
            mode: LocationMode::Inbox,
            images: false,
            is_target: false,
        },
        config::FilingLocation {
            path: archive.clone(),
            mode: LocationMode::Archive,
            images: false,
            is_target: true,
        },
    ];
    let mut app = App::with_fixed_panels();
    app.hermes.filing = Some(FilingModule::ready_over_index_with_locations(
        index, &locations,
    ));
    app.hermes.filing_config = Filing {
        locations,
        ..Filing::default()
    };
    let mut list = ReviewList::new();
    list.loaded(Vec::new());
    app.hermes.review_list = Some(list);
    Fixture {
        _dir: dir,
        inbox,
        archive,
        document: document.path,
        app,
    }
}

fn step(app: &App) -> Option<&ReplanStep> {
    app.review_list_for_test().and_then(ReviewList::replan)
}

fn previewed_items(app: &App) -> Vec<ReplanItem> {
    match step(app) {
        Some(ReplanStep::Preview { items, .. }) => items.clone(),
        Some(ReplanStep::Looking(_)) | None => panic!("no preview"),
    }
}

fn review_open(app: &App) -> bool {
    let access = app
        .hermes
        .filing
        .as_ref()
        .and_then(FilingModule::review_access)
        .unwrap();
    !access.load().unwrap().is_empty()
}

#[tokio::test]
async fn replan_the_button_shows_a_preview_and_files_nothing() {
    let mut fixture = fixture();

    settle(
        &mut fixture.app,
        Message::ReviewList(ReviewListMessage::ReplanOpenCases),
    )
    .await;

    let items = previewed_items(&fixture.app);
    assert_eq!(items.len(), 1);
    assert!(matches!(items[0].after, ReplanAfter::Filed(_)));
    assert!(
        fixture.document.exists(),
        "nothing moves before the confirm"
    );
    assert!(!fixture.app.jobs.is_busy());
    assert!(fixture.app.hermes.pending_refile.is_none());
    assert!(review_open(&fixture.app));
}

#[tokio::test]
async fn replan_cancel_and_escape_leave_everything_as_it_was() {
    let mut fixture = fixture();
    settle(
        &mut fixture.app,
        Message::ReviewList(ReviewListMessage::ReplanOpenCases),
    )
    .await;

    settle(
        &mut fixture.app,
        Message::ReviewList(ReviewListMessage::ReplanCancel),
    )
    .await;
    assert!(step(&fixture.app).is_none());

    settle(
        &mut fixture.app,
        Message::ReviewList(ReviewListMessage::ReplanOpenCases),
    )
    .await;
    assert!(step(&fixture.app).is_some());
    settle(
        &mut fixture.app,
        Message::ReviewList(ReviewListMessage::Close),
    )
    .await;

    assert!(step(&fixture.app).is_none());
    assert!(
        fixture.app.review_list_for_test().is_some(),
        "the page stays"
    );
    assert!(fixture.document.exists());
    assert!(!fixture.app.jobs.is_busy());
    assert!(review_open(&fixture.app));
}

#[tokio::test]
async fn replan_confirm_files_the_previewed_documents_in_one_job() {
    let mut fixture = fixture();
    settle(
        &mut fixture.app,
        Message::ReviewList(ReviewListMessage::ReplanOpenCases),
    )
    .await;

    let job = fixture
        .app
        .update(Message::ReviewList(ReviewListMessage::ReplanConfirm));
    let mut stream = task::into_stream(job).unwrap();
    let mut notice = None;
    while let Some(action) = stream.next().await {
        if let Action::Output(reported) = action {
            drop(fixture.app.update(reported));
            notice = notice.or(fixture.app.notice_for_test().map(str::to_owned));
        }
    }

    assert!(!fixture.document.exists());
    assert!(fixture
        .archive
        .join("Insurance")
        .join("Invoice.pdf")
        .exists());
    assert!(!fixture.app.jobs.is_busy(), "the job is finished");
    assert!(step(&fixture.app).is_none());
    assert_eq!(
        notice.as_deref(),
        Some("1 filed, 0 stayed in review, 0 changed since the preview, 0 read again")
    );
    assert!(!review_open(&fixture.app));
}

#[tokio::test]
async fn replan_confirm_waits_for_a_running_job_and_queues_exactly_one() {
    let mut fixture = fixture();
    settle(
        &mut fixture.app,
        Message::ReviewList(ReviewListMessage::ReplanOpenCases),
    )
    .await;
    let previewed = previewed_items(&fixture.app);
    fixture.app.jobs.enqueue(jobs::Job {
        kind: jobs::JobKind::Copy,
        path: PathBuf::new(),
        total: None,
    });
    drop(fixture.app.jobs.start_next(|_| Task::<Message>::none()));

    drop(
        fixture
            .app
            .update(Message::ReviewList(ReviewListMessage::ReplanConfirm)),
    );

    assert_eq!(fixture.app.jobs.waiting(), 0, "it does not queue behind");
    assert_eq!(
        fixture.app.hermes.pending_refile,
        Some((PathBuf::new(), previewed))
    );
    assert!(fixture.document.exists());

    fixture.app.jobs.finish();
    drop(
        fixture
            .app
            .run_hermes_flow(Vec::new(), ncrs_hermes_ui::run_refile),
    );

    let running = fixture.app.jobs.running().unwrap();
    assert_eq!(running.kind, jobs::JobKind::File);
    assert_eq!(running.total, Some(1));
    assert_eq!(fixture.app.jobs.waiting(), 0);
    assert!(fixture.app.hermes.pending_refile.is_none());
}

#[tokio::test]
async fn replan_reprocess_folder_outside_the_inbox_only_says_so() {
    let mut fixture = fixture();
    fixture.app.hermes.review_list = None;

    drop(fixture.app.reprocess_folder(fixture.archive.clone()));

    assert_eq!(
        fixture.app.notice_for_test(),
        Some("Only folders in an inbox location can be reprocessed.")
    );
    assert!(fixture.app.review_list_for_test().is_none());
    assert!(!fixture.app.jobs.is_busy());
    assert!(fixture.document.exists());
}

#[tokio::test]
async fn replan_reprocess_folder_in_the_inbox_opens_the_preview() {
    let mut fixture = fixture();
    fixture.app.hermes.review_list = None;
    assert_eq!(
        crate::keymap::KeyAction::HermesReprocessFolder.message(),
        Message::HermesFile(HermesFileMessage::ReprocessFolder)
    );

    let task = fixture.app.reprocess_folder(fixture.inbox.clone());
    settle_task(&mut fixture.app, task).await;

    let items = previewed_items(&fixture.app);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].now, Some(ReviewReason::NoMatchingRule));
    assert!(fixture.document.exists());
    assert!(review_open(&fixture.app));
}
