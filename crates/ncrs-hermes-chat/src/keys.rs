use iced::keyboard::{key::Named, Key, Modifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatKey {
    Close,
    /// Closes the chat and opens the command line.
    CloseToCommandLine,
    /// Not the chat's: the app handles it.
    Pass,
    /// A function key the chat does not use; nothing else sees it either.
    Swallow,
    /// A key for the input.
    Type,
}

const FUNCTION_KEYS: [Named; 12] = [
    Named::F1,
    Named::F2,
    Named::F3,
    Named::F4,
    Named::F5,
    Named::F6,
    Named::F7,
    Named::F8,
    Named::F9,
    Named::F10,
    Named::F11,
    Named::F12,
];

pub fn chat_key(key: &Key, modifiers: Modifiers) -> ChatKey {
    match key.as_ref() {
        Key::Named(Named::Escape) => ChatKey::Close,
        Key::Named(Named::F2) if modifiers.shift() => ChatKey::CloseToCommandLine,
        Key::Named(Named::F2) => ChatKey::Close,
        Key::Named(Named::F10) => ChatKey::Pass,
        Key::Named(named) if FUNCTION_KEYS.contains(&named) => ChatKey::Swallow,
        _ => ChatKey::Type,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(key: Named, modifiers: Modifiers) -> ChatKey {
        chat_key(&Key::Named(key), modifiers)
    }

    #[test]
    fn escape_and_f2_close_and_shift_f2_goes_to_the_command_line() {
        assert_eq!(named(Named::Escape, Modifiers::empty()), ChatKey::Close);
        assert_eq!(named(Named::F2, Modifiers::empty()), ChatKey::Close);
        assert_eq!(
            named(Named::F2, Modifiers::SHIFT),
            ChatKey::CloseToCommandLine
        );
    }

    #[test]
    fn f10_passes_other_function_keys_are_swallowed_letters_type() {
        assert_eq!(named(Named::F10, Modifiers::empty()), ChatKey::Pass);
        assert_eq!(named(Named::F5, Modifiers::empty()), ChatKey::Swallow);
        assert_eq!(
            chat_key(&Key::Character("a".into()), Modifiers::empty()),
            ChatKey::Type
        );
    }
}
