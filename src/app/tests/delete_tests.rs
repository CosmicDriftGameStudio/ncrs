use crate::app::*;
use iced::keyboard::key::Named;

fn keys(delete_dialog: DeleteKeys) -> PromptKeyState {
    PromptKeyState {
        delete_dialog,
        ..PromptKeyState::default()
    }
}

fn route(state: &PromptKeyState, named: Named, modifiers: Modifiers) -> Option<Message> {
    route_key(state, Key::Named(named), modifiers)
}

fn review_page(sidebar: bool, editing: bool) -> PromptKeyState {
    PromptKeyState {
        review_list: true,
        review_list_editing: editing,
        hermes_sidebar: sidebar,
        ..PromptKeyState::default()
    }
}

#[test]
fn left_in_a_hermes_page_moves_to_the_sidebar() {
    assert_eq!(
        route(
            &review_page(false, false),
            Named::ArrowLeft,
            Modifiers::default()
        ),
        Some(Message::HermesFocus(HermesFocus::Sidebar))
    );
}

#[test]
fn up_and_down_in_the_sidebar_switch_the_page() {
    let sidebar = review_page(true, false);
    assert_eq!(
        route(&sidebar, Named::ArrowUp, Modifiers::default()),
        Some(Message::HermesStep(-1))
    );
    assert_eq!(
        route(&sidebar, Named::ArrowDown, Modifiers::default()),
        Some(Message::HermesStep(1))
    );
}

#[test]
fn right_in_the_sidebar_goes_back_into_the_page() {
    assert_eq!(
        route(
            &review_page(true, false),
            Named::ArrowRight,
            Modifiers::default()
        ),
        Some(Message::HermesFocus(HermesFocus::Content))
    );
}

#[test]
fn up_in_the_page_still_moves_in_the_list() {
    assert_eq!(
        route(
            &review_page(false, false),
            Named::ArrowUp,
            Modifiers::default()
        ),
        Some(Message::ReviewList(ReviewListMessage::Move(-1)))
    );
}

#[test]
fn an_open_form_keeps_the_arrows_for_its_fields() {
    assert_eq!(
        route(
            &review_page(false, true),
            Named::ArrowLeft,
            Modifiers::default()
        ),
        None
    );
}

#[test]
fn f8_and_shift_f8_are_different_keys() {
    let idle = PromptKeyState::default();
    assert_eq!(
        route(&idle, Named::F8, Modifiers::default()),
        Some(Message::Delete { permanent: false })
    );
    assert_eq!(
        route(&idle, Named::F8, Modifiers::SHIFT),
        Some(Message::Delete { permanent: true })
    );
}

#[test]
fn enter_presses_the_focused_button_and_escape_cancels() {
    let state = keys(DeleteKeys::Trash);
    assert_eq!(
        route(&state, Named::Enter, Modifiers::default()),
        Some(Message::DialogActivate)
    );
    assert_eq!(
        route(&state, Named::Escape, Modifiers::default()),
        Some(Message::DeleteCancel)
    );
    // Nothing else reaches the panels behind the dialog.
    assert_eq!(route(&state, Named::ArrowDown, Modifiers::default()), None);
}

#[test]
fn the_permanent_dialog_still_confirms_on_a_second_shift_f8() {
    let state = keys(DeleteKeys::Permanent);
    assert_eq!(
        route(&state, Named::F8, Modifiers::SHIFT),
        Some(Message::DeleteConfirm)
    );
    assert_eq!(route(&state, Named::F8, Modifiers::default()), None);
}

#[test]
fn arrows_and_tab_move_the_focus_in_both_dialogs() {
    let conflict = PromptKeyState {
        conflict_open: true,
        ..PromptKeyState::default()
    };
    for state in [
        keys(DeleteKeys::Trash),
        keys(DeleteKeys::Permanent),
        conflict,
    ] {
        assert_eq!(
            route(&state, Named::ArrowRight, Modifiers::default()),
            Some(Message::DialogFocus(1))
        );
        assert_eq!(
            route(&state, Named::ArrowLeft, Modifiers::default()),
            Some(Message::DialogFocus(-1))
        );
        assert_eq!(
            route(&state, Named::Tab, Modifiers::default()),
            Some(Message::DialogFocus(1))
        );
        assert_eq!(
            route(&state, Named::Tab, Modifiers::SHIFT),
            Some(Message::DialogFocus(-1))
        );
    }
}

#[test]
fn space_and_shift_arrows_tag() {
    let idle = PromptKeyState::default();
    assert_eq!(
        route(&idle, Named::Space, Modifiers::default()),
        Some(Message::Panel(PanelMessage::ToggleTag))
    );
    assert_eq!(
        route(&idle, Named::ArrowDown, Modifiers::SHIFT),
        Some(Message::Panel(PanelMessage::TagMove(1)))
    );
    assert_eq!(
        route(&idle, Named::ArrowUp, Modifiers::SHIFT),
        Some(Message::Panel(PanelMessage::TagMove(-1)))
    );
    assert_eq!(
        route(&idle, Named::ArrowDown, Modifiers::default()),
        Some(Message::Panel(PanelMessage::MoveSelection(1)))
    );
}

#[test]
fn escape_leaves_a_filing_card_that_may_be_left() {
    let card = PromptKeyState {
        filing_card: true,
        ..PromptKeyState::default()
    };
    assert_eq!(
        route(&card, Named::Escape, Modifiers::default()),
        Some(Message::Filing(FilingMessage::Dismiss))
    );
    assert_eq!(
        route(
            &PromptKeyState::default(),
            Named::Escape,
            Modifiers::default()
        ),
        None
    );
}

#[test]
fn escape_closes_the_filing_setup_and_leaves_a_running_job_alone() {
    let setup_over_job = PromptKeyState {
        filing_setup: true,
        job_running: true,
        ..PromptKeyState::default()
    };
    assert_eq!(
        route(&setup_over_job, Named::Escape, Modifiers::default()),
        Some(Message::FilingSetup(FilingSetupMessage::Close))
    );
}

#[test]
fn a_held_enter_does_not_press_a_button() {
    assert!(!repeats(&Message::DialogActivate));
    assert!(!repeats(&Message::DialogFocus(1)));
}

#[tokio::test]
async fn focus_wraps_and_enter_presses_the_focused_button() {
    let mut app = app_with_entries(&["a.txt"]);
    let _open = app.update(Message::Delete { permanent: true });
    assert_eq!(
        app.dialog_focus,
        delete_dialog_view::CANCEL_FOCUS,
        "the permanent dialog must start on Cancel"
    );

    let _wrap = app.update(Message::DialogFocus(1));
    assert_eq!(app.dialog_focus, delete_dialog_view::CONFIRM_FOCUS);
    let _back = app.update(Message::DialogFocus(-1));
    assert_eq!(app.dialog_focus, delete_dialog_view::CANCEL_FOCUS);
    let _cancel = app.update(Message::DialogActivate);
    assert!(
        app.ops.delete_dialog.is_none(),
        "Enter on Cancel kept the dialog"
    );

    let _again = app.update(Message::Delete { permanent: false });
    assert_eq!(
        app.dialog_focus,
        delete_dialog_view::CONFIRM_FOCUS,
        "the trash dialog starts on Delete"
    );
    let _move = app.update(Message::DialogFocus(1));
    let _enter = app.update(Message::DialogActivate);
    assert!(app.ops.delete_dialog.is_none());
    assert!(
        app.ops.deleting.is_none(),
        "Enter on Cancel started a delete"
    );
}

#[tokio::test]
async fn a_command_click_tags_the_row() {
    let mut app = app_with_entries(&["a.txt", "b.txt"]);
    let click = |target: &mut App| {
        let _task = target.update(Message::Panel(PanelMessage::RowClicked {
            side: PanelSide::Left,
            index: 1,
            on_tag: false,
        }));
    };
    click(&mut app);
    assert!(
        !app.panels.left.selection.any_tagged(),
        "a plain click tagged"
    );

    let _held = app.update(Message::ModifiersChanged(Modifiers::COMMAND));
    click(&mut app);
    assert!(
        app.panels.left.selection.any_tagged(),
        "Cmd-click did not tag"
    );
}

#[tokio::test]
async fn space_tags_and_moves_down_but_skips_the_parent_entry() {
    let mut app = app_with_entries(&["..", "a.txt", "b.txt"]);
    app.panels.left.selected = 0;
    let _parent = app.update(Message::Panel(PanelMessage::ToggleTag));
    assert!(!app.panels.left.selection.any_tagged(), "`..` was tagged");
    assert_eq!(app.panels.left.selected, 1);

    let _tag = app.update(Message::Panel(PanelMessage::ToggleTag));
    assert_eq!(app.panels.left.selected, 2);
    let _up = app.update(Message::Panel(PanelMessage::TagMove(-1)));
    assert_eq!(app.panels.left.selected, 1);
    assert_eq!(
        app.panels
            .left
            .selection
            .ordered(&app.panels.left.entries)
            .len(),
        2
    );
}

fn app_with_entries(names: &[&str]) -> App {
    let mut app = App::with_environment(Environment::isolated()).0;
    app.panels.left.path = PathBuf::from("/left");
    app.panels.left.entries = names
        .iter()
        .map(|name| fs::FileEntry {
            name: (*name).into(),
            path: PathBuf::from("/left").join(name),
            is_dir: false,
            is_symlink: false,
            is_parent: *name == "..",
            size: 1,
            modified: None,
            folder_size: None,
        })
        .collect();
    app
}

#[tokio::test]
async fn an_empty_panel_opens_no_dialog() {
    let mut app = app_with_entries(&[]);
    let _task = app.update(Message::Delete { permanent: false });
    assert!(app.ops.delete_dialog.is_none());
}

#[tokio::test]
async fn the_subject_is_the_name_for_one_and_the_count_for_many() {
    let mut app = app_with_entries(&["a.txt", "b.txt"]);
    app.panels.left.selected = 0;
    let _task = app.update(Message::Delete { permanent: false });
    let one = app.ops.delete_dialog.clone().unwrap();
    assert_eq!(ncrs_ops::delete_subject(app.lang, &one), "a.txt");

    app.ops.delete_dialog = None;
    app.panels.left.selection.toggle(Path::new("/left/a.txt"));
    app.panels.left.selection.toggle(Path::new("/left/b.txt"));
    let _again = app.update(Message::Delete { permanent: true });
    let many = app.ops.delete_dialog.clone().unwrap();
    assert_eq!(
        ncrs_ops::delete_subject(app.lang, &many),
        "2 items\na.txt\nb.txt"
    );
    assert!(many.permanent);

    // The trash dialog stays short: only the count.
    let trash = Deletion {
        permanent: false,
        ..many.clone()
    };
    assert_eq!(ncrs_ops::delete_subject(app.lang, &trash), "2 items");

    let seven = Deletion {
        sources: (1..=7)
            .map(|i| PathBuf::from(format!("/left/f{i}")))
            .collect(),
        ..many
    };
    assert_eq!(
        ncrs_ops::delete_subject(app.lang, &seven),
        "7 items\nf1\nf2\nf3\nf4\nf5\n… and 2 more"
    );
}

#[test]
fn the_sort_chord_is_routed_and_not_taken_for_type_ahead() {
    let binding = keymap::sort_binding(crate::fs::SortColumn::Size);
    assert_eq!(
        route_key(&PromptKeyState::default(), binding.key, binding.modifiers),
        Some(Message::Panel(PanelMessage::SortActive(
            crate::fs::SortColumn::Size
        )))
    );
}

#[test]
fn alt_ctrl_and_super_do_not_fall_back_to_the_bare_key() {
    let idle = PromptKeyState::default();
    for modifiers in [Modifiers::ALT, Modifiers::CTRL, Modifiers::LOGO] {
        assert_eq!(route(&idle, Named::Enter, modifiers), None, "{modifiers:?}");
        assert_eq!(route(&idle, Named::F8, modifiers), None, "{modifiers:?}");
    }
    // Ctrl+* is CTRL|SHIFT on a real keyboard, since `*` needs Shift.
    assert_eq!(
        route_key(
            &idle,
            Key::Character("*".into()),
            Modifiers::CTRL | Modifiers::SHIFT
        ),
        Some(Message::Panel(PanelMessage::ClearTags))
    );
    // Shift alone still reaches the bare key, as `*` needs.
    assert_eq!(
        route_key(&idle, Key::Character("*".into()), Modifiers::SHIFT),
        Some(Message::Panel(PanelMessage::TagAll))
    );
}

#[test]
fn only_movement_tagging_and_typing_repeat() {
    assert!(repeats(&Message::Panel(PanelMessage::MoveSelection(1))));
    assert!(repeats(&Message::Panel(PanelMessage::ToggleTag)));
    assert!(repeats(&Message::PromptInput("a".into())));
    assert!(!repeats(&Message::Delete { permanent: true }));
    assert!(!repeats(&Message::DeleteConfirm));
    assert!(!repeats(&Message::Transfer(TransferKind::Copy)));
}
