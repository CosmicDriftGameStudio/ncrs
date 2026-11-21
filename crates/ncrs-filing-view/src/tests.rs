//! Update and title of the filing tab on a hand-built plan, without a model.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ncrs_filing::chat_work::{LiveFile, LivePlan, LiveRun, LiveState, Override};
use ncrs_i18n::Language;
use ncrs_index::{ContentHash, Transfer};

use crate::footer::figures;
use crate::{tab_title, update, Action, Ctx, Message, Session, State};

const TARGET: &str = "/archive";

fn ctx() -> Ctx {
    Ctx {
        lang: Language::German,
        access: None,
        model: None,
    }
}

fn file(from: &str, state: LiveState) -> LiveFile {
    LiveFile {
        from: PathBuf::from(from),
        hash: Some(ContentHash([1; 32])),
        state,
        by_user: false,
    }
}

fn filed(folder: &str, name: &str) -> LiveState {
    LiveState::Filed {
        folder: PathBuf::from(folder),
        name: name.to_owned(),
        unsure: false,
    }
}

fn state_with(files: Vec<LiveFile>, done: bool) -> State {
    let mut session = Session::new(
        PathBuf::from("/inbox"),
        PathBuf::from(TARGET),
        Arc::new(LiveRun::default()),
    );
    session.plan = Some(LivePlan {
        source_root: PathBuf::from("/inbox"),
        target_root: PathBuf::from(TARGET),
        files,
        done,
        folder_choice_unsupported: false,
    });
    session.selected = Some(PathBuf::from("a.pdf"));
    State {
        session: Some(session),
    }
}

fn session(state: &State) -> &Session {
    state.session.as_ref().unwrap()
}

fn simple(done: bool) -> State {
    state_with(
        vec![
            file("a.pdf", filed("Rechnungen", "a.pdf")),
            file("b.pdf", LiveState::NoFolder),
            file(
                "c.pdf",
                LiveState::AlreadyThere {
                    at: PathBuf::from("x/c.pdf"),
                },
            ),
        ],
        done,
    )
}

fn send(state: &mut State, message: Message) -> Option<Action> {
    update(state, message, &ctx()).1
}

fn shown_name(state: &State) -> String {
    let plan = session(state).shown_plan().unwrap();
    match &plan.files[0].state {
        LiveState::Filed { name, .. } => name.clone(),
        other => panic!("not filed: {other:?}"),
    }
}

#[test]
fn a_typed_name_replaces_the_name_of_the_file() {
    let mut state = simple(true);
    send(&mut state, Message::StartRename);
    assert_eq!(session(&state).renaming.as_deref(), Some("a.pdf"));
    send(
        &mut state,
        Message::RenameInput("Rechnung Muster GmbH.pdf".into()),
    );
    send(&mut state, Message::RenameSubmit);
    assert_eq!(session(&state).renaming, None);
    assert_eq!(shown_name(&state), "Rechnung Muster GmbH.pdf");
}

#[test]
fn escape_while_renaming_keeps_the_name() {
    let mut state = simple(true);
    send(&mut state, Message::StartRename);
    send(&mut state, Message::RenameInput("other.pdf".into()));
    send(&mut state, Message::RenameCancel);
    assert_eq!(session(&state).renaming, None);
    assert!(session(&state).overrides.is_empty());
    assert_eq!(shown_name(&state), "a.pdf");
}

#[test]
fn an_empty_name_or_one_with_a_separator_keeps_the_field_open() {
    for bad in ["", "  ", "x/y.pdf", ".", ".."] {
        let mut state = simple(true);
        send(&mut state, Message::StartRename);
        send(&mut state, Message::RenameInput(bad.into()));
        send(&mut state, Message::RenameSubmit);
        assert!(session(&state).renaming.is_some(), "{bad:?}");
        assert!(session(&state).overrides.is_empty(), "{bad:?}");
        assert!(session(&state).error.is_some(), "{bad:?}");
    }
}

#[test]
fn the_end_of_an_older_planning_does_not_finish_the_new_session() {
    let mut state = simple(false);
    let older = session(&state).id;
    state.session = Some(Session::new(
        PathBuf::from("/inbox"),
        PathBuf::from(TARGET),
        Arc::new(LiveRun::default()),
    ));
    send(&mut state, Message::PlanFinished(older, Ok(())));
    assert!(crate::is_running(&state));
    let current = session(&state).id;
    send(&mut state, Message::PlanFinished(current, Ok(())));
    assert!(!crate::is_running(&state));
}

#[test]
fn a_file_that_is_not_filed_cannot_be_renamed() {
    let mut state = simple(true);
    send(&mut state, Message::Select(PathBuf::from("b.pdf")));
    send(&mut state, Message::StartRename);
    assert_eq!(session(&state).renaming, None);
}

#[test]
fn f8_leaves_the_file_where_it_is() {
    let mut state = simple(true);
    send(&mut state, Message::Stay);
    let plan = session(&state).shown_plan().unwrap();
    assert_eq!(plan.files[0].state, LiveState::NoFolder);
    assert!(plan.files[0].by_user);
}

#[test]
fn a_folder_inside_the_write_folder_becomes_the_folder_of_the_file() {
    let mut state = simple(true);
    send(
        &mut state,
        Message::FolderPicked(Some(Path::new(TARGET).join("Verträge/2026"))),
    );
    assert_eq!(
        session(&state).overrides.get(Path::new("a.pdf")),
        Some(&Override::Folder(PathBuf::from("Verträge/2026")))
    );
    assert_eq!(session(&state).error, None);
}

#[test]
fn a_folder_outside_the_write_folder_is_an_error_and_changes_nothing() {
    let mut state = simple(true);
    send(
        &mut state,
        Message::FolderPicked(Some(PathBuf::from("/elsewhere"))),
    );
    assert!(session(&state).overrides.is_empty());
    assert!(session(&state).error.is_some());
}

#[test]
fn picking_a_folder_asks_the_app_starting_in_the_write_folder() {
    let mut state = simple(true);
    let action = send(&mut state, Message::PickFolder);
    assert_eq!(
        action,
        Some(Action::PickFolder {
            start: PathBuf::from(TARGET)
        })
    );
}

#[test]
fn filing_waits_until_the_plan_is_done() {
    let mut state = simple(false);
    let (_, action) = update(&mut state, Message::File(Transfer::Copy), &ctx());
    assert_eq!(action, None);
    assert!(!session(&state).filing);
    assert!(!crate::update::can_file(session(&state)));
    assert!(crate::update::can_file(session(&simple(true))));
}

#[test]
fn the_figures_add_up_to_the_files() {
    let mut state = state_with(
        vec![
            file("a.pdf", filed("R", "a.pdf")),
            file("b.pdf", LiveState::NoFolder),
            file(
                "c.pdf",
                LiveState::AlreadyThere {
                    at: PathBuf::from("c.pdf"),
                },
            ),
            file("d.pdf", LiveState::Unreadable),
            file("e.pdf", LiveState::Waiting),
            file("f.pdf", LiveState::Looking),
        ],
        false,
    );
    let plan = session(&state).shown_plan().unwrap();
    let counts = ncrs_filing::chat_work::tally(&plan);
    assert_eq!(
        counts.filed
            + counts.already_there
            + counts.no_folder
            + counts.unreadable
            + counts.waiting
            + counts.looking,
        plan.files.len()
    );
    let texts = figures(session(&state), &plan, Language::German);
    assert_eq!(texts[0], "4 von 6 gelesen");
    assert_eq!(texts[1], "1 kommen in 1 Ordner");
    state.session = None;
}

#[test]
fn the_title_counts_while_reading_and_says_ready_when_done() {
    let running = state_with(
        vec![
            file("a.pdf", filed("R", "a.pdf")),
            file("b.pdf", LiveState::Looking),
            file("c.pdf", LiveState::Waiting),
        ],
        false,
    );
    assert_eq!(
        tab_title(&running, Language::German).as_deref(),
        Some("Ablegen · inbox 1/3")
    );
    let done = simple(true);
    assert_eq!(
        tab_title(&done, Language::German).as_deref(),
        Some("Ablegen · inbox ✓ bereit")
    );
    let mut failed = simple(false);
    failed.session.as_mut().unwrap().finished = Some(Err("boom".into()));
    assert_eq!(
        tab_title(&failed, Language::German).as_deref(),
        Some("Ablegen · inbox ✕")
    );
    assert_eq!(tab_title(&State::default(), Language::German), None);
}

#[test]
fn a_model_that_cannot_choose_folders_is_told_apart_from_one_that_failed() {
    use ncrs_ai::AiError;
    use ncrs_filing::chat_work::PlanFailure;

    let unsupported =
        crate::update::failure_text(Language::English, &PlanFailure::Model(AiError::Unsupported));
    let failed = crate::update::failure_text(
        Language::English,
        &PlanFailure::Model(AiError::Transport("down")),
    );
    let other = crate::update::failure_text(
        Language::English,
        &PlanFailure::Other("disk full".to_owned()),
    );

    assert_ne!(unsupported, failed);
    assert!(!failed.contains("down"));
    assert_eq!(other, "disk full");
}

#[test]
fn a_missing_agreement_to_send_folder_names_is_said_and_not_shown_as_no_folder() {
    use ncrs_ai::{AiError, OutgoingPreview};
    use ncrs_filing::chat_work::PlanFailure;

    let refused = crate::update::failure_text(
        Language::English,
        &PlanFailure::Model(AiError::ConsentRequired(OutgoingPreview {
            endpoint: "https://ai.example.com".to_owned(),
            model: "m".to_owned(),
            body: String::new(),
        })),
    );
    let failed = crate::update::failure_text(
        Language::English,
        &PlanFailure::Model(AiError::Transport("down")),
    );

    assert!(refused.contains("agreement"), "{refused}");
    assert_ne!(refused, failed);
}

#[test]
fn cancel_waits_while_the_files_are_carried_out() {
    let mut state = simple(true);
    state.session.as_mut().unwrap().filing = true;

    let action = send(&mut state, Message::Cancel);

    assert_eq!(action, None);
    assert!(state.session.is_some());
}

#[test]
fn cancel_closes_the_tab_when_nothing_runs() {
    let mut state = simple(true);

    let action = send(&mut state, Message::Cancel);

    assert_eq!(action, Some(Action::Close));
    assert!(state.session.is_none());
}

#[test]
fn nothing_is_filed_when_no_file_goes_anywhere() {
    let mut state = state_with(vec![file("a.pdf", LiveState::NoFolder)], true);

    send(&mut state, Message::File(Transfer::Copy));

    assert!(!session(&state).filing);
}

#[test]
fn enter_copies_and_shift_enter_moves() {
    use iced::keyboard::key::Named;
    use iced::keyboard::{Key, Modifiers};

    let enter = Key::Named(Named::Enter);
    assert_eq!(
        crate::filing_key(&enter, Modifiers::empty(), false),
        Some(Message::File(Transfer::Copy))
    );
    assert_eq!(
        crate::filing_key(&enter, Modifiers::SHIFT, false),
        Some(Message::File(Transfer::Move))
    );
    assert_eq!(crate::filing_key(&enter, Modifiers::empty(), true), None);
}

#[test]
fn the_tab_says_while_the_files_are_carried_out() {
    let mut state = simple(true);
    state.session.as_mut().unwrap().filing = true;

    let title = tab_title(&state, Language::English).unwrap();

    assert!(!title.contains('✓'));
}
