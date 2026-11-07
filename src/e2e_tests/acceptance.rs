//! Acceptance of the permissions dialog, the panel columns, the font size,
//! the tab circle, paste and the settings keys: real keys and clicks on the
//! laid-out view, and every result read from the disk or from config.toml.

use std::sync::{Arc, Mutex};

use iced::keyboard::{key::Named, Key, Modifiers};
use iced_test::core::widget::operation::Focusable;
use iced_test::core::widget::{Id, Operation};
use iced_test::core::Rectangle;

use super::*;

/// Finds where the focusable widget with an id is on screen.
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

fn bounds_of<P: iced::Program<State = App, Message = Message> + 'static>(
    session: &mut Session<P>,
    id: &'static str,
) -> Rectangle {
    let found = Arc::new(Mutex::new(None));
    session.operate(Box::new(FindBounds {
        target: Id::new(id),
        found: Arc::clone(&found),
    }));
    let bounds = *found.lock().expect("the bounds slot");
    bounds.unwrap_or_else(|| panic!("{id} is not on screen"))
}

/// The chord that opens the permissions dialog.
fn permissions_chord() -> Modifiers {
    if cfg!(target_os = "macos") {
        Modifiers::ALT | Modifiers::COMMAND
    } else {
        Modifiers::CTRL | Modifiers::ALT
    }
}

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o7777
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
}

/// Opens the dialog on the row under the cursor with the real chord.
fn open_permissions<P: iced::Program<State = App, Message = Message> + 'static>(
    session: &mut Session<P>,
) {
    session.press_with(Key::Character("r".into()), permissions_chord());
    session.settle();
    assert!(
        session.app.permissions_dialog_is_open(),
        "the permissions chord opened no dialog"
    );
}

/// Types an octal number into the dialog's field and confirms with Enter.
fn type_octal_and_confirm<P: iced::Program<State = App, Message = Message> + 'static>(
    session: &mut Session<P>,
    octal: &str,
) {
    let field = bounds_of(session, ncrs_ops::permissions::view::OCTAL_FIELD_ID);
    session.click_at(field.center());
    for _ in 0..8 {
        session.press_in_widgets(Key::Named(Named::Backspace), Modifiers::default());
    }
    session.type_into_widgets(octal);
    session.press_in_widgets(Key::Named(Named::Enter), Modifiers::default());
    session.settle();
}

#[cfg(unix)]
#[test]
fn permissions_one_file_octal_640_lands_on_disk() {
    let dir = scratch("perm-one");
    let file = dir.path().join("alpha.txt");
    write(&file, "a");
    set_mode(&file, 0o644);

    let mut session = session_in(dir.path());
    cursor_to(&mut session, "alpha.txt");
    open_permissions(&mut session);
    type_octal_and_confirm(&mut session, "640");

    assert!(
        !session.app.permissions_dialog_is_open(),
        "Enter left the dialog up"
    );
    assert_eq!(mode_of(&file), 0o640);
    assert!(session.app.job_is_idle());
}

#[cfg(unix)]
#[test]
fn permissions_folder_contents_without_execute() {
    use ncrs_fs::permissions::{ForFiles, ForFolders};
    use ncrs_ops::PermissionsMessage as P;

    let dir = scratch("perm-contents");
    let top = dir.path().join("tree");
    write(&top.join("a.sh"), "a");
    write(&top.join("sub").join("b.sh"), "b");
    set_mode(&top.join("a.sh"), 0o755);
    set_mode(&top.join("sub").join("b.sh"), 0o755);
    set_mode(&top.join("sub"), 0o700);
    set_mode(&top, 0o700);

    let mut session = session_in(dir.path());
    cursor_to(&mut session, "tree");
    open_permissions(&mut session);
    for message in [
        P::OctalInput("750".into()),
        P::ToggleContents,
        P::SetForFiles(ForFiles::WithoutExecute),
        P::SetForFolders(ForFolders::AsAbove),
    ] {
        session.send(Message::Permissions(message));
    }
    session.press(Key::Named(Named::Enter));
    session.settle();

    assert_eq!(mode_of(&top), 0o750);
    assert_eq!(mode_of(&top.join("sub")), 0o750);
    assert_eq!(
        mode_of(&top.join("a.sh")) & 0o111,
        0,
        "a file kept an execute bit"
    );
    assert_eq!(mode_of(&top.join("sub").join("b.sh")) & 0o111, 0);
    assert!(session.app.job_is_idle(), "the job did not end");
}

#[cfg(unix)]
#[test]
fn permissions_a_000_folder_is_made_readable_again() {
    let dir = scratch("perm-000");
    let locked = dir.path().join("locked");
    std::fs::create_dir_all(&locked).unwrap();
    set_mode(&locked, 0o000);

    let mut session = session_in(dir.path());
    cursor_to(&mut session, "locked");
    open_permissions(&mut session);
    type_octal_and_confirm(&mut session, "755");

    let mode = mode_of(&locked);
    set_mode(&locked, 0o700);
    assert_eq!(mode, 0o755);
}

/// Every text on screen that `matches`, with where it is.
struct FindTexts<F: Fn(&str) -> bool + Send> {
    matches: F,
    found: Arc<Mutex<Vec<Rectangle>>>,
}

impl<F: Fn(&str) -> bool + Send> Operation for FindTexts<F> {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn text(&mut self, _id: Option<&Id>, bounds: Rectangle, text: &str) {
        if (self.matches)(text) {
            if let Ok(mut found) = self.found.lock() {
                found.push(bounds);
            }
        }
    }
}

fn texts_where<P: iced::Program<State = App, Message = Message> + 'static>(
    session: &mut Session<P>,
    matches: impl Fn(&str) -> bool + Send + 'static,
) -> Vec<Rectangle> {
    let found = Arc::new(Mutex::new(Vec::new()));
    session.operate(Box::new(FindTexts {
        matches,
        found: Arc::clone(&found),
    }));
    let all = found.lock().expect("the texts slot").clone();
    all
}

fn first_text<P: iced::Program<State = App, Message = Message> + 'static>(
    session: &mut Session<P>,
    title: &'static str,
) -> Rectangle {
    texts_where(session, move |text| text.starts_with(title))
        .first()
        .copied()
        .unwrap_or_else(|| panic!("{title:?} is not on screen"))
}

fn right_click_at<P: iced::Program<State = App, Message = Message> + 'static>(
    session: &mut Session<P>,
    point: iced_test::core::Point,
) {
    use iced_test::core::Event;
    session.interact(vec![
        Event::Mouse(mouse::Event::CursorMoved { position: point }),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
    ]);
    session.interact(vec![Event::Mouse(mouse::Event::ButtonReleased(
        mouse::Button::Right,
    ))]);
}

/// A session whose settings are read from and written to `config`.
fn session_with_config_file(
    dir: &Path,
    config_file: &Path,
) -> Session<impl iced::Program<State = App, Message = Message> + 'static> {
    let left = std::fs::canonicalize(dir).expect("a canonical scratch path");
    let right = left.join("home");
    let config = config_file.to_path_buf();
    Session::boot(crate::program(
        crate::ui::layout::INITIAL_WINDOW_SIZE,
        move || {
            let (app, task) = App::starting_in(left.clone(), right.clone());
            let configured = app
                .with_config(crate::config::load(&config))
                .with_config_path_for_test(config.clone());
            (configured, task)
        },
    ))
}

fn saved_columns(config: &Path) -> ncrs_config::panel_columns::PanelColumns {
    crate::config::load(config)
        .expect("config.toml loads")
        .columns
}

#[test]
fn the_header_menu_adds_the_type_column_and_it_survives_a_restart() {
    use ncrs_core::Column;
    let dir = scratch("columns-menu");
    let data = scratch("columns-menu-data");
    let config = data.path().join("config.toml");

    let mut session = session_with_config_file(dir.path(), &config);
    assert!(!session.app.columns_for_test().left.contains(&Column::Type));
    let name = first_text(&mut session, "Name");
    right_click_at(&mut session, name.center());
    let entry = first_text(&mut session, "Type");
    session.click_at(entry.center());
    session.settle();

    assert!(
        session.app.columns_for_test().left.contains(&Column::Type),
        "the menu entry did not add the column"
    );
    let on_disk = std::fs::read_to_string(&config).expect("config.toml was written");
    assert!(on_disk.contains("\"type\""), "config.toml: {on_disk}");
    assert!(saved_columns(&config).left.contains(&Column::Type));

    let mut restarted = session_with_config_file(dir.path(), &config);
    assert!(restarted
        .app
        .columns_for_test()
        .left
        .contains(&Column::Type));
    assert!(
        !texts_where(&mut restarted, |text| text.starts_with("Type")).is_empty(),
        "the restarted app shows no Type title"
    );
}

#[test]
fn dragging_the_edge_of_a_title_saves_the_width() {
    use iced_test::core::{Event, Point};
    use ncrs_core::Column;
    let dir = scratch("columns-resize");
    let data = scratch("columns-resize-data");
    let config = data.path().join("config.toml");

    let mut session = session_with_config_file(dir.path(), &config);
    let size = first_text(&mut session, "Size");
    let edge = Point::new(size.x + size.width - 2.0, size.center_y());
    session.interact(vec![
        Event::Mouse(mouse::Event::CursorMoved { position: edge }),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
    ]);
    session.interact(vec![Event::Mouse(mouse::Event::CursorMoved {
        position: Point::new(edge.x - 30.0, edge.y),
    })]);
    session.interact(vec![Event::Mouse(mouse::Event::ButtonReleased(
        mouse::Button::Left,
    ))]);
    session.settle();

    let widths = saved_columns(&config).widths;
    assert!(
        widths.iter().any(|(column, _)| *column == Column::Size),
        "config.toml holds no width for Size: {widths:?}"
    );
}

/// A frame is drawn: the key routing reads what the view last showed, as it does
/// in the running app, where a redraw follows every update.
fn redraw<P: iced::Program<State = App, Message = Message> + 'static>(session: &mut Session<P>) {
    focus_the_window(session);
}

fn open_settings<P: iced::Program<State = App, Message = Message> + 'static>(
    session: &mut Session<P>,
) {
    session.press_with(Key::Character(",".into()), Modifiers::COMMAND);
    session.settle();
    assert!(
        session.app.settings_for_test().is_some(),
        "Cmd+, opened no settings"
    );
    redraw(session);
}

fn press_times<P: iced::Program<State = App, Message = Message> + 'static>(
    session: &mut Session<P>,
    named: Named,
    times: usize,
) {
    for _ in 0..times {
        session.press(Key::Named(named));
    }
}

/// A window that has been told it has the focus, as every real one is; the
/// app ignores the keys of a window without it.
fn opened_settings<P: iced::Program<State = App, Message = Message> + 'static>(
    session: &mut Session<P>,
) {
    focus_the_window(session);
    open_settings(session);
}

#[test]
fn the_font_size_is_chosen_by_keyboard_and_saved() {
    let dir = scratch("font-size");
    let data = scratch("font-size-data");
    let config = data.path().join("config.toml");

    let mut session = session_with_config_file(dir.path(), &config);
    opened_settings(&mut session);
    let before = session.app.scale_factor();
    // Appearance is the first page; into its rows, down to the size row (the
    // fifth keyed row: look, VS Code theme, Open VSX, font, size).
    session.press(Key::Named(Named::ArrowRight));
    press_times(&mut session, Named::ArrowDown, 4);
    session.press(Key::Named(Named::Enter));
    session.press(Key::Named(Named::ArrowDown));
    session.press(Key::Named(Named::Enter));
    session.settle();

    let on_disk = std::fs::read_to_string(&config).expect("config.toml was written");
    assert!(
        on_disk.contains("[appearance]") && on_disk.contains("font_size = 10"),
        "config.toml: {on_disk}"
    );
    assert!(
        session.app.scale_factor() < before,
        "the scale did not follow: {before} -> {}",
        session.app.scale_factor()
    );
}

#[test]
fn tab_walks_the_circle_of_panels_settings_and_monitor_and_back() {
    use crate::messages::ShownTab;
    let dir = scratch("tab-circle");
    let mut session = session_in(dir.path());
    focus_the_window(&mut session);
    open_settings(&mut session);
    session.press(Key::Named(Named::F12));
    session.settle();
    redraw(&mut session);
    assert!(session.app.monitor_for_test().is_open(), "no monitor");
    assert_eq!(session.app.shown_tab_for_test(), ShownTab::Monitor);

    let stop = |shown: &Session<_>| {
        (
            shown.app.shown_tab_for_test(),
            shown.app.active_panel_side_for_test(),
        )
    };
    let forward = [
        (ShownTab::Panels, PanelSide::Left),
        (ShownTab::Panels, PanelSide::Right),
        (ShownTab::Settings, PanelSide::Right),
        (ShownTab::Monitor, PanelSide::Right),
    ];
    for expected in forward {
        session.press(Key::Named(Named::Tab));
        session.settle();
        assert_eq!(stop(&session).0, expected.0, "forward, wanted {expected:?}");
        if expected.0 == ShownTab::Panels {
            assert_eq!(stop(&session).1, expected.1);
        }
    }
    let backward = [
        (ShownTab::Settings, PanelSide::Right),
        (ShownTab::Panels, PanelSide::Right),
        (ShownTab::Panels, PanelSide::Left),
        (ShownTab::Monitor, PanelSide::Left),
    ];
    for expected in backward {
        session.press_with(Key::Named(Named::Tab), Modifiers::SHIFT);
        session.settle();
        assert_eq!(
            stop(&session).0,
            expected.0,
            "backward, wanted {expected:?}"
        );
        if expected.0 == ShownTab::Panels {
            assert_eq!(stop(&session).1, expected.1);
        }
    }
    assert!(session.app.settings_for_test().is_some(), "Settings closed");
    assert!(session.app.monitor_for_test().is_open(), "Monitor closed");

    // On the monitor tab the settings key shows the open settings tab, and
    // F12 on the settings tab shows the open monitor tab; neither closes.
    assert_eq!(session.app.shown_tab_for_test(), ShownTab::Monitor);
    session.press_with(Key::Character(",".into()), Modifiers::COMMAND);
    session.settle();
    assert_eq!(session.app.shown_tab_for_test(), ShownTab::Settings);
    session.press(Key::Named(Named::F12));
    session.settle();
    assert_eq!(session.app.shown_tab_for_test(), ShownTab::Monitor);
    assert!(session.app.settings_for_test().is_some(), "Settings closed");
    assert!(session.app.monitor_for_test().is_open(), "Monitor closed");
}

fn pasted_text() -> iced::Task<Option<crate::clipboard_paste::PastedContent>> {
    iced::Task::done(Some(crate::clipboard_paste::PastedContent::Text(
        "pasted words".to_owned(),
    )))
}

fn file_count(dir: &Path) -> usize {
    std::fs::read_dir(dir).map_or(0, Iterator::count)
}

#[test]
fn paste_in_the_command_line_makes_no_file_and_in_the_panel_it_does() {
    let dir = scratch("paste");
    let left = std::fs::canonicalize(dir.path()).expect("canonical");
    let mut session = {
        let (start_left, start_right) = (left.clone(), left.join("home"));
        Session::boot(crate::program(
            crate::ui::layout::INITIAL_WINDOW_SIZE,
            move || {
                let (app, task) = App::starting_in(start_left.clone(), start_right.clone());
                (app.with_clipboard_reader_for_test(pasted_text), task)
            },
        ))
    };
    focus_the_window(&mut session);
    let before = file_count(&left);

    let line = bounds_of(
        &mut session,
        ncrs_palette::command_line::view::COMMAND_LINE_ID,
    );
    session.click_at(line.center());
    redraw(&mut session);
    assert!(
        session.app.command_line_for_test().is_focused(),
        "the click did not focus the command line"
    );
    session.press_with(Key::Character("v".into()), Modifiers::COMMAND);
    session.settle();
    assert!(
        !session.app.prompt_is_open_for_test(),
        "paste in the command line asked for a file name"
    );
    assert_eq!(
        file_count(&left),
        before,
        "paste in the command line made a file"
    );

    session.press(Key::Named(Named::Escape));
    redraw(&mut session);
    assert!(!session.app.command_line_for_test().is_focused());
    session.press_with(Key::Character("v".into()), Modifiers::COMMAND);
    session.settle();
    assert!(
        session.app.prompt_is_open_for_test(),
        "paste in the panel asked for no file name"
    );
    session.press(Key::Named(Named::Enter));
    session.settle();
    let made: Vec<_> = std::fs::read_dir(&left)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "txt"))
        .collect();
    assert_eq!(
        made.len(),
        1,
        "paste in the panel made {} files",
        made.len()
    );
    assert_eq!(
        std::fs::read_to_string(made[0].path()).unwrap(),
        "pasted words"
    );
}

#[test]
fn the_settings_keys_toggle_the_type_column() {
    use ncrs_core::Column;
    let dir = scratch("settings-columns");
    let data = scratch("settings-columns-data");
    let config = data.path().join("config.toml");

    let mut session = session_with_config_file(dir.path(), &config);
    opened_settings(&mut session);
    for _ in 0..20 {
        if session.app.settings_for_test().map(|state| state.feature)
            == Some(ncrs_config::SettingsFeature::Columns)
        {
            break;
        }
        session.press(Key::Named(Named::ArrowDown));
    }
    session.press(Key::Named(Named::ArrowRight));
    // Rows: same in both, then the list: icon, name, size, modified, created, type.
    press_times(&mut session, Named::ArrowDown, 6);
    session.press(Key::Named(Named::Space));
    session.settle();

    let on_disk = std::fs::read_to_string(&config).expect("config.toml was written");
    assert!(on_disk.contains("\"type\""), "config.toml: {on_disk}");
    assert_eq!(
        saved_columns(&config).left,
        [
            Column::Icon,
            Column::Name,
            Column::Size,
            Column::Modified,
            Column::Type
        ]
    );
}

/// Where the focused settings row lies in the page, and how much of the page
/// shows: the row's top and bottom, the scroll, the window's height.
type RowAndScroll = (Option<Rectangle>, Option<(f32, f32, f32)>);

struct FindFocusedRow {
    found: Arc<Mutex<RowAndScroll>>,
}

impl Operation for FindFocusedRow {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if id == Some(&Id::new(ncrs_ui_kit::row_scroll::FOCUSED_ROW_ID)) {
            if let Ok(mut slot) = self.found.lock() {
                slot.0 = Some(bounds);
            }
        }
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        content_bounds: Rectangle,
        translation: iced::Vector,
        _state: &mut dyn iced_test::core::widget::operation::Scrollable,
    ) {
        if id == Some(&Id::new("settings-page-scroll")) {
            if let Ok(mut slot) = self.found.lock() {
                slot.1 = Some((content_bounds.y, translation.y, bounds.height));
            }
        }
    }
}

/// Walks down every row of a settings page and checks that the focused row
/// lies wholly in the window after each key. Returns whether the page scrolled.
fn walk_page_checking_the_row_shows(feature: ncrs_config::SettingsFeature) -> bool {
    let dir = scratch("settings-scroll");
    let mut session = session_in(dir.path());
    opened_settings(&mut session);
    for _ in 0..20 {
        if session.app.settings_for_test().map(|state| state.feature) == Some(feature) {
            break;
        }
        session.press(Key::Named(Named::ArrowDown));
    }
    session.press(Key::Named(Named::ArrowRight));
    assert_eq!(session.app.settings_for_test().unwrap().focus.row, 0);

    let mut scrolled_at_some_point = false;
    for step in 0..40 {
        let found = Arc::new(Mutex::new((None, None)));
        session.operate(Box::new(FindFocusedRow {
            found: Arc::clone(&found),
        }));
        let (row, scroll) = *found.lock().unwrap();
        let (row_bounds, (content_top, scrolled, height)) =
            (row.expect("focused row"), scroll.unwrap());
        let (top, bottom) = (
            row_bounds.y - content_top,
            row_bounds.y - content_top + row_bounds.height,
        );
        assert!(
            top >= scrolled && bottom <= scrolled + height,
            "{feature:?} step {step}: row {top}..{bottom} outside the window {scrolled}..{}",
            scrolled + height
        );
        scrolled_at_some_point |= scrolled > 0.0;
        session.press(Key::Named(Named::ArrowDown));
    }
    scrolled_at_some_point
}

#[test]
fn the_focused_settings_row_stays_wholly_in_view_on_long_pages() {
    use ncrs_config::SettingsFeature::{Columns, Hermes, Keys, Look, Programs};
    let scrolled: Vec<bool> = [Look, Keys, Programs, Columns, Hermes]
        .into_iter()
        .map(walk_page_checking_the_row_shows)
        .collect();
    assert!(
        scrolled.contains(&true),
        "no page was long enough to scroll: {scrolled:?}"
    );
}
