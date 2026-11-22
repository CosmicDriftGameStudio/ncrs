//! Flow 3, the questions that need a value or a decision: the date, the
//! amount, the rule, the sender and the folder.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use iced::Task;
use ncrs_hermes_ui::reviews_page::{Answer, Question};
use ncrs_index::Field;

use super::drive;
use super::probe::World;
use crate::e2e_tests::acceptance_v160::Live;
use crate::e2e_tests::Session;
use crate::hermes_protocol::thread::entry_text;
use crate::messages::{Message, ReviewsMessage, RuleFormMessage, RulesMessage};

static PICKED: Mutex<Option<PathBuf>> = Mutex::new(None);

fn picker(_start_in: &Path) -> Task<Option<PathBuf>> {
    Task::done(PICKED.lock().unwrap().take())
}

pub fn run<P: Live>(world: &World, session: &mut Session<P>) {
    session.app.set_folder_picker_for_test(picker);
    drive::show_reviews(session);
    dates(world, session);
    amounts(world, session);
    senders(world, session);
    rules(world, session);
    no_rule(world, session);
}

/// The chat is open on the document, with a question about it.
fn chat_is_about<P: Live>(session: &Session<P>, path: &Path) {
    let chat = session.app.hermes_chat_for_test();
    assert!(chat.open, "the chat did not open");
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    let said = chat.thread().last().map(entry_text).unwrap_or_default();
    assert!(
        said.contains(&name),
        "the chat does not ask about {name}: {said:?}"
    );
}

fn dates<P: Live>(world: &World, session: &mut Session<P>) {
    let files = world.inbox.no_date.clone();
    assert!(files.len() >= 3);
    let (typed, ignored) = (&files[0], &files[1]);

    // A date typed in the row: the file lands with it in its name.
    let id = drive::document(session, typed);
    drive::type_into(session, typed, "07.03.2025");
    drive::answer_file(session, typed, Answer::FileTyped);
    assert!(!typed.exists(), "the typed file was not filed");
    let landed = drive::journal_moved(world, typed).expect("no journal item for the typed date");
    assert!(landed.is_file(), "{landed:?} is not there");
    assert!(
        landed
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("2025-03-07"),
        "the typed date is not in the name: {landed:?}"
    );
    let stored = world.index().document(id).unwrap().unwrap();
    assert_eq!(
        stored.fields.date,
        chrono::NaiveDate::from_ymd_opt(2025, 3, 7)
    );
    assert!(!drive::has_open_review(world, typed));
    drive::config_intact(world);

    let text = drive::text_of(ignored);
    drive::answer_file(session, ignored, Answer::Exclude);
    assert_eq!(drive::text_of(ignored), text);
    assert!(world.index().is_excluded(ignored).unwrap());

    // "Ohne Datum ablegen": every pattern of the archive holds {date}, so no
    // undated letter has a place; the button must not be offered, and nothing
    // may move.
    let offered = drive::page(session).group_answers(Question::Date);
    assert!(
        offered.iter().all(|(answer, _)| *answer != Answer::Accept),
        "'Ohne Datum ablegen' is offered without a place: {offered:?}"
    );
    assert!(
        files[2..]
            .iter()
            .all(|path| path.exists() && drive::has_open_review(world, path)),
        "a file without a place was moved"
    );
    drive::config_intact(world);
    drive::shot(session, "reviews-after-date");
}

fn amounts<P: Live>(world: &World, session: &mut Session<P>) {
    let question = Question::Missing(Field::Amount);
    let files = world.inbox.no_amount.clone();
    assert!(files.len() >= 3);
    let (chat, ignored) = (&files[0], &files[1]);

    drive::answer_file(session, chat, Answer::SetFields);
    chat_is_about(session, chat);
    assert!(chat.exists(), "asking in the chat moved the file");
    drive::show_reviews(session);

    let text = drive::text_of(ignored);
    drive::answer_file(session, ignored, Answer::Exclude);
    assert_eq!(drive::text_of(ignored), text);
    assert!(!drive::has_open_review(world, ignored));

    let handled = drive::answer_group(session, question, Answer::Accept);
    assert_eq!(
        handled,
        files.len() - 1,
        "the group button counts the files it handles"
    );
    // The one asked about in the chat still has its review, so the button takes it too.
    for path in std::iter::once(chat).chain(&files[2..]) {
        assert!(!path.exists(), "{path:?} was not filed without an amount");
        let landed = drive::journal_moved(world, path).expect("no journal item");
        assert!(landed.is_file());
    }
    assert!(ignored.exists());
    drive::config_intact(world);
    drive::shot(session, "reviews-after-amount");
}

fn rules<P: Live>(world: &World, session: &mut Session<P>) {
    let files = world.inbox.unconfirmed.clone();
    assert!(files.len() >= 5);
    let rules_before = world.index().rules().unwrap().len();

    // "Nur diesmal": filed, and no rule comes of it.
    drive::answer_file(session, &files[0], Answer::Accept);
    assert!(!files[0].exists());
    assert!(drive::journal_moved(world, &files[0]).is_some_and(|landed| landed.is_file()));
    assert_eq!(
        world.index().rules().unwrap().len(),
        rules_before,
        "a rule was made"
    );

    let text = drive::text_of(&files[1]);
    drive::answer_file(session, &files[1], Answer::Exclude);
    assert_eq!(drive::text_of(&files[1]), text);

    // "Regel übernehmen": the form opens on the first document of the group;
    // saving it puts a rule into the index and moves nothing by itself.
    session.send(Message::Reviews(ReviewsMessage::Answer(
        Question::RuleUnconfirmed,
        Answer::CreateRule,
    )));
    session.send(Message::Rules(RulesMessage::Form(RuleFormMessage::Save)));
    assert_eq!(
        world.index().rules().unwrap().len(),
        rules_before + 1,
        "the rule form did not put a rule into the index"
    );
    drive::show_reviews(session);
    drive::reload(session);
    let left: Vec<&PathBuf> = files[2..].iter().filter(|path| path.exists()).collect();
    println!(
        "after the rule: {} of {} files still wait",
        left.len(),
        files.len() - 2
    );

    // What is left of the group, with its button.
    let waiting = drive::page(session)
        .members(Question::RuleUnconfirmed)
        .len();
    if waiting > 0 {
        let handled = drive::answer_group(session, Question::RuleUnconfirmed, Answer::Accept);
        assert_eq!(handled, waiting);
    }
    assert!(
        files[2..].iter().all(|path| !path.exists()),
        "files of the group are left"
    );
    drive::config_intact(world);
}

fn senders<P: Live>(world: &World, session: &mut Session<P>) {
    let files = world.inbox.unsure_sender_marked.clone();
    assert!(files.len() >= 4);
    let sender = Question::Sender;

    // "Anderer Absender…": typed in the row, it shows in the folder and in the index.
    let id = drive::document(session, &files[0]);
    drive::type_into(session, &files[0], "Anderer Verlag GmbH");
    drive::answer_file(session, &files[0], Answer::FileTyped);
    assert!(!files[0].exists());
    let typed = drive::journal_moved(world, &files[0]).expect("no journal item");
    assert!(
        typed.to_string_lossy().contains("Anderer Verlag GmbH"),
        "the typed sender is not in {typed:?}"
    );
    assert_eq!(
        world
            .index()
            .document(id)
            .unwrap()
            .unwrap()
            .fields
            .sender
            .as_deref(),
        Some("Anderer Verlag GmbH")
    );

    let text = drive::text_of(&files[1]);
    drive::answer_file(session, &files[1], Answer::Exclude);
    assert_eq!(drive::text_of(&files[1]), text);
    assert!(world.index().is_excluded(&files[1]).unwrap());
    assert_eq!(drive::page(session).members(sender).len(), files.len() - 2);

    // "Ja, ablegen" is offered on the group only, not on a row.
    let handled = drive::answer_group(session, sender, Answer::Accept);
    assert_eq!(handled, files.len() - 2);
    for path in &files[2..] {
        assert!(!path.exists());
        let landed = drive::journal_moved(world, path).expect("no journal item");
        assert!(
            landed.to_string_lossy().contains(archive_sender()),
            "{landed:?}"
        );
    }
    drive::config_intact(world);
}

fn archive_sender() -> &'static str {
    super::archive::UNCONFIRMED_SENDER
}

/// Without a rule: pick a folder, ask the chat, ignore; the others stay for
/// the hand filing.
fn no_rule<P: Live>(world: &World, session: &mut Session<P>) {
    let files: Vec<PathBuf> = world
        .inbox
        .no_rule
        .iter()
        .chain(&world.inbox.unsure_sender)
        .cloned()
        .collect();
    assert!(files.len() >= 6);
    let (folder_file, chat_file, ignored) = (&files[0], &files[1], &files[2]);

    // "Ordner wählen…": the dialog answers with a folder.
    let folder = world.archive_dir.join("Neu sortiert");
    std::fs::create_dir_all(&folder).unwrap();
    *PICKED.lock().unwrap() = Some(folder.clone());
    drive::answer_file(session, folder_file, Answer::PickFolder);
    assert!(!folder_file.exists(), "the file stayed in the inbox");
    let moved = drive::journal_moved(world, folder_file).expect("no journal item");
    assert!(moved.starts_with(&folder) && moved.is_file(), "{moved:?}");
    assert!(!drive::has_open_review(world, folder_file));

    drive::answer_file(session, chat_file, Answer::SetFields);
    chat_is_about(session, chat_file);
    assert!(chat_file.exists());
    drive::show_reviews(session);

    let text = drive::text_of(ignored);
    drive::answer_file(session, ignored, Answer::Exclude);
    assert_eq!(drive::text_of(ignored), text);

    drive::config_intact(world);
}
