use crate::app::*;
use crate::command_line::ShellOutcome;
use crate::commands::command_of;

const NONE: Modifiers = Modifiers::empty();

fn line(message: CommandLineMessage) -> Option<Message> {
    Some(Message::CommandLine(message))
}

fn focused_app() -> App {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::CommandLine(CommandLineMessage::Focus)));
    app
}

fn type_line(app: &mut App, text: &str) {
    drop(app.update(Message::CommandLine(CommandLineMessage::Input(
        text.to_owned(),
    ))));
}

fn submit(app: &mut App, text: &str) {
    type_line(app, text);
    drop(app.update(Message::CommandLine(CommandLineMessage::Submit)));
}

fn cd_answer(
    app: &App,
    request_id: u64,
    result: Result<fs::Destination, fs::PathProblem>,
) -> Message {
    Message::CommandLine(CommandLineMessage::CdChecked {
        request_id,
        side: app.panels.active,
        target: PathBuf::from("/home/test/links/sub"),
        result,
    })
}

fn sub_destination() -> Result<fs::Destination, fs::PathProblem> {
    Ok(fs::Destination {
        directory: PathBuf::from("/home/test/links/sub"),
        select: None,
    })
}

#[test]
fn without_the_focus_letters_stay_type_ahead_and_f2_focuses() {
    let idle = PromptKeyState::default();
    let typed = route_key(&idle, Key::Character("l".into()), NONE);
    assert_eq!(
        typed,
        Some(Message::Panel(PanelMessage::TypeAhead("l".into())))
    );
    assert_eq!(
        route_key(&idle, Key::Named(Named::F2), NONE),
        line(CommandLineMessage::Focus)
    );
}

#[test]
fn with_the_focus_only_history_leaving_and_quitting_get_through() {
    let state = PromptKeyState {
        command_line: true,
        ..PromptKeyState::default()
    };
    let route = |key: Key, modifiers| route_key(&state, key, modifiers);
    assert_eq!(route(Key::Character("5".into()), NONE), None);
    assert_eq!(route(Key::Named(Named::F5), NONE), None);
    assert_eq!(route(Key::Named(Named::Tab), NONE), None);
    assert_eq!(
        route(Key::Named(Named::ArrowUp), NONE),
        line(CommandLineMessage::HistoryPrevious)
    );
    assert_eq!(
        route(Key::Named(Named::ArrowDown), NONE),
        line(CommandLineMessage::HistoryNext)
    );
    assert_eq!(
        route(Key::Named(Named::Escape), NONE),
        line(CommandLineMessage::Leave)
    );
    assert_eq!(
        route(Key::Named(Named::F2), NONE),
        line(CommandLineMessage::Leave)
    );
    let quit_chord = if cfg!(target_os = "macos") {
        Modifiers::COMMAND
    } else {
        Modifiers::CTRL
    };
    assert_eq!(
        route(Key::Character("q".into()), quit_chord),
        Some(Message::Quit)
    );
}

#[test]
fn the_history_keys_repeat() {
    assert!(repeats(&Message::CommandLine(
        CommandLineMessage::HistoryPrevious
    )));
    assert!(!repeats(&Message::CommandLine(CommandLineMessage::Submit)));
}

#[test]
fn focusing_ends_a_path_edit_and_is_published_to_the_key_routing() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::PathFieldInput("/typed".into())));
    drop(app.update(Message::CommandLine(CommandLineMessage::Focus)));
    assert_eq!(app.path_field_for_test(), None);
    assert!(app.command_line_for_test().is_focused());
    assert!(app.prompt_key_state().command_line);
    assert!(app.input.lock().command_line);
}

#[test]
fn typing_in_the_path_field_ends_the_command_line() {
    let mut app = focused_app();
    type_line(&mut app, "ls");
    drop(app.update(Message::PathFieldInput("/typed".into())));
    assert!(!app.command_line_for_test().is_focused());
    assert_eq!(app.command_line_for_test().typed(), "");
}

#[test]
fn input_sets_the_text_and_leave_clears_it() {
    let mut app = App::with_fixed_panels();
    type_line(&mut app, "ls -la");
    assert_eq!(app.command_line_for_test().typed(), "ls -la");
    assert!(app.command_line_for_test().is_focused());
    drop(app.update(Message::CommandLine(CommandLineMessage::Leave)));
    assert_eq!(app.command_line_for_test().typed(), "");
    assert!(!app.command_line_for_test().is_focused());
    assert!(!app.prompt_key_state().command_line);
}

#[test]
fn other_actions_end_the_command_line() {
    let mut app = focused_app();
    type_line(&mut app, "ls");
    drop(app.update(Message::Panel(PanelMessage::SwitchPanel)));
    assert!(!app.command_line_for_test().is_focused());
    assert_eq!(app.command_line_for_test().typed(), "");
}

#[test]
fn a_shell_line_is_cleared_remembered_and_counted_as_running() {
    let mut app = focused_app();
    submit(&mut app, "ls");
    let line = app.command_line_for_test();
    assert_eq!(line.typed(), "");
    assert!(!line.is_focused());
    assert_eq!(line.running(), 1);
    drop(app.update(Message::CommandLine(CommandLineMessage::Focus)));
    drop(app.update(Message::CommandLine(CommandLineMessage::HistoryPrevious)));
    assert_eq!(app.command_line_for_test().typed(), "ls");
}

#[test]
fn a_blank_line_just_leaves() {
    let mut app = focused_app();
    submit(&mut app, "   ");
    assert!(!app.command_line_for_test().is_focused());
    assert_eq!(app.command_line_for_test().running(), 0);
}

#[test]
fn cd_keeps_text_and_focus_until_the_answer_is_in() {
    let mut app = focused_app();
    submit(&mut app, "cd sub");
    assert_eq!(app.command_line_for_test().typed(), "cd sub");
    assert!(app.command_line_for_test().is_focused());
    assert_eq!(app.command_line_for_test().running(), 0);
}

#[test]
fn a_found_directory_is_loaded_and_ends_the_line() {
    let mut app = focused_app();
    submit(&mut app, "cd sub");
    let id = app.command_line.next_cd_check();
    let before = app.active_panel().request_id;
    drop(app.update(cd_answer(&app, id, sub_destination())));
    assert!(!app.command_line_for_test().is_focused());
    assert_eq!(app.command_line_for_test().typed(), "");
    assert_ne!(app.active_panel().request_id, before, "a load started");
}

#[test]
fn a_missing_directory_is_reported_and_the_line_stays() {
    let mut app = focused_app();
    submit(&mut app, "cd sub");
    let id = app.command_line.next_cd_check();
    let before = app.active_panel().request_id;
    drop(app.update(cd_answer(&app, id, Err(fs::PathProblem::Missing))));
    assert!(app.command_line_for_test().is_focused());
    assert_eq!(app.command_line_for_test().typed(), "cd sub");
    assert_eq!(app.active_panel().request_id, before);
    let error = app.job_error.clone().unwrap();
    assert!(error.contains("/home/test/links/sub"), "{error}");
}

#[test]
fn an_answer_to_text_that_has_changed_is_dropped() {
    let mut app = focused_app();
    submit(&mut app, "cd sub");
    let stale = app.command_line.next_cd_check();
    type_line(&mut app, "cd sub2");
    let before = app.active_panel().request_id;
    drop(app.update(cd_answer(&app, stale, sub_destination())));
    assert!(app.command_line_for_test().is_focused());
    assert_eq!(app.command_line_for_test().typed(), "cd sub2");
    assert_eq!(app.active_panel().request_id, before);
}

fn finished(app: &mut App, outcome: ShellOutcome) {
    drop(
        app.update(Message::CommandLine(CommandLineMessage::Finished {
            command: "make".into(),
            outcome,
        })),
    );
}

#[test]
fn a_clean_exit_is_a_notice_and_a_failure_an_error() {
    let mut app = App::with_fixed_panels();
    submit(&mut app, "make");
    app.job_error = Some("old".into());
    finished(&mut app, ShellOutcome::Exited(0));
    assert_eq!(app.command_line_for_test().running(), 0);
    assert_eq!(app.job_error, None);
    assert!(app
        .notice
        .as_deref()
        .is_some_and(|notice| notice.contains("exit status 0")));

    finished(&mut app, ShellOutcome::Exited(2));
    assert!(app
        .job_error
        .as_deref()
        .is_some_and(|error| error.contains("exit status 2")));
}

#[test]
fn a_command_with_a_placeholder_in_it_is_shown_as_typed() {
    let mut app = App::with_fixed_panels();
    drop(
        app.update(Message::CommandLine(CommandLineMessage::Finished {
            command: "echo {code}".into(),
            outcome: ShellOutcome::Exited(1),
        })),
    );
    let error = app.job_error.clone().unwrap();
    assert!(error.starts_with("echo {code}: exit status 1"), "{error}");
}

#[test]
fn a_shell_that_did_not_start_or_was_killed_is_an_error() {
    let mut app = App::with_fixed_panels();
    finished(&mut app, ShellOutcome::NoExitCode);
    assert!(app
        .job_error
        .clone()
        .unwrap()
        .contains("without an exit status"));
    finished(
        &mut app,
        ShellOutcome::NotStarted {
            reason: "no such file".into(),
        },
    );
    let error = app.job_error.clone().unwrap();
    assert!(
        error.contains("did not start") && error.contains("no such file"),
        "{error}"
    );
}

#[test]
fn finishing_reloads_both_panels() {
    let mut app = App::with_fixed_panels();
    let before = (app.panels.left.request_id, app.panels.right.request_id);
    finished(&mut app, ShellOutcome::Exited(0));
    assert_ne!(app.panels.left.request_id, before.0);
    assert_ne!(app.panels.right.request_id, before.1);
}

#[test]
fn focusing_does_nothing_while_an_overlay_is_open() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::QuickOpen(QuickOpenMessage::Open)));
    drop(app.update(Message::CommandLine(CommandLineMessage::Focus)));
    assert!(!app.command_line_for_test().is_focused());
}

#[test]
fn an_overlay_that_opens_while_focused_takes_the_focus_away() {
    let mut app = focused_app();
    type_line(&mut app, "ls");
    drop(app.update(Message::VolumeMenu(PanelSide::Left)));
    assert!(app.panels.volume_menu.is_some());
    assert!(!app.command_line_for_test().is_focused());
    assert!(!app.prompt_key_state().command_line);
}

#[test]
fn f2_is_a_command_and_labels_the_second_slot() {
    assert!(command_of(KeyAction::CommandLine).is_some());
    let app = App::with_fixed_panels();
    assert_eq!(app.function_keys[1], Some("Cmd line"));
}
