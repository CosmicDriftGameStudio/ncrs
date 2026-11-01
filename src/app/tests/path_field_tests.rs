use crate::app::*;

/// Edits the field: its first keystroke, with the panel's path as it was.
fn open(app: &mut App) {
    let shown = app.active_panel().path.display().to_string();
    drop(app.update(Message::PathFieldInput(shown)));
}

fn type_path(app: &mut App, typed: &str) {
    drop(app.update(Message::PathFieldInput(typed.to_string())));
}

#[test]
fn typing_replaces_what_the_field_shows_and_a_cancel_restores_the_panels_path() {
    let mut app = App::with_fixed_panels();
    assert_eq!(
        app.panels.path_field, None,
        "unedited, the field shows the panel"
    );
    type_path(&mut app, "/home/test/links/Docu");
    assert_eq!(
        app.panels.path_field.as_deref(),
        Some("/home/test/links/Docu")
    );
    drop(app.update(Message::PathFieldCancel));
    assert_eq!(app.panels.path_field, None);
}

fn submit(app: &mut App, typed: &str) -> u64 {
    open(app);
    type_path(app, typed);
    drop(app.update(Message::PathFieldSubmit));
    app.panels.path_check_id
}

fn checked(app: &mut App, request_id: u64, result: Result<fs::Destination, fs::PathProblem>) {
    drop(app.update(Message::PathChecked {
        request_id,
        side: PanelSide::Left,
        target: PathBuf::from("/typed/path"),
        result,
    }));
}

#[test]
fn submitting_does_not_touch_the_field_or_the_panel_until_the_answer_is_in() {
    let mut app = App::with_fixed_panels();
    let before = app.active_panel().request_id;
    submit(&mut app, "/typed/path");
    assert_eq!(app.panels.path_field.as_deref(), Some("/typed/path"));
    assert_eq!(app.active_panel().request_id, before);
}

#[test]
fn a_destination_closes_the_field_and_loads_it() {
    let mut app = App::with_fixed_panels();
    let id = submit(&mut app, "/typed/path");
    let before = app.active_panel().request_id;
    checked(
        &mut app,
        id,
        Ok(fs::Destination {
            directory: PathBuf::from("/typed/path"),
            select: None,
        }),
    );
    assert_eq!(app.panels.path_field, None);
    assert_ne!(app.active_panel().request_id, before, "a load started");
}

#[test]
fn a_missing_path_is_reported_and_the_field_stays_open() {
    let mut app = App::with_fixed_panels();
    let id = submit(&mut app, "/typed/path");
    let before = app.active_panel().request_id;
    checked(&mut app, id, Err(fs::PathProblem::Missing));
    assert_eq!(app.panels.path_field.as_deref(), Some("/typed/path"));
    assert_eq!(app.active_panel().request_id, before, "nothing was loaded");
    assert!(app
        .job_error
        .as_deref()
        .is_some_and(|error| error.contains("/typed/path") && error.contains("exist")));
}

#[test]
fn an_unreadable_path_says_why_and_is_not_called_missing() {
    let mut app = App::with_fixed_panels();
    let id = submit(&mut app, "/typed/path");
    checked(
        &mut app,
        id,
        Err(fs::PathProblem::Unreadable {
            reason: "Permission denied".to_string(),
        }),
    );
    let error = app.job_error.clone().unwrap();
    assert!(error.contains("Permission denied"), "{error}");
    assert!(!error.contains("does not exist"), "{error}");
}

#[test]
fn an_answer_to_text_that_has_changed_or_closed_is_dropped() {
    let destination = || {
        Ok(fs::Destination {
            directory: PathBuf::from("/typed/path"),
            select: None,
        })
    };
    let mut app = App::with_fixed_panels();
    let first_id = submit(&mut app, "/typed/path");
    type_path(&mut app, "/typed/path/more");
    let before = app.active_panel().request_id;
    checked(&mut app, first_id, destination());
    assert_eq!(app.panels.path_field.as_deref(), Some("/typed/path/more"));
    assert_eq!(app.active_panel().request_id, before);

    let second_id = submit(&mut app, "/typed/path");
    drop(app.update(Message::PathFieldCancel));
    checked(&mut app, second_id, destination());
    assert_eq!(
        app.active_panel().request_id,
        before,
        "the field was closed"
    );
}

#[test]
fn escape_click_outside_and_other_actions_end_the_editing() {
    for leave in [
        Message::PathFieldCancel,
        Message::ClickedOutside,
        Message::Panel(PanelMessage::SwitchPanel),
        Message::Panel(PanelMessage::RowClicked {
            side: PanelSide::Left,
            index: 1,
            on_tag: false,
        }),
        Message::Panel(PanelMessage::SortBy {
            side: PanelSide::Left,
            column: fs::SortColumn::Size,
        }),
    ] {
        let mut app = App::with_fixed_panels();
        open(&mut app);
        drop(app.update(leave));
        assert_eq!(app.panels.path_field, None);
    }
}

#[test]
fn while_editing_the_panels_get_no_keys_but_quit() {
    let mut app = App::with_fixed_panels();
    open(&mut app);
    let state = app.prompt_key_state();
    assert!(state.path_editing);
    let none = Modifiers::default();
    assert_eq!(route_key(&state, Key::Named(Named::ArrowDown), none), None);
    // Escape and Tab reach the app from the input state, not from here.
    assert_eq!(route_key(&state, Key::Named(Named::Escape), none), None);
    assert_eq!(
        route_key(&state, Key::Named(Named::F10), none),
        Some(Message::Quit)
    );
    let quit_chord = if cfg!(target_os = "macos") {
        Modifiers::COMMAND
    } else {
        Modifiers::CTRL
    };
    assert_eq!(
        route_key(&state, Key::Character("q".into()), quit_chord),
        Some(Message::Quit)
    );
}

#[test]
fn the_copy_icon_copies_the_active_panels_directory() {
    let app = App::with_fixed_panels();
    let copied =
        ncrs_ops::clipboard_text(rows_of(app.active_panel()), ClipboardKind::Directory).unwrap();
    assert_eq!(copied.text, "/home/test/links");
    assert_eq!(copied.count, 1);
}
