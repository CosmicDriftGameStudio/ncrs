use super::*;

/// A point on the path in the status bar of a 1200 x 760 window, just right of
/// the mode field. The field is only as wide as its path, and a temporary path
/// is short on Linux, so the point stays near the field's start; End then puts
/// the caret at the end.
const PATH_LINE: Point = Point::new(120.0, 749.0);

/// Both panels in a scratch tree that holds a `lib` directory with two files.
fn path_session(
    label: &str,
) -> (
    tempfile::TempDir,
    Session<impl iced::Program<State = App, Message = Message> + 'static>,
) {
    let dir = scratch(label);
    write(&dir.path().join("lib").join("alpha.txt"), "a");
    write(&dir.path().join("lib").join("beta.txt"), "b");
    let session = session_in(dir.path());
    (dir, session)
}

/// What the user types after the panel's own path to end in `lib`. The
/// platform's separator: in a Windows `\\?\` path a `/` is no separator.
fn typed_lib() -> String {
    format!("{}lib", std::path::MAIN_SEPARATOR)
}

/// The scratch tree's `lib` as the app spells it (the canonical path).
fn lib_of(app: &App) -> std::path::PathBuf {
    app.active_panel().path.join("lib")
}

/// Clicks into the field and puts the caret at the end: the scratch path is
/// longer than the click point is far from the field's start.
fn click_into_the_path<P: iced::Program<State = App, Message = Message> + 'static>(
    session: &mut Session<P>,
) {
    session.click_at(PATH_LINE);
    session.press_in_widgets(Key::Named(keyboard::key::Named::End), Modifiers::default());
}

pub(in crate::e2e_tests) fn focus_the_window<
    P: iced::Program<State = App, Message = Message> + 'static,
>(
    session: &mut Session<P>,
) {
    session.interact(vec![
        iced_test::core::Event::Window(window::Event::Focused),
        iced_test::core::Event::Window(window::Event::RedrawRequested(
            iced_test::core::time::Instant::now(),
        )),
    ]);
}

#[test]
fn clicking_the_path_then_typing_and_enter_goes_to_that_directory() {
    let (_dir, mut session) = path_session("path-enter");
    let lib = lib_of(&session.app);
    assert_eq!(session.app.path_field_for_test(), None);

    click_into_the_path(&mut session);
    focus_the_window(&mut session);
    session.type_into_widgets(&typed_lib());
    assert_eq!(
        session.app.path_field_for_test().map(Path::new),
        Some(lib.as_path()),
        "typing did not reach the field"
    );

    session.press_in_widgets(
        Key::Named(keyboard::key::Named::Enter),
        Modifiers::default(),
    );
    session.settle();
    assert_eq!(session.app.active_panel().path, lib);
    assert_eq!(session.app.path_field_for_test(), None);

    // The field gave the focus back: a letter is type-ahead in the panel.
    session.type_into_widgets("b");
    assert_eq!(session.app.path_field_for_test(), None);
    assert_eq!(
        selected(&session.app, PanelSide::Left).as_deref(),
        Some("beta.txt")
    );
}

#[test]
fn pasting_into_the_path_field_with_the_command_key_works() {
    let (_dir, mut session) = path_session("path-paste");
    let lib = lib_of(&session.app);
    session.widgets.clipboard.content = Some(typed_lib());

    click_into_the_path(&mut session);
    // The operating system reports the held key first; the field reads it from
    // there rather than from the key press.
    session.interact(vec![iced_test::core::Event::Keyboard(
        keyboard::Event::ModifiersChanged(Modifiers::COMMAND),
    )]);
    session.press_in_widgets(Key::Character("v".into()), Modifiers::COMMAND);
    assert_eq!(
        session.app.path_field_for_test().map(Path::new),
        Some(lib.as_path())
    );
}

#[test]
fn keys_go_to_the_panels_until_the_path_is_clicked_and_back_after_escape() {
    let (_dir, mut session) = path_session("path-escape");
    // Not focused: a letter is type-ahead, not path text.
    session.type_text("l");
    assert_eq!(session.app.path_field_for_test(), None);

    click_into_the_path(&mut session);
    session.type_into_widgets(&typed_lib());
    assert!(session.app.path_field_for_test().is_some());

    session.press_in_widgets(
        Key::Named(keyboard::key::Named::Escape),
        Modifiers::default(),
    );
    assert_eq!(
        session.app.path_field_for_test(),
        None,
        "Escape restores the path"
    );
    session.type_into_widgets("l");
    assert_eq!(
        session.app.path_field_for_test(),
        None,
        "the field gave the focus back"
    );
}

#[test]
fn clicking_a_panel_after_editing_restores_the_path() {
    let (_dir, mut session) = path_session("path-click-away");
    click_into_the_path(&mut session);
    session.type_into_widgets(&typed_lib());
    session.click_at(Point::new(300.0, 300.0));
    assert_eq!(session.app.path_field_for_test(), None);
    session.type_into_widgets("l");
    assert_eq!(
        session.app.path_field_for_test(),
        None,
        "the field gave the focus back"
    );
}

#[test]
fn clicking_a_column_title_while_editing_the_path_ends_the_editing() {
    let (_dir, mut session) = path_session("path-sort-click");
    click_into_the_path(&mut session);
    session.type_into_widgets(&typed_lib());
    assert!(session.app.path_field_for_test().is_some());
    session.click_at(LEFT_SIZE_HEADER);
    assert_eq!(session.app.path_field_for_test(), None);
}

#[test]
fn a_click_into_the_path_field_with_a_context_menu_open_closes_the_menu() {
    let (_dir, mut session) = path_session("path-menu");
    session.press_with(Key::Named(keyboard::key::Named::F10), Modifiers::SHIFT);
    assert!(session.app.context_menu_for_test().is_some());
    session.click_at(PATH_LINE);
    assert!(session.app.context_menu_for_test().is_none());
}

const LEFT_NAME_HEADER: Point = Point::new(90.0, 45.0);
const LEFT_SIZE_HEADER: Point = Point::new(418.0, 45.0);
const LEFT_MODIFIED_HEADER: Point = Point::new(544.0, 45.0);

fn sorting_session(
    label: &str,
) -> (
    tempfile::TempDir,
    Session<impl iced::Program<State = App, Message = Message> + 'static>,
) {
    let dir = scratch(label);
    write(&dir.path().join("a.txt"), "a");
    write(&dir.path().join("b.txt"), &"b".repeat(300));
    write(&dir.path().join("c.txt"), &"c".repeat(20));
    let session = session_in(dir.path());
    (dir, session)
}

fn text_files(app: &App, side: PanelSide) -> Vec<String> {
    names(app, side)
        .into_iter()
        .filter(|name| name.ends_with(".txt"))
        .collect()
}

#[test]
fn clicking_a_column_title_sorts_that_panel_and_a_second_click_reverses_it() {
    let (_dir, mut session) = sorting_session("sort-click");
    assert_eq!(
        text_files(&session.app, PanelSide::Left),
        ["a.txt", "b.txt", "c.txt"]
    );

    session.click_at(LEFT_SIZE_HEADER);
    assert_eq!(
        text_files(&session.app, PanelSide::Left),
        ["a.txt", "c.txt", "b.txt"]
    );
    assert_eq!(names(&session.app, PanelSide::Left)[0], "..");

    session.click_at(LEFT_SIZE_HEADER);
    assert_eq!(
        text_files(&session.app, PanelSide::Left),
        ["b.txt", "c.txt", "a.txt"]
    );
    let left = names(&session.app, PanelSide::Left);
    assert_eq!(left[0], "..");
    assert!(
        left[1..4].iter().all(|name| !name.ends_with(".txt")),
        "directories stay before the files: {left:?}"
    );

    session.click_at(LEFT_NAME_HEADER);
    assert_eq!(
        text_files(&session.app, PanelSide::Left),
        ["a.txt", "b.txt", "c.txt"]
    );

    assert_eq!(
        session.app.panel(PanelSide::Right).sort,
        crate::fs::SortKey::default(),
        "the other panel is not sorted"
    );
}

#[test]
fn sorting_keeps_the_cursor_on_its_entry() {
    let (_dir, mut session) = sorting_session("sort-cursor");
    cursor_to(&mut session, "c.txt");

    session.click_at(LEFT_MODIFIED_HEADER);
    session.click_at(LEFT_SIZE_HEADER);

    assert_eq!(
        selected(&session.app, PanelSide::Left).as_deref(),
        Some("c.txt")
    );
}

#[test]
fn the_sort_keys_sort_the_active_panel_and_a_second_press_reverses() {
    use crate::fs::{SortColumn, SortKey};
    let (_dir, mut session) = sorting_session("sort-keys");
    let press_sort = |running: &mut Session<_>, column| {
        let binding = crate::keymap::sort_binding(column);
        running.press_with(binding.key, binding.modifiers);
    };
    let sort_of = |running: &Session<_>| running.app.panel(PanelSide::Left).sort;

    press_sort(&mut session, SortColumn::Size);
    assert_eq!(
        sort_of(&session),
        SortKey {
            column: SortColumn::Size,
            descending: false
        }
    );
    press_sort(&mut session, SortColumn::Size);
    assert_eq!(
        sort_of(&session),
        SortKey {
            column: SortColumn::Size,
            descending: true
        }
    );
    assert_eq!(
        text_files(&session.app, PanelSide::Left),
        ["b.txt", "c.txt", "a.txt"]
    );

    press_sort(&mut session, SortColumn::Modified);
    assert_eq!(sort_of(&session).column, SortColumn::Modified);
    press_sort(&mut session, SortColumn::Name);
    assert_eq!(sort_of(&session), SortKey::default());
    assert_eq!(session.app.panel(PanelSide::Right).sort, SortKey::default());
}
