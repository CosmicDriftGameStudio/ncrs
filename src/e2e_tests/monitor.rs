//! The System page, driven the way a person drives it: real keys, real
//! clicks on the laid-out view, and the machine's own measurement.

#[cfg(all(unix, not(feature = "sandbox")))]
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
#[cfg(all(unix, not(feature = "sandbox")))]
use std::time::{Duration, Instant};

use iced::keyboard::{key::Named, Key, Modifiers};
use iced_test::core::widget::operation::Focusable;
use iced_test::core::widget::{Id, Operation};
use iced_test::core::{Point, Rectangle};
use ncrs_monitor::{MonitorMessage, TaskColumn, FILTER_ID};

use super::*;

/// Finds where the filter field is on screen.
struct FindBounds {
    target: Id,
    found: Arc<Mutex<Option<Rectangle>>>,
}

impl Operation for FindBounds {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn focusable(&mut self, id: Option<&Id>, bounds: Rectangle, _state: &mut dyn Focusable) {
        if id == Some(&self.target) {
            if let Ok(mut slot) = self.found.lock() {
                *slot = Some(bounds);
            }
        }
    }
}

fn filter_bounds<P: iced::Program<State = App, Message = Message> + 'static>(
    session: &mut Session<P>,
) -> Rectangle {
    let found = Arc::new(Mutex::new(None));
    session.operate(Box::new(FindBounds {
        target: Id::new(FILTER_ID),
        found: Arc::clone(&found),
    }));
    let bounds = *found.lock().expect("the bounds slot");
    bounds.expect("the filter field is on the page")
}

fn f12<P: iced::Program<State = App, Message = Message> + 'static>(session: &mut Session<P>) {
    session.press(Key::Named(Named::F12));
}

/// The page open with a measurement on it.
fn open_page(
    label: &str,
) -> (
    tempfile::TempDir,
    Session<impl iced::Program<State = App, Message = Message> + 'static>,
) {
    let dir = scratch(label);
    let mut session = session_in(dir.path());
    f12(&mut session);
    session.settle();
    assert!(
        session.app.monitor_for_test().is_open(),
        "F12 did not open the page"
    );
    assert!(
        session.app.monitor_for_test().snapshot().is_some(),
        "the measurement did not arrive"
    );
    (dir, session)
}

fn focus_filter_and_type<P: iced::Program<State = App, Message = Message> + 'static>(
    session: &mut Session<P>,
    text: &str,
) -> Rectangle {
    let bounds = filter_bounds(session);
    session.click_at(bounds.center());
    session.type_into_widgets(text);
    assert_eq!(
        session.app.monitor_for_test().filter_text(),
        text,
        "typing did not reach the filter"
    );
    assert!(session.app.monitor_for_test().filter_has_focus());
    bounds
}

#[test]
fn f12_opens_escape_closes_and_the_letters_sort() {
    let (_dir, mut session) = open_page("monitor-keys");
    assert_eq!(session.app.monitor_for_test().sorted_by(), TaskColumn::Cpu);

    session.press(Key::Character("m".into()));
    assert_eq!(
        session.app.monitor_for_test().sorted_by(),
        TaskColumn::Memory
    );
    session.press(Key::Character("t".into()));
    assert_eq!(session.app.monitor_for_test().sorted_by(), TaskColumn::Time);
    session.press(Key::Character("c".into()));
    assert_eq!(session.app.monitor_for_test().sorted_by(), TaskColumn::Cpu);

    session.press(Key::Named(Named::Escape));
    assert!(!session.app.monitor_for_test().is_open());
    f12(&mut session);
    session.settle();
    assert!(session.app.monitor_for_test().is_open());
    f12(&mut session);
    assert!(
        !session.app.monitor_for_test().is_open(),
        "F12 on the page closes it"
    );
}

#[cfg(not(feature = "sandbox"))]
#[test]
fn the_arrows_move_the_row_and_scroll_the_command() {
    let (_dir, mut session) = open_page("monitor-arrows");
    session.press(Key::Named(Named::ArrowDown));
    let first = session.app.monitor_for_test().pid_under_cursor();
    assert!(first.is_some(), "Down put the cursor on no task");
    assert_eq!(session.app.monitor_for_test().cursor_index, 1);
    session.press(Key::Named(Named::ArrowDown));
    let second = session.app.monitor_for_test().pid_under_cursor();
    assert_ne!(second, first);
    session.press(Key::Named(Named::ArrowUp));
    assert_eq!(session.app.monitor_for_test().pid_under_cursor(), first);

    assert_eq!(session.app.monitor_for_test().command_offset, 0);
    session.press(Key::Named(Named::ArrowRight));
    assert!(
        session.app.monitor_for_test().command_offset > 0,
        "Right did not move the command"
    );
    session.press(Key::Named(Named::ArrowLeft));
    assert_eq!(session.app.monitor_for_test().command_offset, 0);
}

#[test]
fn a_click_beside_the_filter_gives_the_keys_back_and_escape_closes_once() {
    let (_dir, mut session) = open_page("monitor-click-away");
    let bounds = focus_filter_and_type(&mut session, "x");

    session.click_at(Point::new(bounds.x - 40.0, bounds.center_y()));
    assert!(
        !session.app.monitor_for_test().filter_has_focus(),
        "the click took the focus from the field, the page still thinks it has it"
    );

    session.press(Key::Character("m".into()));
    assert_eq!(
        session.app.monitor_for_test().sorted_by(),
        TaskColumn::Memory
    );

    session.press(Key::Named(Named::Escape));
    assert!(!session.app.monitor_for_test().is_open());
    assert_eq!(
        session.app.monitor_for_test().filter_text(),
        "x",
        "Escape was taken twice: once to clear the filter, once to close"
    );
}

#[test]
fn a_click_on_a_column_title_gives_the_keys_back() {
    let (_dir, mut session) = open_page("monitor-click-title");
    let bounds = focus_filter_and_type(&mut session, "x");

    let title = Point::new(
        bounds.center_x(),
        bounds.y
            + bounds.height
            + ncrs_ui_kit::theme::spacing::M
            + ncrs_ui_kit::layout::ROW_HEIGHT / 2.0,
    );
    session.click_at(title);
    assert_eq!(
        session.app.monitor_for_test().sorted_by(),
        TaskColumn::Command,
        "the click did not land on the title of the command column"
    );
    assert!(!session.app.monitor_for_test().filter_has_focus());

    session.press(Key::Character("c".into()));
    assert_eq!(session.app.monitor_for_test().sorted_by(), TaskColumn::Cpu);
    session.press(Key::Named(Named::Escape));
    assert!(!session.app.monitor_for_test().is_open());
}

#[test]
fn escape_with_the_filter_in_use_clears_it_then_closes() {
    let (_dir, mut session) = open_page("monitor-escape-order");
    focus_filter_and_type(&mut session, "x");
    session.press_in_widgets(Key::Named(Named::Escape), Modifiers::default());
    assert!(session.app.monitor_for_test().is_open());
    assert_eq!(session.app.monitor_for_test().filter_text(), "");
    assert!(!session.app.monitor_for_test().filter_has_focus());
    session.press(Key::Named(Named::Escape));
    assert!(!session.app.monitor_for_test().is_open());
}

#[test]
fn escape_over_the_command_list_closes_only_the_command_list() {
    let (_dir, mut session) = open_page("monitor-palette");
    focus_filter_and_type(&mut session, "x");

    session.press(Key::Named(Named::F1));
    assert!(session.app.command_palette_for_test().is_some());

    session.press(Key::Named(Named::Escape));
    assert!(session.app.command_palette_for_test().is_none());
    assert!(
        session.app.monitor_for_test().is_open(),
        "the page went with the command list"
    );
    assert_eq!(session.app.monitor_for_test().filter_text(), "x");
}

/// A child process of the test, killed when the test ends.
#[cfg(all(unix, not(feature = "sandbox")))]
struct Sleeper(Child);

#[cfg(all(unix, not(feature = "sandbox")))]
impl Sleeper {
    fn start(seconds: &str) -> Self {
        Self(
            Command::new("sleep")
                .arg(seconds)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("the sleep command"),
        )
    }
}

#[cfg(all(unix, not(feature = "sandbox")))]
impl Drop for Sleeper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(all(unix, not(feature = "sandbox")))]
fn wait_until_exited(child: &mut Child) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Ok(Some(status)) = child.try_wait() {
            return Some(status);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

#[cfg(all(unix, not(feature = "sandbox")))]
#[test]
fn kill_ends_the_chosen_task_and_only_that_one() {
    let mut chosen = Sleeper::start("7001");
    let mut other = Sleeper::start("7002");
    let (_dir, mut session) = open_page("monitor-kill");
    session.send(Message::Monitor(MonitorMessage::SelectRow(chosen.0.id())));
    assert_eq!(
        session.app.monitor_for_test().pid_under_cursor(),
        Some(chosen.0.id())
    );

    session.press(Key::Character("k".into()));
    assert!(
        session.app.monitor_for_test().dialog_is_open(),
        "k asked nothing"
    );
    // The dialog starts on Cancel; Tab moves to the button that kills.
    session.press(Key::Named(Named::Tab));
    session.press(Key::Named(Named::Enter));
    session.settle();

    assert!(
        wait_until_exited(&mut chosen.0).is_some(),
        "the chosen task is still running"
    );
    assert!(
        matches!(other.0.try_wait(), Ok(None)),
        "the other task was ended as well"
    );
}

#[cfg(all(unix, not(feature = "sandbox")))]
#[test]
fn copy_command_line_puts_the_whole_line_on_the_clipboard() {
    let task = Sleeper::start("7003");
    let (_dir, mut session) = open_page("monitor-copy");
    session.send(Message::Monitor(MonitorMessage::OpenMenu(task.0.id())));
    assert!(session.app.monitor_for_test().menu_is_open());
    // Kill, then Copy command line.
    session.press(Key::Named(Named::ArrowDown));
    session.press(Key::Named(Named::Enter));
    session.settle();

    let pasted = session
        .widgets
        .clipboard
        .content
        .clone()
        .unwrap_or_default();
    assert!(
        pasted.ends_with("sleep 7003"),
        "the clipboard holds {pasted:?}, not the command with its argument"
    );
}

#[test]
fn a_click_on_a_drive_shows_its_mount_point_in_the_panel() {
    let (dir, mut session) = open_page("monitor-drive");
    let mount = std::fs::canonicalize(dir.path().join("files")).expect("the files folder");
    session.send(Message::Monitor(MonitorMessage::OpenDrive(mount.clone())));
    assert_eq!(session.app.active_panel().path, mount);
    assert!(
        !session.app.monitor_for_test().is_open(),
        "the page stayed open"
    );
}

#[test]
fn a_changed_system_setting_is_written_and_read_back() {
    use crate::messages::{SettingChange, SettingsFeature, SettingsMessage};
    use ncrs_config::system_monitor::{RefreshEvery, StatusValue};

    let dir = scratch("monitor-settings");
    let file = dir.path().join("config.toml");
    std::fs::write(&file, "").expect("an empty config");
    let left = std::fs::canonicalize(dir.path()).expect("a canonical scratch path");
    let right = left.join("home");
    let config_file = file.clone();
    let mut session = Session::boot(crate::program(
        crate::ui::layout::INITIAL_WINDOW_SIZE,
        move || {
            let (app, task) = App::starting_in(left.clone(), right.clone());
            (app.with_config_path_for_test(config_file.clone()), task)
        },
    ));

    session.send(Message::Settings(SettingsMessage::Toggle));
    session.send(Message::Settings(SettingsMessage::Select(
        SettingsFeature::System,
    )));
    let toggled = session
        .app
        .monitor_for_test()
        .settings()
        .clone()
        .toggled(StatusValue::Memory, true);
    let changed = ncrs_config::system_monitor::SystemSettings {
        refresh: RefreshEvery::FiveSeconds,
        ..toggled
    };
    session.send(Message::Settings(SettingsMessage::Change(
        SettingChange::System(changed.clone()),
    )));

    let text = std::fs::read_to_string(&file).expect("the config file");
    assert!(text.contains("[system]"), "{text}");
    assert!(text.contains("refresh = \"5s\""), "{text}");
    assert_eq!(session.app.monitor_for_test().settings(), &changed);
    let reloaded = crate::config::load(&file).expect("the file loads again");
    assert_eq!(reloaded.system, changed);
}

fn tap_option<P: iced::Program<State = App, Message = Message> + 'static>(
    session: &mut Session<P>,
) {
    session.press_raw(keyboard::Event::ModifiersChanged(Modifiers::ALT));
    session.press_raw(keyboard::Event::ModifiersChanged(Modifiers::empty()));
}

#[cfg(not(feature = "sandbox"))]
#[test]
fn an_option_tap_opens_the_menu_on_the_first_row_and_the_choice_is_copied() {
    let (_dir, mut session) = open_page("monitor-tap");
    tap_option(&mut session);
    assert!(session.app.monitor_for_test().menu_is_open(), "no menu");
    assert!(
        !session.app.context_menu_is_open(),
        "the panel under the page opened a menu"
    );
    let first = session
        .app
        .monitor_for_test()
        .pid_under_cursor()
        .expect("the first row has the cursor");
    // Kill is the first entry when it is offered; Copy command line is next.
    if ncrs_monitor::KILL_OFFERED {
        session.press(Key::Named(Named::ArrowDown));
    }
    session.press(Key::Named(Named::Enter));
    session.settle();
    let expected = session
        .app
        .monitor_for_test()
        .snapshot()
        .and_then(|snapshot| snapshot.tasks.iter().find(|task| task.pid == first))
        .map(|task| task.command.clone())
        .expect("the task of the first row");
    assert_eq!(
        session
            .widgets
            .clipboard
            .content
            .clone()
            .unwrap_or_default(),
        expected
    );
}

#[cfg(not(feature = "sandbox"))]
#[test]
fn a_second_option_tap_closes_the_menu() {
    let (_dir, mut session) = open_page("monitor-tap-twice");
    tap_option(&mut session);
    assert!(session.app.monitor_for_test().menu_is_open());
    tap_option(&mut session);
    assert!(!session.app.monitor_for_test().menu_is_open());
}

#[test]
fn an_option_tap_with_the_filter_in_use_opens_nothing() {
    let (_dir, mut session) = open_page("monitor-tap-filter");
    focus_filter_and_type(&mut session, "x");
    tap_option(&mut session);
    assert!(!session.app.monitor_for_test().menu_is_open());
    assert!(!session.app.context_menu_is_open());
}

#[test]
fn an_option_tap_over_the_command_list_opens_nothing() {
    let (_dir, mut session) = open_page("monitor-tap-palette");
    session.press(Key::Named(Named::F1));
    assert!(session.app.command_palette_for_test().is_some());
    tap_option(&mut session);
    assert!(!session.app.monitor_for_test().menu_is_open());
    assert!(session.app.command_palette_for_test().is_some());
}

#[cfg(not(feature = "sandbox"))]
#[test]
fn shift_f10_right_after_f12_opens_the_menu_on_the_first_row() {
    let (_dir, mut session) = open_page("monitor-shift-f10");
    session.press_with(Key::Named(Named::F10), Modifiers::SHIFT);
    assert!(session.app.monitor_for_test().menu_is_open());
    assert!(!session.app.context_menu_is_open());
}

#[cfg(all(unix, not(feature = "sandbox")))]
#[test]
fn grouped_there_is_no_kill_and_ungrouped_it_hits_only_the_chosen_task() {
    let mut chosen = Sleeper::start("7101");
    let mut other = Sleeper::start("7102");
    let (_dir, mut session) = open_page("monitor-group");
    focus_filter_and_type(&mut session, "sleep 710");
    session.press(Key::Named(Named::Enter));
    assert!(!session.app.monitor_for_test().filter_has_focus());

    session.press(Key::Character("g".into()));
    let monitor = session.app.monitor_for_test();
    assert!(monitor.is_grouped());
    let sleeps = monitor
        .groups
        .iter()
        .find(|group| group.name == "sleep")
        .expect("the sleeps are a row");
    assert!(sleeps.count() >= 2, "the two sleeps are one row");
    let representative = sleeps.row.pid;
    let expected = monitor
        .snapshot()
        .and_then(|snapshot| {
            snapshot
                .tasks
                .iter()
                .find(|task| task.pid == representative)
        })
        .map(|task| task.command.clone())
        .expect("the task that stands for the group");
    session.send(Message::Monitor(MonitorMessage::SelectRow(representative)));

    session.press(Key::Character("k".into()));
    assert!(
        !session.app.monitor_for_test().dialog_is_open(),
        "k asked while grouped"
    );

    // With no Kill entry the first entry is Copy command line.
    session.press_with(Key::Named(Named::F10), Modifiers::SHIFT);
    assert!(session.app.monitor_for_test().menu_is_open());
    session.press(Key::Named(Named::Enter));
    session.settle();
    assert!(
        !session.app.monitor_for_test().dialog_is_open(),
        "the menu of a group asked to kill"
    );
    let pasted = session
        .widgets
        .clipboard
        .content
        .clone()
        .unwrap_or_default();
    assert_eq!(pasted, expected, "not the line of the smallest pid");

    session.press(Key::Character("g".into()));
    assert!(!session.app.monitor_for_test().is_grouped());
    session.send(Message::Monitor(MonitorMessage::SelectRow(chosen.0.id())));
    session.press(Key::Character("k".into()));
    assert!(session.app.monitor_for_test().dialog_is_open());
    session.press(Key::Named(Named::Tab));
    session.press(Key::Named(Named::Enter));
    session.settle();
    assert!(
        wait_until_exited(&mut chosen.0).is_some(),
        "the chosen task is still running"
    );
    assert!(
        matches!(other.0.try_wait(), Ok(None)),
        "the other task was ended as well"
    );
}

#[test]
fn the_page_opens_with_the_cursor_on_row_zero_and_a_tick_does_not_move_it() {
    let (_dir, mut session) = open_page("monitor-row-zero");
    let monitor = session.app.monitor_for_test();
    assert_eq!(monitor.cursor_index, 0);
    assert_eq!(monitor.window.offset(monitor.order.len(), 1), 0);

    session.send(Message::Monitor(MonitorMessage::Tick));
    session.settle();
    let measured = session.app.monitor_for_test();
    assert!(measured.snapshot().is_some());
    assert_eq!(measured.cursor_index, 0, "a measurement moved the cursor");
    assert_eq!(
        measured.window.offset(measured.order.len(), 1),
        0,
        "a measurement scrolled the list"
    );
}
