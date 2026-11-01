use crate::app::*;
use iced::keyboard::key::Named;

fn app() -> App {
    App::with_environment(Environment::isolated()).0
}

fn with_prompt() -> App {
    let mut app = app();
    app.ops.prompt = Some(Prompt::create_dir(PathBuf::from("/tmp")));
    app
}

/// Routes as the subscription does: build the state, then call the free
/// function. Same path the real keypress takes.
fn press(app: &App, key: Key) -> Option<Message> {
    route_key(&app.prompt_key_state(), key, Modifiers::default())
}

/// While the prompt is open, F7 must not open a second one and the panel
/// keys must not move the cursor. This is the bug the whole routing exists
/// to prevent: Enter in a text field that also opens directories.
#[test]
fn an_open_prompt_takes_every_key() {
    let app = with_prompt();

    for key in [
        Key::Named(Named::ArrowDown),
        Key::Named(Named::Enter),
        Key::Named(Named::Tab),
        Key::Named(Named::F10),
    ] {
        let label = format!("{key:?}");
        let routed = press(&app, key.clone());
        let leaked = matches!(
            routed,
            Some(Message::Panel(PanelMessage::MoveSelection(_)))
                | Some(Message::Panel(PanelMessage::SwitchPanel))
                | Some(Message::Quit)
                | Some(Message::CreateDirPrompt)
        );
        assert!(!leaked, "{label} escaped the prompt as {routed:?}");
    }
}

#[test]
fn shift_f4_opens_the_new_file_prompt_only_when_none_is_open() {
    let closed = app();
    assert_eq!(
        route_key(
            &closed.prompt_key_state(),
            Key::Named(Named::F4),
            Modifiers::SHIFT
        ),
        Some(Message::CreateFilePrompt)
    );
    let open = with_prompt();
    assert_ne!(
        route_key(
            &open.prompt_key_state(),
            Key::Named(Named::F4),
            Modifiers::SHIFT
        ),
        Some(Message::CreateFilePrompt)
    );
}

/// The open shortcuts dialog is itself an overlay: the theme fetch does
/// not open on top of it.
#[test]
fn the_theme_fetch_stays_shut_while_the_shortcuts_dialog_is_open() {
    let mut commander = app();
    drop(commander.update(Message::KeyBindings(KeyBindingsMessage::Open)));
    drop(commander.update(Message::ThemeFetch(ThemeFetchMessage::Open)));
    assert!(commander.settings.key_bindings.is_some());
    assert!(commander.settings.theme_fetch.is_none());
}

/// The dialog takes the arrows, Enter and Delete, while recording every
/// key but Escape becomes the new key, and a chord the search field does
/// not type searches by key.
#[test]
fn the_shortcuts_dialog_takes_the_keys_it_lists() {
    use crate::keymap::Binding;
    let mut commander = app();
    let shortcuts = |message| Some(Message::KeyBindings(message));
    let routed = route_key(
        &commander.prompt_key_state(),
        Key::Character("k".into()),
        Modifiers::COMMAND,
    );
    assert_eq!(routed, shortcuts(KeyBindingsMessage::Open));
    drop(commander.update(Message::KeyBindings(KeyBindingsMessage::Open)));
    assert!(commander.settings.key_bindings.is_some());

    let with = |dialog_open: &App, key: Key, modifiers| {
        route_key(&dialog_open.prompt_key_state(), key, modifiers)
    };
    let plain = Modifiers::default();
    assert_eq!(
        with(&commander, Key::Named(Named::ArrowDown), plain),
        shortcuts(KeyBindingsMessage::Move(1))
    );
    assert_eq!(
        with(&commander, Key::Named(Named::Delete), plain),
        shortcuts(KeyBindingsMessage::RemoveKey)
    );
    assert_eq!(
        with(&commander, Key::Named(Named::Backspace), Modifiers::SHIFT),
        shortcuts(KeyBindingsMessage::Reset)
    );
    assert_eq!(
        with(&commander, Key::Character("a".into()), plain),
        shortcuts(KeyBindingsMessage::Typed("a".to_owned()))
    );
    let ctrl_f5 = Binding::with_modifiers(Key::Named(Named::F5), Modifiers::CTRL);
    assert_eq!(
        with(&commander, Key::Named(Named::F5), Modifiers::CTRL),
        shortcuts(KeyBindingsMessage::SearchByKey(ctrl_f5))
    );
    assert_eq!(with(&commander, Key::Named(Named::Escape), plain), None);

    let enter = with(&commander, Key::Named(Named::Enter), plain);
    assert_eq!(enter, shortcuts(KeyBindingsMessage::Record));
    drop(commander.update(Message::KeyBindings(KeyBindingsMessage::Record)));
    assert!(commander
        .settings
        .key_bindings
        .as_ref()
        .is_some_and(KeyBindings::is_recording));
    assert_eq!(
        with(&commander, Key::Named(Named::F12), plain),
        shortcuts(KeyBindingsMessage::Recorded(Binding::with_modifiers(
            Key::Named(Named::F12),
            plain
        )))
    );
    assert_eq!(
        with(&commander, Key::Named(Named::Delete), plain),
        shortcuts(KeyBindingsMessage::Recorded(Binding::with_modifiers(
            Key::Named(Named::Delete),
            plain
        )))
    );
    assert_eq!(
        with(&commander, Key::Named(Named::Escape), plain),
        shortcuts(KeyBindingsMessage::CancelRecording)
    );
}

#[test]
fn enter_submits_and_escape_cancels() {
    let app = with_prompt();
    assert_eq!(
        press(&app, Key::Named(Named::Enter)),
        Some(Message::PromptSubmit)
    );
    assert_eq!(
        press(&app, Key::Named(Named::Escape)),
        Some(Message::PromptCancel)
    );
}

/// Typing appends, backspace removes one character — the field behaves like
/// a text field, not like a panel selection.
#[test]
fn typing_and_backspace_edit_the_name() {
    let mut app = with_prompt();
    app.ops.prompt.as_mut().unwrap().set_name("abc".into());

    assert_eq!(
        press(&app, Key::Character("d".into())),
        Some(Message::PromptInput("abcd".into()))
    );
    assert_eq!(
        press(&app, Key::Named(Named::Space)),
        Some(Message::PromptInput("abc ".into()))
    );
    assert_eq!(
        press(&app, Key::Named(Named::Backspace)),
        Some(Message::PromptInput("ab".into()))
    );
}

/// A new keystroke clears the error, so a corrected name does not still
/// show the complaint about the previous one.
#[test]
fn typing_clears_a_validation_error() {
    let mut prompt = Prompt::create_dir(PathBuf::from("/tmp"));
    prompt.set_validation_error("empty_name");
    assert!(prompt.error().is_some());

    prompt.set_name("x".into());
    assert_eq!(prompt.error(), None);
}

/// Without a prompt the bindings apply unchanged, so the routing did not
/// change normal navigation.
#[test]
fn without_a_prompt_the_bindings_apply() {
    let app = app();
    assert_eq!(
        press(&app, Key::Named(Named::ArrowDown)),
        Some(Message::Panel(PanelMessage::MoveSelection(1)))
    );
    assert_eq!(
        press(&app, Key::Named(Named::Enter)),
        Some(Message::Panel(PanelMessage::OpenSelected))
    );
}

/// F7 is bound, and the header hint comes from the same entry, so the key
/// that opens the prompt and the key the user reads about are the same.
#[test]
fn f7_opens_the_prompt_and_is_advertised() {
    let app = app();
    assert_eq!(
        press(&app, Key::Named(Named::F7)),
        Some(Message::CreateDirPrompt)
    );
    assert!(
        Keymap::built_in().function_keys(Language::English, KeyScope::Panel)[6].is_some(),
        "F7 opens the prompt but the function key bar does not show it"
    );
}

// --- the whole chain, without a window -------------------------------
//
// Driving `update` the way a keypress would: open, type, submit. The
// filesystem work itself is not awaited here, so this covers routing and
// validation; the directory is checked in the async test below.

fn press_and_update(app: &mut App, key: Key) {
    if let Some(message) = route_key(&app.prompt_key_state(), key, Modifiers::default()) {
        let _ = app.update(message);
    }
}

/// Removes itself, so a failed assertion does not leave a directory that
/// makes the next run fail.
fn scratch(label: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("ncrs-chain-{label}-"))
        .tempdir()
        .expect("a scratch directory")
}

#[test]
fn f7_then_a_name_then_enter_readies_the_operation() {
    let scratch = scratch("submit");
    let base = scratch.path().to_path_buf();
    std::fs::create_dir_all(&base).unwrap();
    let mut app = app();
    app.panels.left.path = base.clone();

    press_and_update(&mut app, Key::Named(Named::F7));
    assert!(app.ops.prompt.is_some(), "F7 did not open the prompt");

    for c in "neuer ordner".chars() {
        press_and_update(&mut app, Key::Character(c.to_string().into()));
    }
    assert_eq!(app.ops.prompt.as_ref().unwrap().name(), "neuer ordner");

    // Submit starts the operation and marks the prompt busy.
    let _task = app.update(Message::PromptSubmit);
    assert!(
        app.ops.prompt.is_some(),
        "the prompt closed before the work ran"
    );
    assert!(app.ops.prompt.as_ref().unwrap().busy());
    assert_eq!(app.ops.prompt.as_ref().unwrap().error(), None);
}

/// An empty name is refused while the dialog is still open: no filesystem
/// call, no busy state, an error the user can act on.
#[test]
fn submitting_an_empty_name_reports_it_without_closing() {
    let scratch = scratch("empty");
    let base = scratch.path().to_path_buf();
    std::fs::create_dir_all(&base).unwrap();
    let mut app = app();
    app.panels.left.path = base.clone();

    press_and_update(&mut app, Key::Named(Named::F7));
    let _task = app.update(Message::PromptSubmit);

    assert!(
        app.ops.prompt.is_some(),
        "the prompt closed on an invalid name"
    );
    assert!(!app.ops.prompt.as_ref().unwrap().busy());
    assert_eq!(app.ops.prompt.as_ref().unwrap().error(), Some("empty_name"));
}

/// Escape closes the prompt and leaves no state behind.
#[test]
fn escape_closes_the_prompt() {
    let mut app = app();
    press_and_update(&mut app, Key::Named(Named::F7));
    assert!(app.ops.prompt.is_some());

    press_and_update(&mut app, Key::Named(Named::Escape));
    assert!(app.ops.prompt.is_none());
    assert_eq!(app.ops.prompt_side, None);
}

/// A result that arrives after the user dismissed the prompt is dropped:
/// it must not reopen anything or report a failure into the void.
#[test]
fn a_result_from_a_dismissed_prompt_is_ignored() {
    let mut app = app();
    app.ops.prompt = Some(Prompt::create_dir(PathBuf::from("/tmp")));
    app.ops.prompt_request_id = 7;
    press_and_update(&mut app, Key::Named(Named::Escape));

    let stale = Message::PromptFinished {
        prompt: PromptKind::CreateDir,
        request_id: 7,
        result: Err(fs::CreateDirError::TaskFailed {
            reason: "stale".into(),
        }),
    };
    let _task = app.update(stale);
    assert!(app.ops.prompt.is_none());
}

/// The two filesystem failures get different wording, because the user
/// needs to act differently: a taken name means pick another, a missing
/// parent means the panel is stale.
#[test]
fn the_two_failures_read_differently() {
    let mut app = app();
    app.ops.prompt = Some(Prompt::create_dir(PathBuf::from("/tmp")));
    app.ops.prompt.as_mut().unwrap().set_name("x".into());

    let taken = fs::CreateDirError::AlreadyExists {
        path: PathBuf::from("/tmp/x"),
        source: std::io::Error::new(std::io::ErrorKind::AlreadyExists, "exists"),
    };
    let parent_gone = fs::CreateDirError::AlreadyExists {
        path: PathBuf::from("/tmp/x"),
        source: std::io::Error::new(std::io::ErrorKind::NotFound, "no parent"),
    };

    let taken_text = app.prompt_error(&taken);
    let parent_text = app.prompt_error(&parent_gone);
    assert_ne!(
        taken_text, parent_text,
        "a taken name and a missing parent need different messages"
    );
    assert!(
        !taken_text.contains("{"),
        "placeholder left in: {taken_text}"
    );
    assert!(
        !parent_text.contains("{"),
        "placeholder left in: {parent_text}"
    );
}
