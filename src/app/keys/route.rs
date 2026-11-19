use crate::app::*;
use crate::messages::MenuMessage;

fn delete_dialog_key(
    prompt: &PromptKeyState,
    key: &Key,
    key_modifiers: Modifiers,
    scope: KeyScope,
) -> Option<Message> {
    let permanent = prompt.delete_dialog == DeleteKeys::Permanent;
    match key.as_ref() {
        Key::Named(Named::Escape) => Some(Message::from(ncrs_ops::OpsMessage::DeleteCancel)),
        // The fixed dialog keys come first: a configured chord must not
        // take Enter or an arrow, or one press could confirm a deletion.
        // Only then the chord that started the permanent delete confirms it.
        _ => dialog_focus_key(key.clone(), key_modifiers).or_else(|| {
            (permanent
                && bound_message(&prompt.keymap, key, key_modifiers, scope)
                    == Some(Message::Delete { permanent: true }))
            .then_some(Message::Ops(OpsMessage::DeleteConfirm))
        }),
    }
}

fn key_scope(prompt: &PromptKeyState) -> KeyScope {
    if prompt.search {
        KeyScope::Search
    } else {
        KeyScope::Panel
    }
}

/// The keys of the overlay on top, which takes them all.
fn modal_key(
    prompt: &PromptKeyState,
    modal: Modal,
    key: Key,
    key_modifiers: Modifiers,
) -> Option<Message> {
    let scope = key_scope(prompt);
    match modal {
        // A menu takes the arrows and Enter that would otherwise move the
        // panel behind it; any other key does nothing until it is closed.
        Modal::HeaderMenu => ncrs_files::keys::header_menu_key(&key).map(Message::Panel),
        Modal::VolumeMenu => ncrs_files::keys::volume_menu_key(&key).map(volume_menu_message),
        Modal::QuickFix => ncrs_hermes_ui::keys::quick_fix_key(&key).map(Message::from),
        Modal::ConfirmAgain => ncrs_hermes_ui::keys::confirm_again_key(&key).map(Message::from),
        Modal::FavouriteMenu => {
            ncrs_files::favourites_bar::menu_key(&key).map(Message::FavouritesBar)
        }
        Modal::ContextMenu => context_menu_key(prompt, &key),
        Modal::Delete => delete_dialog_key(prompt, &key, key_modifiers, scope),
        Modal::Quit => card_dialog_key(
            Message::from(ncrs_ops::OpsMessage::QuitCancel),
            key,
            key_modifiers,
        ),
        Modal::OpenFile => card_dialog_key(
            Message::from(ncrs_ops::OpsMessage::OpenFileCancel),
            key,
            key_modifiers,
        ),
        Modal::BatchRename => ncrs_ops::keys::batch_rename_key(&key)
            .map(|inner| Message::from(ncrs_ops::OpsMessage::BatchRename(inner))),
        Modal::Permissions => ncrs_ops::keys::permissions_key(&key)
            .map(|inner| Message::from(ncrs_ops::OpsMessage::Permissions(inner))),
        Modal::KeyBindings => key_bindings_key(prompt, &key, key_modifiers),
        // The fields take the typing and Enter; Tab goes from one to the
        // next. Of the bindings only quitting reaches past the dialog.
        Modal::Compare => compare_dialog_key(prompt, &key, key_modifiers, scope),
        Modal::Connect => connect_dialog_key(prompt, &key, key_modifiers, scope),
        // Cancel has the focus when a download card opens, so Enter alone
        // never downloads.
        Modal::About | Modal::OcrModels | Modal::AiModel | Modal::Grant | Modal::StartOver => {
            let close = card_close_message(prompt)?;
            card_dialog_key(close, key, key_modifiers)
        }
        Modal::Conflict => conflict_key(prompt, key, key_modifiers, scope),
        // The target field takes the typing and Enter on its own; Escape is
        // `InputState`'s there and here. Tab switches between copy and move,
        // as the footer says; of the bindings only quitting reaches past the
        // dialog.
        Modal::TransferDialog | Modal::Progress => {
            transfer_overlay_key(prompt, &key, key_modifiers, scope)
        }
        // The field takes the typing and Enter; Escape is `InputState`'s there
        // and here, so it closes the palette with a job running too. The
        // arrows are the entries'. Of the bindings only quitting and the
        // palette's own chord (which gives the field the focus back) reach
        // past it.
        Modal::CommandPalette => match key.as_ref() {
            Key::Named(Named::Escape) => {
                Some(Message::CommandPalette(CommandPaletteMessage::Close))
            }
            Key::Named(Named::Enter) => Some(Message::CommandPalette(CommandPaletteMessage::Run)),
            Key::Named(Named::ArrowUp) if key_modifiers.is_empty() => {
                Some(Message::CommandPalette(CommandPaletteMessage::Move(-1)))
            }
            Key::Named(Named::ArrowDown) if key_modifiers.is_empty() => {
                Some(Message::CommandPalette(CommandPaletteMessage::Move(1)))
            }
            _ => bound_message(&prompt.keymap, &key, key_modifiers, scope)
                .filter(|message| {
                    matches!(
                        message,
                        Message::Quit | Message::CommandPalette(CommandPaletteMessage::Toggle)
                    )
                })
                .filter(|_| is_not_typed_by_text_field(&key, key_modifiers)),
        },
        // Like the command palette: the field takes the typing, the card the
        // rest. Enter reaches the routing when the list has the keys, not the
        // field.
        Modal::ThemeFetch => theme_fetch_key(prompt, &key, key_modifiers, scope),
        // The field takes the typing, Enter and Escape (`InputState`); the
        // arrows are the hits'. Of the bindings only quitting reaches past it,
        // as it does for the path field.
        Modal::QuickOpen => match key.as_ref() {
            Key::Named(Named::ArrowUp) if key_modifiers.is_empty() => {
                Some(Message::QuickOpen(QuickOpenMessage::Move(-1)))
            }
            Key::Named(Named::ArrowDown) if key_modifiers.is_empty() => {
                Some(Message::QuickOpen(QuickOpenMessage::Move(1)))
            }
            _ => bound_message(&prompt.keymap, &key, key_modifiers, scope)
                .filter(|message| *message == Message::Quit)
                .filter(|_| is_not_typed_by_text_field(&key, key_modifiers)),
        },
        Modal::Prompt => prompt_key(prompt, &key),
    }
}

/// Keys while no overlay is up: the chat, the pages, the fields and the
/// panels.
fn keys_without_modal(
    prompt: &PromptKeyState,
    key: Key,
    key_modifiers: Modifiers,
) -> Option<Message> {
    let scope = key_scope(prompt);
    // The chat's field takes the typing and Enter; Escape is `InputState`'s
    // there and here. Of the bindings only quitting and the keys of the other
    // pages reach past the chat; no other function key reaches the panels
    // behind it.
    if prompt.hermes_chat {
        return hermes_chat_key(prompt, &key, key_modifiers, scope);
    }

    // Tab moves inside the shown page, never to another tab (Ctrl+Tab does
    // that): the widgets are asked whether a text field has the keyboard
    // (`TabInPage`). The System page's own filter, a dialog or a menu keep the key.
    let page_tab_stop = (prompt.monitor
        && !(prompt.monitor_filter || prompt.monitor_dialog || prompt.monitor_menu))
        || prompt.settings;
    if page_tab_stop && matches!(key.as_ref(), Key::Named(Named::Tab)) {
        if let Some(message) = page_tab_message(key_modifiers) {
            return Some(message);
        }
    }

    if prompt.shows_viewer_or_monitor() {
        return viewer_or_monitor_key(prompt, &key, key_modifiers, scope);
    }

    if prompt.settings {
        return settings_key(prompt, &key, key_modifiers, scope);
    }

    // The text field takes the typing and Enter on its own, and `InputState`
    // sees Escape and Tab leave it. Of the bindings only quitting reaches past
    // it: the field has no use for F10 or Cmd+Q, and a program that cannot be
    // closed while a path is half typed is a trap. Only a key the field does
    // not type: a quit rebound to `*` or Backspace must not end the program in
    // the middle of a path.
    if prompt.path_editing {
        return bound_message(&prompt.keymap, &key, key_modifiers, scope)
            .filter(|message| *message == Message::Quit)
            .filter(|_| is_not_typed_by_text_field(&key, key_modifiers));
    }

    if prompt.command_line {
        return command_line_key(prompt, &key, key_modifiers, scope);
    }

    if prompt.trash {
        return trash_key(prompt, &key, key_modifiers);
    }

    // Escape is `InputState`'s: the text fields capture it, so it only ever
    // reaches the routing when no field has the focus.
    if prompt.search {
        return search_key(prompt, &key, key_modifiers);
    }

    if matches!(key.as_ref(), Key::Named(Named::Escape)) {
        return escape_message(prompt);
    }
    bound_message(&prompt.keymap, &key, key_modifiers, scope)
        .or_else(|| type_ahead_message(&key, key_modifiers))
}

fn hermes_chat_key(
    prompt: &PromptKeyState,
    key: &Key,
    key_modifiers: Modifiers,
    scope: KeyScope,
) -> Option<Message> {
    let quit = || {
        bound_message(&prompt.keymap, key, key_modifiers, scope)
            .filter(|message| *message == Message::Quit)
            .filter(|_| is_not_typed_by_text_field(key, key_modifiers))
    };
    if matches!(key.as_ref(), Key::Named(Named::Tab)) {
        if let Some(message) = page_tab_message(key_modifiers) {
            return Some(message);
        }
    }
    let other_page = || {
        bound_message(&prompt.keymap, key, key_modifiers, scope)
            // The chat's own key closes it; `chat_key` answers that below.
            .filter(|message| {
                is_page_spanning(message) && *message != KeyAction::HermesChat.message()
            })
            .filter(|_| is_not_typed_by_text_field(key, key_modifiers))
    };
    if let Some(page) = other_page() {
        return Some(page);
    }
    match ncrs_hermes_chat::chat_key(key, key_modifiers) {
        ncrs_hermes_chat::ChatKey::Close => Some(Message::HermesChat(HermesChatMessage::Close)),
        ncrs_hermes_chat::ChatKey::CloseToCommandLine => Some(Message::HermesChatToCommandLine),
        ncrs_hermes_chat::ChatKey::Pass | ncrs_hermes_chat::ChatKey::Type => quit(),
        ncrs_hermes_chat::ChatKey::Swallow => None,
    }
}

/// The binding a press hits, with the Shift rules of the panels: see the
/// comments inside.
pub(in crate::app) fn bound_message(
    keymap: &Keymap,
    key: &Key,
    key_modifiers: Modifiers,
    scope: KeyScope,
) -> Option<Message> {
    // The exact chord first (Shift+F8 is not F8). Only Shift may fall back to
    // the bare key: it changes the character typed (`*` is Shift+8), while Alt,
    // Ctrl and Super make a different chord that must not trigger the plain one.
    //
    // For a character, Shift is already in the character (`*` needs it on the
    // US and German layouts), so Ctrl+`*` arrives as CTRL|SHIFT and has to be
    // looked up as Ctrl. Named keys keep it: Shift+F8 is not F8.
    //
    // Except for a chord that names Shift itself (Ctrl+Shift+C): tried first,
    // with the letter lowercased, because Shift made it a capital.
    if key_modifiers.shift() {
        if let Some(message) = keymap.map_key(&lowercase_character(key), key_modifiers, scope) {
            return Some(message);
        }
    }
    let modifiers = if matches!(key, Key::Character(_)) {
        key_modifiers - Modifiers::SHIFT
    } else {
        key_modifiers
    };
    keymap.map_key(key, modifiers, scope).or_else(|| {
        (modifiers - Modifiers::SHIFT)
            .is_empty()
            .then(|| keymap.map_key(key, Modifiers::default(), scope))
            .flatten()
    })
}

/// A function key, or a chord with Super or Ctrl (without Alt): nothing a
/// text field would type.
pub(in crate::app) fn is_not_typed_by_text_field(key: &Key, modifiers: Modifiers) -> bool {
    let is_function_key = matches!(
        key,
        Key::Named(
            Named::F1
                | Named::F2
                | Named::F3
                | Named::F4
                | Named::F5
                | Named::F6
                | Named::F7
                | Named::F8
                | Named::F9
                | Named::F10
                | Named::F11
                | Named::F12
        )
    );
    // Alt types characters on a Mac, and AltGr arrives as Ctrl+Alt on Windows.
    let is_chord = modifiers.logo() || (modifiers.control() && !modifiers.alt());
    is_function_key || is_chord
}

pub(in crate::app) fn type_ahead_message(key: &Key, modifiers: Modifiers) -> Option<Message> {
    ncrs_files::keys::type_ahead_message(key, modifiers).map(Message::Panel)
}

fn lowercase_character(key: &Key) -> Key {
    match key {
        Key::Character(c) => Key::Character(c.to_lowercase().into()),
        other => other.clone(),
    }
}

/// The key a binding is written against.
///
/// With Option held a Mac types another character: Option+C is "ç", and with
/// Ctrl it can be a control character (Ctrl+X types U+0018). The binding is
/// for the letter or digit on the key, so such a press is taken from the
/// control character or, for a non-ASCII character, the physical key instead.
///
/// Only with Ctrl, Alt or Super held, and never for printable ASCII: that is
/// what the user typed (`*` on Ctrl+Shift+8, punctuation on the AZERTY digit
/// row), and the binding is written against it.
pub(in crate::app) fn binding_key(
    key: Key,
    physical_key: keyboard::key::Physical,
    modifiers: Modifiers,
) -> Key {
    use keyboard::key::{Code, Physical};
    let Key::Character(typed) = &key else {
        return key;
    };
    if !modifiers.intersects(Modifiers::CTRL | Modifiers::ALT | Modifiers::LOGO) {
        return key;
    }
    let mut typed_chars = typed.chars();
    if let (Some(control @ '\u{1}'..='\u{1a}'), None) = (typed_chars.next(), typed_chars.next()) {
        if let Some(letter) = char::from_u32('a' as u32 + control as u32 - 1) {
            return Key::Character(letter.to_string().into());
        }
    }
    if typed.is_ascii() {
        return key;
    }
    let Physical::Code(code) = physical_key else {
        return key;
    };
    let character = match code {
        Code::KeyA => "a",
        Code::KeyB => "b",
        Code::KeyC => "c",
        Code::KeyD => "d",
        Code::KeyE => "e",
        Code::KeyF => "f",
        Code::KeyG => "g",
        Code::KeyH => "h",
        Code::KeyI => "i",
        Code::KeyJ => "j",
        Code::KeyK => "k",
        Code::KeyL => "l",
        Code::KeyM => "m",
        Code::KeyN => "n",
        Code::KeyO => "o",
        Code::KeyP => "p",
        Code::KeyQ => "q",
        Code::KeyR => "r",
        Code::KeyS => "s",
        Code::KeyT => "t",
        Code::KeyU => "u",
        Code::KeyV => "v",
        Code::KeyW => "w",
        Code::KeyX => "x",
        Code::KeyY => "y",
        Code::KeyZ => "z",
        Code::Digit0 => "0",
        Code::Digit1 => "1",
        Code::Digit2 => "2",
        Code::Digit3 => "3",
        Code::Digit4 => "4",
        Code::Digit5 => "5",
        Code::Digit6 => "6",
        Code::Digit7 => "7",
        Code::Digit8 => "8",
        Code::Digit9 => "9",
        _ => return key,
    };
    Key::Character(character.into())
}

/// A key press as the subscription sees it: the routed message, unless the
/// press is a key repeat of something that must happen once.
pub fn route_press(
    prompt: &PromptKeyState,
    key: Key,
    physical_key: keyboard::key::Physical,
    modifiers: Modifiers,
    repeat: bool,
) -> Option<Message> {
    // A text prompt types what the key produced, so only the bindings see the
    // physical letter.
    let routed_key = if prompt.top_modal() == Some(Modal::Prompt) {
        key.clone()
    } else {
        binding_key(key.clone(), physical_key, modifiers)
    };
    let routed = match route_key(prompt, routed_key, modifiers) {
        // The jump looks for the character typed (Option+C is "ç" on a Mac),
        // not for the key a binding is written against.
        Some(Message::Panel(PanelMessage::TypeAhead(_))) => type_ahead_message(&key, modifiers),
        other => other,
    };
    routed.filter(|message| !repeat || repeats(message))
}

pub(in crate::app) fn dialog_focus_key(key: Key, modifiers: Modifiers) -> Option<Message> {
    ncrs_ops::keys::dialog_focus_key(&key, modifiers).map(dialog_message)
}

pub(in crate::app) fn dialog_message(pressed: DialogKey) -> Message {
    match pressed {
        DialogKey::Activate => Message::DialogActivate,
        DialogKey::Focus(step) => Message::DialogFocus(step),
    }
}

/// Whether holding the key down may fire the message again.
///
/// Only cursor movement, tagging and typing repeat. An action such as F5 or a
/// dialog answer must happen once per press: a held Shift+F8 would otherwise
/// confirm the permanent-delete dialog it has just opened.
pub(in crate::app) fn repeats(message: &Message) -> bool {
    matches!(
        message,
        Message::Panel(PanelMessage::MoveSelection(_))
            | Message::Monitor(
                MonitorMessage::Move(_)
                    | MonitorMessage::PageMove(_)
                    | MonitorMessage::First
                    | MonitorMessage::Last
                    | MonitorMessage::ShiftCommands(_)
                    | MonitorMessage::MenuMove(_),
            )
            | Message::Panel(PanelMessage::PageUp)
            | Message::Panel(PanelMessage::PageDown)
            | Message::Panel(PanelMessage::SelectFirst)
            | Message::Panel(PanelMessage::SelectLast)
            | Message::Panel(PanelMessage::ToggleTag)
            | Message::Panel(PanelMessage::TagMove(_))
            | Message::Panel(PanelMessage::VolumeMenuMove(_))
            | Message::Panel(PanelMessage::VolumeMenuFirst)
            | Message::Panel(PanelMessage::VolumeMenuLast)
            | Message::Ops(OpsMessage::PromptInput(_))
            | Message::CommandLine(
                CommandLineMessage::HistoryPrevious | CommandLineMessage::HistoryNext,
            )
            | Message::Files(MenuMessage::ContextMenuMove(_))
            | Message::Files(MenuMessage::ContextMenuFirst)
            | Message::Files(MenuMessage::ContextMenuLast)
            | Message::QuickFix(QuickFixMessage::Move(_))
            | Message::QuickOpen(QuickOpenMessage::Move(_))
            | Message::CommandPalette(CommandPaletteMessage::Move(_))
            | Message::KeyBindings(KeyBindingsMessage::Move(_) | KeyBindingsMessage::MoveKey(_))
            | Message::ThemeFetch(ThemeFetchMessage::Move(_))
            | Message::Search(
                SearchMessage::ToggleTag
                    | SearchMessage::TagMove(_)
                    | SearchMessage::Move(_)
                    | SearchMessage::Page(_)
                    | SearchMessage::First
                    | SearchMessage::Last
                    | SearchMessage::AppendToQuery(_),
            )
    )
}

pub fn route_key(prompt: &PromptKeyState, key: Key, modifiers: Modifiers) -> Option<Message> {
    match prompt.top_modal() {
        Some(modal) => modal_key(prompt, modal, key, modifiers),
        None => keys_without_modal(prompt, key, modifiers),
    }
}

/// The create-directory prompt takes every key: it types the name.
fn prompt_key(prompt: &PromptKeyState, key: &Key) -> Option<Message> {
    if prompt.busy {
        return None;
    }
    let mut name = prompt.typed.to_string();
    match key.as_ref() {
        Key::Named(Named::Enter) => Some(Message::Ops(OpsMessage::PromptSubmit)),
        Key::Named(Named::Escape) => Some(Message::Ops(OpsMessage::PromptCancel)),
        // Backspace edits the text; it must not walk to the parent directory.
        Key::Named(Named::Backspace) => {
            name.pop();
            Some(Message::Ops(OpsMessage::PromptInput(name)))
        }
        // No filtering: a directory name may contain anything but a path
        // separator, and `Prompt::validate` reports the rest.
        Key::Character(c) => {
            name.push_str(c);
            Some(Message::Ops(OpsMessage::PromptInput(name)))
        }
        // iced delivers the space bar as a named key, not as a character.
        Key::Named(Named::Space) => {
            name.push(' ');
            Some(Message::Ops(OpsMessage::PromptInput(name)))
        }
        _ => None,
    }
}

fn volume_menu_message(pressed: VolumeMenuKey) -> Message {
    match pressed {
        VolumeMenuKey::Close => Message::Panel(PanelMessage::VolumeMenuClose),
        VolumeMenuKey::Activate => Message::Panel(PanelMessage::VolumeMenuActivate),
        VolumeMenuKey::Move(delta) => Message::Panel(PanelMessage::VolumeMenuMove(delta)),
        VolumeMenuKey::First => Message::Panel(PanelMessage::VolumeMenuFirst),
        VolumeMenuKey::Last => Message::Panel(PanelMessage::VolumeMenuLast),
        VolumeMenuKey::Remove => Message::Panel(PanelMessage::VolumeMenuRemove),
    }
}

fn page_tab_message(key_modifiers: Modifiers) -> Option<Message> {
    if key_modifiers == Modifiers::SHIFT {
        Some(Message::TabInPage { backward: true })
    } else if key_modifiers.is_empty() {
        Some(Message::TabInPage { backward: false })
    } else {
        None
    }
}
