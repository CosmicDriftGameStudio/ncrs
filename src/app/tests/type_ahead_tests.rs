use crate::app::*;
use std::time::{Duration, Instant};

/// An app on a panel of `names` (after `..`) and a clock the test moves.
fn app_with(names: &[&str]) -> (App, Arc<Mutex<Instant>>) {
    let now = Arc::new(Mutex::new(Instant::now()));
    let clock = Arc::clone(&now);
    let mut app = App::with_fixed_panels().with_clock(move || *clock.lock().unwrap());
    app.panel_mut(PanelSide::Left).entries = std::iter::once("..")
        .chain(names.iter().copied())
        .enumerate()
        .map(|(index, name)| fs::FileEntry {
            name: name.into(),
            path: PathBuf::from("/home/test").join(name),
            is_dir: false,
            is_symlink: false,
            is_parent: index == 0,
            size: 0,
            modified: None,
            folder_size: None,
        })
        .collect();
    (app, now)
}

fn type_key(app: &mut App, typed: &str) {
    drop(app.update(Message::Panel(PanelMessage::TypeAhead(typed.to_string()))));
}

fn advance(clock: &Arc<Mutex<Instant>>, by: Duration) {
    *clock.lock().unwrap() += by;
}

fn cursor(app: &App) -> usize {
    app.panel(PanelSide::Left).selected
}

const SECOND: Duration = Duration::from_secs(1);

#[test]
fn q_is_type_ahead_and_only_command_q_quits() {
    let (mut app, _clock) = app_with(&["alpha", "quarry"]);
    let state = app.prompt_key_state();
    let q = || Key::Character("q".into());
    let typed = route_key(&state, q(), Modifiers::default());
    assert_eq!(
        typed,
        Some(Message::Panel(PanelMessage::TypeAhead("q".to_string())))
    );
    drop(app.update(typed.unwrap()));
    assert_eq!(cursor(&app), 2);
    assert_eq!(
        route_key(&state, q(), Modifiers::COMMAND),
        Some(Message::Quit)
    );
}

#[test]
fn a_character_jumps_to_the_first_entry_it_starts_ignoring_case() {
    let (mut app, _clock) = app_with(&["alpha", "Beta", "bravo"]);
    type_key(&mut app, "b");
    assert_eq!(cursor(&app), 2);
    type_key(&mut app, "r");
    assert_eq!(cursor(&app), 3, "br extends the prefix");
}

#[test]
fn a_character_within_a_second_extends_the_prefix_and_later_starts_anew() {
    let (mut app, clock) = app_with(&["ab", "ac", "ba", "bc"]);
    type_key(&mut app, "b");
    assert_eq!(cursor(&app), 3);
    advance(&clock, SECOND);
    type_key(&mut app, "c");
    assert_eq!(cursor(&app), 4, "bc, one second after b");
    advance(&clock, SECOND + Duration::from_millis(1));
    type_key(&mut app, "a");
    assert_eq!(cursor(&app), 1, "a on its own, not bca");
}

#[test]
fn no_match_leaves_the_cursor() {
    let (mut app, _clock) = app_with(&["alpha", "beta"]);
    type_key(&mut app, "b");
    assert_eq!(cursor(&app), 2);
    type_key(&mut app, "z");
    assert_eq!(cursor(&app), 2);
}

#[test]
fn the_parent_row_does_not_count() {
    let (mut app, _clock) = app_with(&["alpha"]);
    type_key(&mut app, ".");
    assert_eq!(cursor(&app), 0);
}

#[test]
fn moving_the_cursor_or_the_panel_ends_the_prefix() {
    for reset in [
        Message::Panel(PanelMessage::MoveSelection(1)),
        Message::Panel(PanelMessage::SwitchPanel),
        Message::Panel(PanelMessage::SelectFirst),
        Message::Panel(PanelMessage::PageDown),
    ] {
        let (mut app, _clock) = app_with(&["ab", "ba", "bb", "ca"]);
        type_key(&mut app, "b");
        drop(app.update(reset.clone()));
        app.panels.active = PanelSide::Left;
        type_key(&mut app, "a");
        assert_eq!(cursor(&app), 1, "{reset:?} did not end the prefix");
    }
}

#[test]
fn the_view_follows_the_cursor() {
    let names: Vec<String> = (0..40).map(|i| format!("file{i:02}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let (mut app, _clock) = app_with(&refs);
    app.set_visible_rows_for_test(5);
    type_key(&mut app, "f");
    for digit in ["i", "l", "e", "3", "5"] {
        type_key(&mut app, digit);
    }
    assert_eq!(cursor(&app), 36);
    let panel = app.panel(PanelSide::Left);
    assert!(panel.scroll_offset <= 36 && 36 < panel.scroll_offset + 5);
}

#[test]
fn nothing_happens_under_an_overlay() {
    let (mut app, _clock) = app_with(&["alpha", "beta"]);
    app.open_delete_dialog_for_test(1, false);
    type_key(&mut app, "b");
    assert_eq!(cursor(&app), 0);
}

#[test]
fn printable_keys_route_to_type_ahead_but_not_with_option_ctrl_or_cmd() {
    let route = |key: &str, modifiers: Modifiers| {
        route_key(
            &PromptKeyState::default(),
            Key::Character(key.into()),
            modifiers,
        )
    };
    assert_eq!(
        route("a", Modifiers::default()),
        Some(Message::Panel(PanelMessage::TypeAhead("a".into())))
    );
    assert_eq!(
        route("A", Modifiers::SHIFT),
        Some(Message::Panel(PanelMessage::TypeAhead("A".into())))
    );
    assert_eq!(route("ç", Modifiers::ALT), None);
    assert_eq!(route("a", Modifiers::CTRL), None);
    assert_eq!(route("a", Modifiers::LOGO), None);
    assert_eq!(
        route("*", Modifiers::SHIFT),
        Some(Message::Panel(PanelMessage::TagAll))
    );
    assert_eq!(
        route_key(
            &PromptKeyState::default(),
            Key::Named(Named::Space),
            Modifiers::default()
        ),
        Some(Message::Panel(PanelMessage::ToggleTag))
    );
    let prompt = PromptKeyState {
        open: true,
        ..PromptKeyState::default()
    };
    assert_eq!(
        route_key(&prompt, Key::Character("a".into()), Modifiers::default()),
        Some(Message::PromptInput("a".into()))
    );
}
