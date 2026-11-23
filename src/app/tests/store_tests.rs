use crate::app::*;
use crate::fs::FileEntry;
use crate::messages::StoreMessage;
use iced::widget::text_editor;
use ncrs_store::{Entitlement, FullKind};
use std::ffi::OsString;

fn app_with_a_file(entitlement: Entitlement) -> App {
    let mut app = App::with_environment(Environment::isolated()).0;
    app.panels.left.path = PathBuf::from("/left");
    app.panels.right.path = PathBuf::from("/right");
    app.panels.left.entries = vec![FileEntry {
        name: OsString::from("a.txt"),
        path: PathBuf::from("/left/a.txt"),
        is_dir: false,
        is_symlink: false,
        is_parent: false,
        size: 1,
        modified: None,
        folder_size: None,
        details: Default::default(),
    }];
    app.store.set_entitlement(entitlement);
    app
}

fn refusal(app: &App, reason: Msg) -> String {
    ncrs_i18n::fill_template(
        app.lang.text(Msg::StoreLockedPress),
        &[("{reason}", app.lang.text(reason))],
    )
}

#[tokio::test]
async fn an_expired_trial_holds_f5_back_and_the_notice_opens_the_page() {
    let mut app = app_with_a_file(Entitlement::Expired);

    drop(app.update(Message::Transfer(TransferKind::Copy)));

    assert!(app.ops.transfer_dialog.is_none(), "the dialog opened");
    assert_eq!(
        app.job_error.as_deref(),
        Some(refusal(&app, Msg::StoreStatusExpired).as_str())
    );
    assert_eq!(
        app.job_error_press,
        Some(Message::Store(StoreMessage::OpenPage))
    );

    drop(app.update(Message::Store(StoreMessage::OpenPage)));
    let state = app.settings.page.as_ref().unwrap();
    assert_eq!(
        settings_ui::shown_feature(&app.settings_view(state)),
        Some(crate::messages::SettingsFeature::FullVersion)
    );
}

#[tokio::test]
async fn a_trial_that_was_not_started_names_the_way_in() {
    let mut app = app_with_a_file(Entitlement::TrialNotStarted);

    drop(app.update(Message::Transfer(TransferKind::Move)));

    assert!(app.ops.transfer_dialog.is_none());
    assert_eq!(
        app.job_error.as_deref(),
        Some(refusal(&app, Msg::StoreLockedNotStarted).as_str())
    );
}

#[tokio::test]
async fn a_running_trial_and_a_purchase_let_f5_through() {
    for entitlement in [
        Entitlement::Trial { days_left: 3 },
        Entitlement::Full(FullKind::Purchased),
        Entitlement::Unrestricted,
    ] {
        let mut app = app_with_a_file(entitlement);

        drop(app.update(Message::Transfer(TransferKind::Copy)));

        assert!(app.job_error.is_none(), "{entitlement:?}");
        assert!(app.ops.transfer_dialog.is_some(), "{entitlement:?}");
    }
}

#[tokio::test]
async fn a_locked_app_still_browses_and_searches() {
    let mut app = app_with_a_file(Entitlement::Expired);

    drop(app.update(Message::OpenSearch));

    assert!(app.job_error.is_none());
}

fn typing() -> Message {
    Message::Notes(ncrs_notes::Message::Edited(text_editor::Action::Edit(
        text_editor::Edit::Insert('a'),
    )))
}

#[tokio::test]
async fn editing_a_note_waits_for_the_full_version_but_moving_the_cursor_does_not() {
    let mut locked = app_with_a_file(Entitlement::Expired);
    drop(locked.update(typing()));
    assert_eq!(
        locked.job_error.as_deref(),
        Some(refusal(&locked, Msg::StoreStatusExpired).as_str())
    );

    let mut moving = app_with_a_file(Entitlement::Expired);
    drop(moving.update(Message::Notes(ncrs_notes::Message::Edited(
        text_editor::Action::Move(text_editor::Motion::Left),
    ))));
    assert!(moving.job_error.is_none());

    let mut trial = app_with_a_file(Entitlement::Trial { days_left: 9 });
    drop(trial.update(typing()));
    assert!(trial.job_error.is_none());
}

#[tokio::test]
async fn running_a_hermes_plan_waits_for_the_full_version() {
    let run = || Message::FilingView(ncrs_filing_view::Message::File(ncrs_index::Transfer::Move));

    let mut locked = app_with_a_file(Entitlement::TrialNotStarted);
    drop(locked.update(run()));
    assert_eq!(
        locked.job_error.as_deref(),
        Some(refusal(&locked, Msg::StoreLockedNotStarted).as_str())
    );

    let mut trial = app_with_a_file(Entitlement::Trial { days_left: 9 });
    drop(trial.update(run()));
    assert!(trial.job_error.is_none());
}

#[test]
fn the_status_bar_reminds_in_the_last_days_and_after_the_trial() {
    let hint = |entitlement| {
        App::with_fixed_panels()
            .with_entitlement(entitlement)
            .store_badge()
            .map(|badge| badge.text)
    };
    let lang = Language::default();
    assert_eq!(hint(Entitlement::Trial { days_left: 9 }), None);
    assert_eq!(hint(Entitlement::Full(FullKind::Purchased)), None);
    assert_eq!(
        hint(Entitlement::Trial { days_left: 2 }),
        Some(lang.text(Msg::StoreBadgeTrial).replace("{days}", "2"))
    );
    assert_eq!(
        hint(Entitlement::Expired),
        Some(lang.text(Msg::StoreBadgeExpired).to_owned())
    );
}
