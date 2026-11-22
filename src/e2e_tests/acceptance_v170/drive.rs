//! What the flows share: acting on the reviews page through its messages, and
//! looking at what an answer left on disk, in the index and in the config.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iced::advanced::widget::operation::Outcome;
use iced::advanced::widget::{Id, Operation};
use iced::{Rectangle, Vector};
use ncrs_filing::review_work::{AcceptSummary, ReviewItem};
use ncrs_hermes_ui::reviews_page::{Answer, Question, ReviewsPage, TakenBy};
use ncrs_hermes_ui::HermesPage;
use ncrs_index::DocumentId;

use super::probe::{wait_until, World};
use crate::e2e_tests::acceptance_v160::{shoot_sized, Live, SHOT_SIZE};
use crate::e2e_tests::Session;
use crate::messages::{ActivityMessage, FilingStatusMessage, Message, ReviewsMessage, ShownTab};

pub const SHOT_FOLDER: &str = "target/acceptance/v170";
const PAGE_LIMIT: Duration = Duration::from_secs(60);

/// A picture of the whole window at the size of the other acceptance pictures.
pub fn shot<P: Live>(session: &Session<P>, name: &str) {
    shoot_sized(&session.app, (SHOT_FOLDER, name), SHOT_SIZE, |_| {});
}

pub fn page<P: Live>(session: &Session<P>) -> &ReviewsPage {
    session
        .app
        .reviews_page_for_test()
        .expect("the reviews page is open")
}

/// Waits until the list is read and the files in the taken places are looked at.
pub fn page_ready<P: Live>(session: &mut Session<P>) {
    let ready = wait_until(session, PAGE_LIMIT, |live| {
        live.app.reviews_page_for_test().is_some_and(|page| {
            !page.is_loading()
                && page
                    .members(Question::TargetTaken)
                    .iter()
                    .all(|item| page.taken_by(item.document_id) != TakenBy::Unknown)
        })
    });
    assert!(ready, "the reviews page did not settle");
}

/// The way a person gets there: the status chip, the activity page, its link.
pub fn show_reviews<P: Live>(session: &mut Session<P>) {
    if session.app.open_hermes_page() == Some(HermesPage::Reviews)
        && session.app.shown_tab_for_test() == ShownTab::Settings
    {
        return;
    }
    session.send(Message::FilingStatus(FilingStatusMessage::Open));
    session.send(Message::Activity(ActivityMessage::OpenReviews));
    assert_eq!(session.app.open_hermes_page(), Some(HermesPage::Reviews));
    page_ready(session);
}

/// Reads the list again, as it does after every answer.
pub fn reload<P: Live>(session: &mut Session<P>) {
    session.send(Message::Reviews(ReviewsMessage::Filed(
        AcceptSummary::default(),
    )));
    page_ready(session);
}

pub fn find<P: Live>(session: &Session<P>, path: &Path) -> Option<(Question, ReviewItem)> {
    let page = page(session);
    page.questions().find_map(|question| {
        page.members(question)
            .into_iter()
            .find(|item| item.path == path)
            .map(|item| (question, item.clone()))
    })
}

pub fn item<P: Live>(session: &Session<P>, path: &Path) -> (Question, ReviewItem) {
    find(session, path).unwrap_or_else(|| panic!("no open review for {path:?}"))
}

pub fn document<P: Live>(session: &Session<P>, path: &Path) -> DocumentId {
    item(session, path).1.document_id
}

pub fn answer_file<P: Live>(session: &mut Session<P>, path: &Path, answer: Answer) {
    let (question, item) = item(session, path);
    let offered = page(session).file_answers(question, item.document_id);
    assert!(
        offered.contains(&answer),
        "{answer:?} is not offered for {path:?} ({question:?}): {offered:?}"
    );
    session.send(Message::Reviews(ReviewsMessage::AnswerFile(
        question,
        answer,
        item.document_id,
    )));
    if answer != Answer::SetFields {
        page_ready(session);
    }
}

/// Presses the group button; the count on it is what it must handle.
pub fn answer_group<P: Live>(
    session: &mut Session<P>,
    question: Question,
    answer: Answer,
) -> usize {
    let offered = page(session).group_answers(question);
    let count = offered
        .iter()
        .find(|(known, _)| *known == answer)
        .map(|(_, count)| *count)
        .unwrap_or_else(|| panic!("{answer:?} is not offered for {question:?}: {offered:?}"));
    session.send(Message::Reviews(ReviewsMessage::Answer(question, answer)));
    page_ready(session);
    count
}

/// The inbox files the page asks about because the archive holds their
/// content, with the archive file each repeats.
pub fn already_filed<P: Live>(session: &Session<P>) -> Vec<(PathBuf, PathBuf)> {
    page(session)
        .already_filed()
        .iter()
        .map(|found| (found.path.clone(), found.keeper_path.clone()))
        .collect()
}

pub fn answer_repeat<P: Live>(session: &mut Session<P>, path: &Path, answer: Answer) {
    let asked = already_filed(session);
    assert!(
        asked.iter().any(|(asked_path, _)| asked_path == path),
        "{path:?} is no question of the group: {asked:?}"
    );
    session.send(Message::Reviews(ReviewsMessage::AnswerRepeat(
        answer,
        path.to_path_buf(),
    )));
    page_ready(session);
}

pub fn type_into<P: Live>(session: &mut Session<P>, path: &Path, text: &str) {
    let document = document(session, path);
    session.send(Message::Reviews(ReviewsMessage::InputChanged(
        document,
        text.to_owned(),
    )));
}

pub fn unfold<P: Live>(session: &mut Session<P>, question: Question) {
    if !page(session).is_unfolded(question) {
        session.send(Message::Reviews(ReviewsMessage::ToggleGroup(question)));
    }
}

pub fn text_of(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|err| panic!("{path:?}: {err}"))
}

pub fn has_open_review(world: &World, path: &Path) -> bool {
    let index = world.index();
    index
        .document_by_path(path)
        .unwrap()
        .is_some_and(|document| index.open_review_of(document.id).unwrap().is_some())
}

pub fn open_reviews(world: &World) -> usize {
    world.index().open_reviews().unwrap().len()
}

pub fn files_below(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut open = vec![folder.to_path_buf()];
    while let Some(next) = open.pop() {
        for entry in std::fs::read_dir(&next).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                open.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found
}

/// The files the trash holds.
pub fn trashed(world: &World) -> Vec<PathBuf> {
    files_below(&world.root.join("trash"))
}

pub fn trash_holds(world: &World, text: &str) -> bool {
    trashed(world)
        .iter()
        .any(|path| std::fs::read_to_string(path).is_ok_and(|held| held == text))
}

/// Whether a journal item moved `from` somewhere.
pub fn journal_moved(world: &World, from: &Path) -> Option<PathBuf> {
    let index = world.index();
    index
        .recent_batches(500)
        .unwrap()
        .into_iter()
        .flat_map(|(_, items)| items)
        .find(|item| item.from == from)
        .map(|item| item.to)
}

/// The names of the runs in the journal, newest first.
pub fn batch_names(world: &World) -> Vec<Option<String>> {
    world
        .index()
        .recent_batches(500)
        .unwrap()
        .into_iter()
        .map(|(info, _)| info.name)
        .collect()
}

/// An answer changes no setting: the inbox is still read continuously and the
/// two folders are still there.
pub fn config_intact(world: &World) {
    let saved = std::fs::read_to_string(world.root.join("config.toml")).unwrap();
    assert!(saved.contains("continuous = true"), "config: {saved}");
    assert!(saved.contains("mode = \"archive\""), "config: {saved}");
    assert!(
        saved.contains(&format!("{:?}", world.inbox_dir)),
        "config: {saved}"
    );
}

/// What a look at the sidebar finds: whether the chosen row lies in the
/// window of the scrollable and how far it is scrolled.
#[derive(Default)]
pub struct SidebarLook {
    viewport: Option<(Rectangle, Rectangle, Vector)>,
    row: Option<Rectangle>,
}

impl SidebarLook {
    pub fn scrolled(&self) -> f32 {
        self.viewport
            .map_or(0.0, |(_, _, translation)| translation.y)
    }

    pub fn row_in_view(&self) -> Option<bool> {
        let ((viewport, content, translation), row) = self.viewport.zip(self.row)?;
        let top = row.y - content.y;
        Some(top >= translation.y && top + row.height <= translation.y + viewport.height)
    }
}

struct LookAtSidebar(Arc<Mutex<SidebarLook>>);

impl Operation for LookAtSidebar {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if id == Some(&Id::new("nav-list-chosen")) {
            self.0.lock().unwrap().row = Some(bounds);
        }
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        content_bounds: Rectangle,
        translation: Vector,
        _state: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        if id == Some(&Id::new("nav-list-scroll")) {
            self.0.lock().unwrap().viewport = Some((bounds, content_bounds, translation));
        }
    }

    fn finish(&self) -> Outcome<()> {
        Outcome::None
    }
}

pub fn look_at_sidebar<P: Live>(session: &mut Session<P>) -> SidebarLook {
    let look = Arc::new(Mutex::new(SidebarLook::default()));
    session.operate(Box::new(LookAtSidebar(Arc::clone(&look))));
    let found = std::mem::take(&mut *look.lock().unwrap());
    found
}

pub fn toggle<P: Live>(session: &mut Session<P>, question: Question) {
    session.send(Message::Reviews(ReviewsMessage::ToggleGroup(question)));
}
