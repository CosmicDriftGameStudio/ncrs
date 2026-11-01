use crate::app::*;
use crate::context_menu::MenuEntry;

const LEFT: PanelSide = PanelSide::Left;
// Rows of the fixed left panel: `..`, a directory, two files.
const PARENT: usize = 0;
const DIRECTORY: usize = 1;
const FILE: usize = 2;

fn open_at(app: &mut App, side: PanelSide, index: usize) {
    drop(app.update(Message::ContextMenuAt { side, index }));
}

fn loaded(app: &App, side: PanelSide) -> Message {
    Message::Panel(PanelMessage::DirectoryLoaded {
        side,
        request_id: app.panel(side).request_id,
        path: app.panel(side).path.clone(),
        entries: Vec::new(),
        error: None,
        sorted_by: fs::SortKey::default(),
        select: None,
        archive: None,
    })
}

#[test]
fn a_reload_of_the_active_panel_closes_the_menu_and_the_other_ones_does_not() {
    let mut app = App::with_fixed_panels();
    open_at(&mut app, LEFT, FILE);
    assert!(app.context_menu_for_test().is_some());

    let other = loaded(&app, PanelSide::Right);
    drop(app.update(other));
    assert!(
        app.context_menu_for_test().is_some(),
        "the other side loaded"
    );

    let active = loaded(&app, LEFT);
    drop(app.update(active));
    assert!(app.context_menu_for_test().is_none(), "its rows are gone");
}

#[cfg(target_os = "macos")]
#[test]
fn a_control_click_on_another_row_replaces_the_menu() {
    let mut app = App::with_fixed_panels();
    open_at(&mut app, LEFT, FILE);
    let first = enabled_actions(&app);
    app.set_modifiers_for_test(Modifiers::CTRL);
    drop(app.update(Message::Panel(PanelMessage::RowClicked {
        side: LEFT,
        index: DIRECTORY,
        on_tag: false,
    })));
    assert!(app.context_menu_for_test().is_some(), "a new menu");
    let second = enabled_actions(&app);
    assert_ne!(first, second, "the directory's menu, not the file's");
}

#[test]
fn a_stale_reload_leaves_the_menu_open() {
    let mut app = App::with_fixed_panels();
    open_at(&mut app, LEFT, FILE);
    let mut stale = loaded(&app, LEFT);
    if let Message::Panel(PanelMessage::DirectoryLoaded { request_id, .. }) = &mut stale {
        *request_id = request_id.wrapping_add(1);
    }
    drop(app.update(stale));
    assert!(app.context_menu_for_test().is_some());
}

fn enabled_actions(app: &App) -> Vec<ContextAction> {
    app.context_menu_for_test()
        .unwrap()
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            MenuEntry::Item {
                action,
                enabled: true,
            } => Some(*action),
            _ => None,
        })
        .collect()
}

fn index_of(app: &App, wanted: ContextAction) -> usize {
    app.context_menu_for_test()
        .unwrap()
        .entries()
        .iter()
        .position(|entry| matches!(entry, MenuEntry::Item { action, .. } if *action == wanted))
        .unwrap()
}

fn route(menu_open: bool, named: Named, modifiers: Modifiers) -> Option<Message> {
    let state = PromptKeyState {
        context_menu: menu_open,
        ..PromptKeyState::default()
    };
    route_key(&state, Key::Named(named), modifiers)
}

#[test]
fn a_right_click_on_an_untagged_row_moves_the_cursor_and_the_pointer_places_the_menu() {
    let mut app = App::with_fixed_panels();
    app.set_pointer_for_test(Point::new(100.0, 120.0));
    app.panels.active = PanelSide::Right;
    open_at(&mut app, LEFT, FILE);

    assert!(app.context_menu_is_open());
    assert_eq!(app.active_panel_side_for_test(), LEFT);
    assert_eq!(app.panel(LEFT).selected, FILE);
    assert_eq!(
        app.context_menu_for_test().unwrap().origin(),
        Point::new(100.0, 120.0)
    );
}

#[test]
fn a_right_click_on_a_tagged_row_keeps_the_tags_and_acts_on_all_of_them() {
    let mut app = App::with_fixed_panels();
    app.toggle_tag_for_test(DIRECTORY);
    app.toggle_tag_for_test(3);
    open_at(&mut app, LEFT, 3);

    assert!(app.left_panel_selection_tagged_for_test(DIRECTORY));
    assert!(app.left_panel_selection_tagged_for_test(3));
    let trash = index_of(&app, ContextAction::Trash);
    drop(app.update(Message::ContextMenuClick(trash)));

    assert!(!app.context_menu_is_open());
    assert_eq!(app.ops.delete_dialog.as_ref().unwrap().sources.len(), 2);
}

#[test]
fn the_parent_row_offers_only_what_makes_sense() {
    let mut app = App::with_fixed_panels();
    open_at(&mut app, LEFT, PARENT);
    assert_eq!(
        enabled_actions(&app),
        [
            ContextAction::Open,
            ContextAction::NewFolder,
            ContextAction::CopyPath,
            ContextAction::CopyName,
            ContextAction::ComputeFolderSizes
        ]
    );
}

#[test]
fn a_right_click_outside_the_tags_drops_them_and_acts_on_the_clicked_row_only() {
    let mut app = App::with_fixed_panels();
    app.toggle_tag_for_test(DIRECTORY);
    app.toggle_tag_for_test(3);
    open_at(&mut app, LEFT, FILE);

    assert!(!app.left_panel_selection_tagged_for_test(DIRECTORY));
    assert!(!app.left_panel_selection_tagged_for_test(3));
    let trash = index_of(&app, ContextAction::Trash);
    drop(app.update(Message::ContextMenuClick(trash)));
    assert_eq!(app.ops.delete_dialog.as_ref().unwrap().sources.len(), 1);
}

#[test]
fn the_parent_row_outside_the_tags_offers_the_parent_actions_only() {
    let mut app = App::with_fixed_panels();
    app.toggle_tag_for_test(FILE);
    open_at(&mut app, LEFT, PARENT);
    assert!(!enabled_actions(&app).contains(&ContextAction::Copy));
    assert!(!enabled_actions(&app).contains(&ContextAction::Trash));
}

#[test]
fn the_keyboard_keeps_the_tags() {
    let mut app = App::with_fixed_panels();
    app.toggle_tag_for_test(DIRECTORY);
    app.panel_mut(LEFT).selected = FILE;
    drop(app.update(Message::ContextMenuKey));
    assert!(app.left_panel_selection_tagged_for_test(DIRECTORY));
}

#[test]
fn a_directory_cannot_be_viewed_or_edited_but_a_file_can() {
    let mut app = App::with_fixed_panels();
    open_at(&mut app, LEFT, DIRECTORY);
    let on_directory = enabled_actions(&app);
    assert!(!on_directory.contains(&ContextAction::View));
    assert!(!on_directory.contains(&ContextAction::Edit));
    assert!(on_directory.contains(&ContextAction::Copy));

    open_at(&mut app, LEFT, FILE);
    let on_file = enabled_actions(&app);
    assert!(on_file.contains(&ContextAction::View));
    assert!(on_file.contains(&ContextAction::Edit));
}

#[test]
fn an_empty_panel_still_offers_a_new_folder() {
    let mut app = App::with_fixed_panels();
    app.panel_mut(LEFT).entries.clear();
    open_at(&mut app, LEFT, 0);
    assert_eq!(enabled_actions(&app), [ContextAction::NewFolder]);
}

#[test]
fn enter_runs_the_highlighted_entry_after_closing_the_menu() {
    let mut app = App::with_fixed_panels();
    open_at(&mut app, LEFT, FILE);
    let copy_path = index_of(&app, ContextAction::CopyPath);
    drop(app.update(Message::ContextMenuHover(copy_path)));
    assert_eq!(
        app.context_menu_selected_action(),
        Some(ContextAction::CopyPath)
    );
    drop(app.update(Message::ContextMenuActivate));

    assert!(!app.context_menu_is_open());
    assert_eq!(app.notice.as_deref(), Some("Path copied"));
}

#[test]
fn an_entry_runs_the_same_message_as_its_key() {
    let mut app = App::with_fixed_panels();
    open_at(&mut app, LEFT, FILE);
    let new_folder = index_of(&app, ContextAction::NewFolder);
    drop(app.update(Message::ContextMenuClick(new_folder)));
    assert!(
        app.ops.prompt.is_some(),
        "F7 opens the create-directory prompt"
    );
}

#[test]
fn arrows_skip_what_cannot_be_chosen() {
    let mut app = App::with_fixed_panels();
    open_at(&mut app, LEFT, PARENT);
    // Open, then the next usable entry is the new folder: View, Edit, Copy
    // and Move are greyed out and the separator is not an entry.
    drop(app.update(Message::ContextMenuMove(1)));
    assert_eq!(
        app.context_menu_selected_action(),
        Some(ContextAction::NewFolder)
    );
    drop(app.update(Message::ContextMenuLast));
    assert_eq!(
        app.context_menu_selected_action(),
        Some(ContextAction::ComputeFolderSizes)
    );
    drop(app.update(Message::ContextMenuFirst));
    assert_eq!(
        app.context_menu_selected_action(),
        Some(ContextAction::Open)
    );
}

#[test]
fn the_keys_of_the_open_menu() {
    assert_eq!(
        route(true, Named::Escape, Modifiers::default()),
        Some(Message::ContextMenuClose)
    );
    assert_eq!(
        route(true, Named::Enter, Modifiers::default()),
        Some(Message::ContextMenuActivate)
    );
    assert_eq!(
        route(true, Named::ArrowDown, Modifiers::default()),
        Some(Message::ContextMenuMove(1))
    );
    assert_eq!(
        route(true, Named::ArrowUp, Modifiers::default()),
        Some(Message::ContextMenuMove(-1))
    );
    assert_eq!(
        route(true, Named::Home, Modifiers::default()),
        Some(Message::ContextMenuFirst)
    );
    assert_eq!(
        route(true, Named::End, Modifiers::default()),
        Some(Message::ContextMenuLast)
    );
    assert_eq!(route(true, Named::F5, Modifiers::default()), None);
    assert!(repeats(&Message::ContextMenuMove(1)));
    assert!(!repeats(&Message::ContextMenuActivate));
}

#[test]
fn shift_f10_opens_the_menu_and_does_not_quit() {
    assert_eq!(
        route(false, Named::F10, Modifiers::SHIFT),
        Some(Message::ContextMenuKey)
    );
    assert_eq!(
        route(false, Named::F10, Modifiers::default()),
        Some(Message::Quit)
    );
}

#[test]
fn escape_closes_and_the_cursor_stays() {
    let mut app = App::with_fixed_panels();
    open_at(&mut app, LEFT, FILE);
    drop(app.update(Message::ContextMenuClose));
    assert!(!app.context_menu_is_open());
    assert_eq!(app.panel(LEFT).selected, FILE);
}

#[test]
fn the_keyboard_opens_the_menu_at_the_cursor_row_of_the_active_panel() {
    let mut app = App::with_fixed_panels();
    app.panels.active = PanelSide::Right;
    app.panel_mut(PanelSide::Right).selected = 2;
    drop(app.update(Message::ContextMenuKey));

    let menu = app.context_menu_for_test().unwrap();
    let anchor = layout::cursor_row_anchor(true, 2, layout::ROW_HEIGHT, app.window_size);
    assert_eq!(menu.origin(), anchor);
    assert_eq!(app.active_panel_side_for_test(), PanelSide::Right);
    assert_eq!(app.panel(PanelSide::Right).selected, 2);
}

#[test]
fn the_keyboard_does_nothing_under_an_overlay_or_a_menu_that_is_already_up() {
    let mut prompt = App::with_prompt_open();
    drop(prompt.update(Message::ContextMenuKey));
    assert!(!prompt.context_menu_is_open());

    let mut delete = App::with_fixed_panels();
    delete.open_delete_dialog_for_test(1, false);
    drop(delete.update(Message::ContextMenuKey));
    assert!(!delete.context_menu_is_open());
    drop(delete.update(Message::ContextMenuAt {
        side: LEFT,
        index: FILE,
    }));
    assert!(!delete.context_menu_is_open());

    let mut open = App::with_fixed_panels();
    open_at(&mut open, LEFT, FILE);
    let before = open.context_menu_for_test().cloned();
    drop(open.update(Message::ContextMenuKey));
    assert_eq!(open.context_menu_for_test().cloned(), before);
}

#[test]
fn a_right_click_on_another_row_reopens_the_menu_there() {
    let mut app = App::with_fixed_panels();
    open_at(&mut app, LEFT, DIRECTORY);
    app.set_pointer_for_test(Point::new(300.0, 200.0));
    open_at(&mut app, PanelSide::Right, 1);

    assert!(app.context_menu_is_open());
    assert_eq!(app.active_panel_side_for_test(), PanelSide::Right);
    assert_eq!(app.panel(PanelSide::Right).selected, 1);
    assert_eq!(
        app.context_menu_for_test().unwrap().origin(),
        Point::new(300.0, 200.0)
    );
}

#[test]
fn a_left_click_on_a_row_only_closes_the_menu() {
    let mut app = App::with_fixed_panels();
    open_at(&mut app, LEFT, DIRECTORY);
    drop(app.update(Message::Panel(PanelMessage::RowClicked {
        side: LEFT,
        index: FILE,
        on_tag: false,
    })));
    assert!(!app.context_menu_is_open());
    assert_eq!(app.panel(LEFT).selected, DIRECTORY);
}

#[test]
fn control_click_is_a_right_click_on_a_mac_only() {
    assert_eq!(
        ncrs_files::ctx::is_context_click(Modifiers::CTRL),
        cfg!(target_os = "macos")
    );
    assert!(!ncrs_files::ctx::is_context_click(Modifiers::default()));
    assert!(!ncrs_files::ctx::is_context_click(Modifiers::SHIFT));

    let mut app = App::with_fixed_panels();
    app.set_modifiers_for_test(Modifiers::CTRL);
    drop(app.update(Message::Panel(PanelMessage::RowClicked {
        side: LEFT,
        index: FILE,
        on_tag: false,
    })));
    assert_eq!(app.context_menu_is_open(), cfg!(target_os = "macos"));
    assert_eq!(app.panel(LEFT).selected, FILE);
}

#[test]
fn the_panels_do_not_scroll_under_the_menu() {
    let wheel = Message::Panel(PanelMessage::PanelScrolled {
        side: LEFT,
        delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
    });
    let mut app = App::with_fixed_panels();
    app.set_visible_rows_for_test(2);
    drop(app.update(wheel.clone()));
    assert_eq!(app.panel(LEFT).scroll_offset, 1, "the wheel scrolls");

    let mut covered = App::with_fixed_panels();
    covered.set_visible_rows_for_test(2);
    open_at(&mut covered, LEFT, 0);
    drop(covered.update(wheel));
    assert_eq!(covered.panel(LEFT).scroll_offset, 0);
}

#[test]
fn the_menu_is_an_overlay() {
    let mut app = App::with_fixed_panels();
    assert!(!app.overlay_is_open());
    open_at(&mut app, LEFT, FILE);
    assert!(app.overlay_is_open());
    assert!(app.prompt_key_state().context_menu);
    drop(app.update(Message::VolumeMenu(LEFT)));
    assert!(!app.volume_menu_is_open());
}
