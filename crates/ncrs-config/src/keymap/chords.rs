use super::*;

/// More keys than anyone binds to one action; a bound on what a config file
/// can make the lookup walk through.
pub const MAX_KEYS_PER_ACTION: usize = 16;

/// Invisible formatting characters (Unicode category Cf): a key written with
/// one would look like another in the error line and the status bar.
pub fn is_format_character(c: char) -> bool {
    matches!(c,
        '\u{AD}'
        | '\u{600}'..='\u{605}'
        | '\u{61C}'
        | '\u{6DD}'
        | '\u{70F}'
        | '\u{180E}'
        | '\u{200B}'..='\u{200F}'
        | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{206F}'
        | '\u{FEFF}'
        | '\u{FFF9}'..='\u{FFFB}')
}

/// A key spelled for this platform: `F5`, `⇧F8`, `⌥⌘C` on a Mac,
/// `Ctrl+Alt+C` elsewhere. `None` for a key that has no printable name.
pub fn binding_label(binding: &Binding) -> Option<String> {
    let Binding { key, modifiers } = binding;
    let key_name = match key {
        Key::Named(named) => format!("{named:?}"),
        Key::Character(character) => character.to_uppercase(),
        Key::Unidentified => return None,
    };
    let mut label = String::new();
    if cfg!(target_os = "macos") {
        for (held, symbol) in [
            (modifiers.control(), "⌃"),
            (modifiers.alt(), "⌥"),
            (modifiers.shift(), "⇧"),
            (modifiers.logo(), "⌘"),
        ] {
            if held {
                label.push_str(symbol);
            }
        }
        label.push_str(&key_name);
    } else {
        for (held, name) in [
            (modifiers.control(), "Ctrl"),
            (modifiers.alt(), "Alt"),
            (modifiers.shift(), "Shift"),
            (modifiers.logo(), "Super"),
        ] {
            if held {
                label.push_str(name);
                label.push('+');
            }
        }
        label.push_str(&key_name);
    }
    Some(label)
}

/// The keys `action` ships with, the first-listed first.
pub fn built_in_bindings_of(action: KeyAction) -> Vec<Binding> {
    default_bindings()
        .into_iter()
        .filter(|(bound, _)| *bound == action)
        .map(|(_, binding)| binding)
        .collect()
}

/// The binding a key pressed to be recorded stands for, with the Shift rules
/// the lookup applies: a letter keeps Shift and is written in lower case, any
/// other character already carries Shift in itself (`*` is Shift+8), a named
/// key keeps it (Shift+F8 is not F8). `None` while only a modifier is down.
pub fn recorded_binding(key: &Key, modifiers: Modifiers) -> Option<Binding> {
    use key::Named;
    match key {
        Key::Named(
            Named::Shift
            | Named::Control
            | Named::Alt
            | Named::AltGraph
            | Named::Super
            | Named::Meta
            | Named::Hyper
            | Named::Fn
            | Named::FnLock
            | Named::CapsLock
            | Named::NumLock
            | Named::ScrollLock
            | Named::Symbol
            | Named::SymbolLock,
        )
        | Key::Unidentified => None,
        Key::Named(_) => Some(Binding::with_modifiers(key.clone(), modifiers)),
        Key::Character(typed) => {
            let held = if typed.chars().all(char::is_alphabetic) {
                modifiers
            } else {
                modifiers - Modifiers::SHIFT
            };
            Some(Binding::with_modifiers(
                Key::Character(typed.to_lowercase().into()),
                held,
            ))
        }
    }
}

/// Why a recorded key cannot be written to `[keys]` for an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRefusal {
    /// The key has no name in the config file, e.g. F13 or a media key.
    Unnamed,
    /// A letter or digit without Ctrl, Alt or Cmd would shadow type-ahead.
    NeedsModifier,
    /// Escape closes dialogs and menus and stops jobs.
    Escape,
    /// The search view handles the key itself.
    FixedInSearch,
}

/// The text `[keys]` stores for `binding` when `action` takes it, checked
/// against the rules [`Keymap::with_overrides`] applies to a single key: the
/// text reads back as exactly this binding.
pub fn configurable_chord(action: KeyAction, binding: &Binding) -> Result<String, KeyRefusal> {
    if binding.key == Key::Named(key::Named::Escape) {
        return Err(KeyRefusal::Escape);
    }
    let text = chord_text(binding).ok_or(KeyRefusal::Unnamed)?;
    match parse_chord(&text) {
        Ok(parsed) if parsed == *binding => {}
        Ok(_) => return Err(KeyRefusal::Unnamed),
        Err(_) => {
            let types_a_name = matches!(&binding.key, Key::Character(character)
                if character.chars().all(char::is_alphanumeric));
            return Err(if types_a_name {
                KeyRefusal::NeedsModifier
            } else {
                KeyRefusal::Unnamed
            });
        }
    }
    if !action.works_in(KeyScope::Panel) && is_fixed_in_search(binding) {
        return Err(KeyRefusal::FixedInSearch);
    }
    Ok(text)
}

/// `binding` as `[keys]` writes it, the inverse of [`parse_chord`]: the
/// platform's command modifier is `primary`, so the file means the same key
/// on a Mac and elsewhere.
pub fn chord_text(binding: &Binding) -> Option<String> {
    use key::Named;
    let mut parts: Vec<String> = Vec::new();
    let mut rest = binding.modifiers;
    if rest.contains(Modifiers::COMMAND) {
        parts.push("primary".to_owned());
        rest -= Modifiers::COMMAND;
    }
    for (modifier, name) in [
        (Modifiers::CTRL, "ctrl"),
        (Modifiers::ALT, "alt"),
        (Modifiers::SHIFT, "shift"),
        (Modifiers::LOGO, "super"),
    ] {
        if rest.contains(modifier) {
            parts.push(name.to_owned());
        }
    }
    let key_name = match &binding.key {
        Key::Character(character) if character.as_str() == "+" => "plus".to_owned(),
        Key::Character(character) => character.to_string(),
        Key::Named(named) => match named {
            Named::F1 => "f1",
            Named::F2 => "f2",
            Named::F3 => "f3",
            Named::F4 => "f4",
            Named::F5 => "f5",
            Named::F6 => "f6",
            Named::F7 => "f7",
            Named::F8 => "f8",
            Named::F9 => "f9",
            Named::F10 => "f10",
            Named::F11 => "f11",
            Named::F12 => "f12",
            Named::Enter => "enter",
            Named::Tab => "tab",
            Named::Backspace => "backspace",
            Named::Space => "space",
            Named::Insert => "insert",
            Named::Delete => "delete",
            Named::Home => "home",
            Named::End => "end",
            Named::PageUp => "pageup",
            Named::PageDown => "pagedown",
            Named::ArrowUp => "up",
            Named::ArrowDown => "down",
            Named::ArrowLeft => "left",
            Named::ArrowRight => "right",
            _ => return None,
        }
        .to_owned(),
        Key::Unidentified => return None,
    };
    parts.push(key_name);
    Some(parts.join("+"))
}

/// The number of a bare function key, 1 to 10.
pub fn function_key_number(key: &Key) -> Option<usize> {
    use key::Named::*;
    let Key::Named(named) = key else {
        return None;
    };
    [F1, F2, F3, F4, F5, F6, F7, F8, F9, F10]
        .iter()
        .position(|candidate| candidate == named)
        .map(|index| index + 1)
}

/// Reads one key of the config file: `f5`, `ctrl+alt+p`, `primary+q`.
///
/// Parts are separated by `+`, case does not matter, the last one is the key
/// and the ones before it are modifiers. `primary` is Cmd on a Mac and Ctrl
/// elsewhere. A literal plus is written `plus`.
pub fn parse_chord(text: &str) -> Result<Binding, String> {
    if text.is_empty() {
        return Err("a key is empty".to_owned());
    }
    let mut parts: Vec<&str> = text.split('+').map(str::trim).collect();
    let key_part = parts.pop().unwrap_or_default();
    let mut modifiers = Modifiers::default();
    for part in parts {
        let modifier = match part.to_lowercase().as_str() {
            "primary" => Modifiers::COMMAND,
            "ctrl" | "control" => Modifiers::CTRL,
            "alt" | "option" => Modifiers::ALT,
            "shift" => Modifiers::SHIFT,
            "cmd" | "super" => Modifiers::LOGO,
            _ => return Err(format!("unknown modifier {part:?} in {text:?}")),
        };
        if modifiers.intersects(modifier) {
            return Err(format!("modifier {part:?} is given twice in {text:?}"));
        }
        modifiers |= modifier;
    }
    let key = parse_key_name(key_part)
        .ok_or_else(|| format!("unknown key {key_part:?} in {text:?}"))??;
    if let Key::Character(character) = &key {
        let is_letter = character.chars().all(char::is_alphabetic);
        if modifiers.shift() && !is_letter {
            return Err(
                "shift changes the character; write the character it types, e.g. \"*\"".to_owned(),
            );
        }
        let types_a_name = character.chars().all(char::is_alphanumeric);
        if types_a_name && !modifiers.intersects(Modifiers::CTRL | Modifiers::ALT | Modifiers::LOGO)
        {
            return Err("letters and digits need ctrl, alt or cmd: without them they jump to a file name (type-ahead)".to_owned());
        }
    }
    Ok(Binding::with_modifiers(key, modifiers))
}

/// The key a name stands for. `None` for a name that is not a key; `Some(Err)`
/// for one that is, but is not allowed.
pub fn parse_key_name(name: &str) -> Option<Result<Key, String>> {
    use key::Named;
    let lower = name.to_lowercase();
    let named = match lower.as_str() {
        "escape" | "esc" => {
            return Some(Err(
                "Escape is reserved: it closes dialogs and menus and stops jobs".to_owned(),
            ))
        }
        "f1" => Named::F1,
        "f2" => Named::F2,
        "f3" => Named::F3,
        "f4" => Named::F4,
        "f5" => Named::F5,
        "f6" => Named::F6,
        "f7" => Named::F7,
        "f8" => Named::F8,
        "f9" => Named::F9,
        "f10" => Named::F10,
        "f11" => Named::F11,
        "f12" => Named::F12,
        "enter" => Named::Enter,
        "tab" => Named::Tab,
        "backspace" => Named::Backspace,
        "space" => Named::Space,
        "insert" => Named::Insert,
        "delete" => Named::Delete,
        "home" => Named::Home,
        "end" => Named::End,
        "pageup" => Named::PageUp,
        "pagedown" => Named::PageDown,
        "up" => Named::ArrowUp,
        "down" => Named::ArrowDown,
        "left" => Named::ArrowLeft,
        "right" => Named::ArrowRight,
        "plus" => return Some(Ok(Key::Character("+".into()))),
        _ => {
            let mut chars = name.chars();
            return match (chars.next(), chars.next()) {
                (Some(only), None)
                    if !only.is_whitespace()
                        && !only.is_control()
                        && !is_format_character(only) =>
                {
                    Some(Ok(Key::Character(lower.as_str().into())))
                }
                _ => None,
            };
        }
    };
    Some(Ok(Key::Named(named)))
}
