use crate::app::*;

fn route(state: &PromptKeyState, key: Named, modifiers: Modifiers) -> Option<Message> {
    route_key(state, Key::Named(key), modifiers)
}

fn open_state() -> PromptKeyState {
    PromptKeyState {
        quick_open: true,
        ..PromptKeyState::default()
    }
}

fn ctrl_p(state: &PromptKeyState) -> Option<Message> {
    route_key(state, Key::Character("p".into()), Modifiers::COMMAND)
}

#[test]
fn the_arrows_move_the_hits_and_the_panel_keys_are_dead_while_it_is_open() {
    let state = open_state();
    let none = Modifiers::default();
    assert_eq!(
        route(&state, Named::ArrowDown, none),
        Some(Message::QuickOpen(QuickOpenMessage::Move(1)))
    );
    assert_eq!(
        route(&state, Named::ArrowUp, none),
        Some(Message::QuickOpen(QuickOpenMessage::Move(-1)))
    );
    assert_eq!(route(&state, Named::F7, none), None);
    assert_eq!(route(&state, Named::F5, none), None);
    assert_eq!(route(&state, Named::Backspace, none), None);
    assert_eq!(route(&state, Named::F10, none), Some(Message::Quit));
}

#[test]
fn the_chord_opens_it_over_the_panels_and_not_over_the_search() {
    let panel = PromptKeyState::default();
    assert_eq!(
        ctrl_p(&panel),
        Some(Message::QuickOpen(QuickOpenMessage::Open))
    );
    let search = PromptKeyState {
        search: true,
        ..PromptKeyState::default()
    };
    assert_eq!(ctrl_p(&search), None);
}

#[test]
fn opening_lists_below_the_active_panel_and_takes_the_keys() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::QuickOpen(QuickOpenMessage::Open)));
    let quick_open = app.quick_open_for_test().unwrap();
    assert_eq!(quick_open.root(), app.panel(PanelSide::Left).path);
    assert!(app.prompt_key_state().quick_open);
    assert!(app.blocking_overlay_is_open());
}

#[test]
fn it_does_not_open_over_a_dialog() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::CreateDirPrompt));
    drop(app.update(Message::QuickOpen(QuickOpenMessage::Open)));
    assert!(app.quick_open_for_test().is_none());
}

#[test]
fn a_batch_of_a_closed_walk_is_dropped() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::QuickOpen(QuickOpenMessage::Open)));
    let old = app.quick_open_for_test().unwrap().walk_generation();
    drop(app.update(Message::QuickOpen(QuickOpenMessage::Close)));
    drop(app.update(Message::QuickOpen(QuickOpenMessage::Open)));
    let root = app.quick_open_for_test().unwrap().root().to_path_buf();
    drop(app.update(Message::QuickOpen(QuickOpenMessage::Events {
        generation: old,
        events: vec![crate::quick_open::WalkEvent::File {
            path: root.join("late.txt"),
        }],
    })));
    assert_eq!(app.quick_open_for_test().unwrap().file_count(), 0);
}

#[test]
fn enter_closes_it_and_loads_the_hit_into_the_active_panel() {
    let mut app = App::with_fixed_panels();
    app.open_quick_open_for_test(&["src/config.rs", "README.md"], "cfgrs");
    assert_eq!(app.quick_open_for_test().unwrap().hits().len(), 1);
    let before = app.panel(PanelSide::Left).request_id;
    drop(app.update(Message::QuickOpen(QuickOpenMessage::Activate)));
    assert!(app.quick_open_for_test().is_none());
    assert!(app.quick_open_walk.is_none());
    assert!(app.panel(PanelSide::Left).loading);
    assert_ne!(app.panel(PanelSide::Left).request_id, before);
}

#[test]
fn enter_before_the_background_ranking_is_back_still_loads_the_hit() {
    let mut app = App::with_fixed_panels();
    app.open_quick_open_for_test(&["src/config.rs", "README.md"], "");
    // The task that would rank in the background is dropped unrun.
    drop(
        app.update(Message::QuickOpen(QuickOpenMessage::QueryChanged(
            "cfgrs".to_string(),
        ))),
    );
    assert!(app.quick_open_for_test().unwrap().hits().is_empty());
    let before = app.panel(PanelSide::Left).request_id;
    drop(app.update(Message::QuickOpen(QuickOpenMessage::Activate)));
    assert!(app.quick_open_for_test().is_none());
    assert_ne!(app.panel(PanelSide::Left).request_id, before);
}

#[test]
fn enter_without_a_hit_does_nothing() {
    let mut app = App::with_fixed_panels();
    app.open_quick_open_for_test(&["README.md"], "zzz");
    drop(app.update(Message::QuickOpen(QuickOpenMessage::Activate)));
    assert!(app.quick_open_for_test().is_some());
}

#[test]
fn a_conflict_closes_it() {
    let mut app = App::with_fixed_panels();
    app.open_quick_open_for_test(&["a.txt"], "a");
    let transfer = Transfer {
        kind: TransferKind::Copy,
        sources: vec![PathBuf::from("/left/a.txt")],
        target: PathBuf::from("/right"),
        names: Vec::new(),
    };
    app.ops.transfer = Some(transfer);
    drop(app.update(Message::TransferRowDone {
        result: Err(RowFailure::Conflict("exists".to_string())),
        index: 0,
        generation: app.ops.generation,
    }));
    assert!(app.ops.pending_conflict.is_some());
    assert!(app.quick_open_for_test().is_none());
}
