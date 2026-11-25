//! The Pull and Push menus and the login dialog, driven through `update`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

use ncrs_git::askpass::{AskRequest, PromptKind, Secret};
use ncrs_git::PullStrategy;
use ncrs_git_view::actions::Dialog;
use ncrs_git_view::sync_menu::{SyncAction, SyncKind};
use ncrs_git_view::{update, Ctx, Message, State};
use ncrs_i18n::Language;

fn ask(kind: PromptKind) -> AskRequest {
    AskRequest {
        id: 7,
        job: 1,
        prompt: "Password for 'https://example.test': ".to_owned(),
        kind,
    }
}

fn dialog_is_open(state: &State) -> bool {
    matches!(state.actions.dialog, Some(Dialog::Credential(_)))
}

#[test]
fn the_pull_menu_starts_on_the_default_way() {
    let mut ctx = Ctx::new(Language::English);
    ctx.pull_default = PullStrategy::Rebase;
    let mut state = State::default();
    let _ = update(&mut state, Message::OpenSyncMenu(SyncKind::Pull), &ctx);
    let menu = state.actions.sync_menu.unwrap();
    assert_eq!(
        menu.kind.entries()[menu.cursor],
        SyncAction::Pull(PullStrategy::Rebase)
    );
}

#[test]
fn the_arrow_again_and_escape_close_the_menu() {
    let ctx = Ctx::new(Language::English);
    let mut state = State::default();
    let _ = update(&mut state, Message::OpenSyncMenu(SyncKind::Push), &ctx);
    let _ = update(&mut state, Message::OpenSyncMenu(SyncKind::Push), &ctx);
    assert!(state.actions.sync_menu.is_none());
    let _ = update(&mut state, Message::OpenSyncMenu(SyncKind::Push), &ctx);
    let _ = update(&mut state, Message::Escape, &ctx);
    assert!(state.actions.sync_menu.is_none());
}

#[test]
fn the_menu_of_one_button_replaces_the_other() {
    let ctx = Ctx::new(Language::English);
    let mut state = State::default();
    let _ = update(&mut state, Message::OpenSyncMenu(SyncKind::Pull), &ctx);
    let _ = update(&mut state, Message::OpenSyncMenu(SyncKind::Push), &ctx);
    assert_eq!(state.actions.sync_menu.unwrap().kind, SyncKind::Push);
}

#[test]
fn a_login_question_opens_the_dialog_and_enter_sends_what_was_typed() {
    let ctx = Ctx::new(Language::English);
    let mut state = State::default();
    let _ = update(&mut state, Message::CredentialAsked(ask(PromptKind::Secret)), &ctx);
    assert!(dialog_is_open(&state));
    let _ = update(&mut state, Message::CredentialSend(7), &ctx);
    assert!(dialog_is_open(&state), "an empty password is not sent");
    let _ = update(&mut state, Message::CredentialTyped(7, Secret::new("hunter2")), &ctx);
    let _ = update(&mut state, Message::Enter, &ctx);
    assert!(dialog_is_open(&state), "the field sends its own Enter, with the id");
    let _ = update(&mut state, Message::CredentialSend(7), &ctx);
    assert!(!dialog_is_open(&state));
}

#[test]
fn a_message_for_another_question_is_dropped() {
    let ctx = Ctx::new(Language::English);
    let mut state = State::default();
    let _ = update(&mut state, Message::CredentialAsked(ask(PromptKind::Secret)), &ctx);
    let _ = update(&mut state, Message::CredentialTyped(6, Secret::new("late")), &ctx);
    let Some(Dialog::Credential(shown)) = &state.actions.dialog else {
        panic!("the dialog should show");
    };
    assert!(shown.typed.expose().is_empty());
    let _ = update(&mut state, Message::CredentialSend(6), &ctx);
    let _ = update(&mut state, Message::CredentialCancel(6), &ctx);
    assert!(dialog_is_open(&state));
}

#[test]
fn enter_refuses_an_unknown_host_and_only_the_button_connects() {
    let ctx = Ctx::new(Language::English);
    let mut state = State::default();
    let _ = update(&mut state, Message::CredentialAsked(ask(PromptKind::Confirm)), &ctx);
    let _ = update(&mut state, Message::Enter, &ctx);
    assert!(!dialog_is_open(&state), "Enter cancelled it");
    let _ = update(&mut state, Message::CredentialAsked(ask(PromptKind::Confirm)), &ctx);
    let _ = update(&mut state, Message::CredentialSend(7), &ctx);
    assert!(!dialog_is_open(&state), "the button connected");
}

#[test]
fn cancelling_the_login_closes_the_dialog() {
    let ctx = Ctx::new(Language::English);
    let mut state = State::default();
    let _ = update(&mut state, Message::CredentialAsked(ask(PromptKind::Text)), &ctx);
    let _ = update(&mut state, Message::Escape, &ctx);
    assert!(!dialog_is_open(&state));
}

#[test]
fn a_second_question_waits_for_the_first() {
    let ctx = Ctx::new(Language::English);
    let mut state = State::default();
    let _ = update(&mut state, Message::CredentialAsked(ask(PromptKind::Confirm)), &ctx);
    let mut second = ask(PromptKind::Text);
    second.id = 8;
    let _ = update(&mut state, Message::CredentialAsked(second), &ctx);
    let _ = update(&mut state, Message::Enter, &ctx);
    let Some(Dialog::Credential(next)) = &state.actions.dialog else {
        panic!("the second question should show");
    };
    assert_eq!(next.id, 8);
}

#[test]
fn the_quiet_fetch_asks_nothing_and_shows_no_notice() {
    let ctx = Ctx::new(Language::English);
    let mut state = State::default();
    let _ = update(&mut state, Message::BackgroundFetch, &ctx);
    assert!(state.actions.busy.is_none());
    assert!(state.actions.notice.is_none());
}

#[test]
fn a_force_push_is_not_started_where_the_branch_has_no_upstream() {
    use ncrs_git_view::actions::{ForceBlock, SyncBar};
    let ctx = Ctx::new(Language::English);
    let mut state = State::default();
    state.actions.sync = Some(SyncBar {
        force_blocked: Some(ForceBlock::NoUpstream),
        ..SyncBar::default()
    });
    let _ = update(&mut state, Message::ForcePush, &ctx);
    assert!(state.actions.busy.is_none());
}

#[test]
fn every_pull_way_is_a_command() {
    use ncrs_git_view::message_of_command;
    let wanted = [
        Message::SyncMenuPressed(SyncAction::Pull(PullStrategy::Merge)),
        Message::SyncMenuPressed(SyncAction::Pull(PullStrategy::Rebase)),
        Message::SyncMenuPressed(SyncAction::Pull(PullStrategy::FastForwardOnly)),
    ];
    let offered: Vec<_> = (0..64).filter_map(message_of_command).collect();
    for message in wanted {
        assert!(offered.contains(&message), "{message:?}");
    }
}

#[test]
fn the_quiet_fetch_waits_until_the_user_has_synced_here_once() {
    let ctx = Ctx::new(Language::English);
    let mut state = State::default();
    state.root = Some(std::path::PathBuf::from("/nowhere/repository"));
    let _ = update(&mut state, Message::BackgroundFetch, &ctx);
    assert!(!state.actions.background_fetch, "never fetched by hand here");
    state.actions.deliberate_sync = true;
    let _ = update(&mut state, Message::BackgroundFetch, &ctx);
    assert!(state.actions.background_fetch);
    assert!(state.actions.busy.is_none(), "it shows nothing");
}
