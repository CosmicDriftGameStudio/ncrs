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
    /// Cmd+N: a new chat.
    NewChat,
    /// Cmd+Up: the chat above in the list.
    PreviousChat,
    /// Cmd+Down: the chat below in the list.
    NextChat,
    /// Space: marks the chat in the list. The input types it as well; the
    /// chat only marks when the input is empty.
    MarkChat,
    /// Del: deletes the marked chats; only when the input is empty.
    DeleteChat,
    /// Cmd+Enter: the button of the Platz card that deletes all safe ones.
    SpaceDeleteSafe,
    /// Cmd+Shift+Enter: the button of the Platz card that removes worktrees.
    SpaceRemoveWorktrees,
    /// Cmd+O: the button of the Platz card that opens the overview.
    SpaceOverview,
    /// Cmd+L: back to the input.
    FocusInput,
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
    let command_only = modifiers.command() && !modifiers.shift() && !modifiers.alt();
    let command_shift = modifiers.command() && modifiers.shift() && !modifiers.alt();
    match key.as_ref() {
        Key::Named(Named::Enter) if command_only => ChatKey::SpaceDeleteSafe,
        Key::Named(Named::Enter) if command_shift => ChatKey::SpaceRemoveWorktrees,
        Key::Character("o") if command_only => ChatKey::SpaceOverview,
        Key::Character("l") if command_only => ChatKey::FocusInput,
        Key::Character("n") if command_only => ChatKey::NewChat,
        Key::Named(Named::ArrowUp) if command_only => ChatKey::PreviousChat,
        Key::Named(Named::ArrowDown) if command_only => ChatKey::NextChat,
        Key::Named(Named::Space) if modifiers.is_empty() => ChatKey::MarkChat,
        Key::Named(Named::Delete) if modifiers.is_empty() => ChatKey::DeleteChat,
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
    fn the_chat_list_has_its_keys() {
        let command = Modifiers::COMMAND;
        assert_eq!(
            chat_key(&Key::Character("n".into()), command),
            ChatKey::NewChat
        );
        assert_eq!(named(Named::ArrowUp, command), ChatKey::PreviousChat);
        assert_eq!(named(Named::ArrowDown, command), ChatKey::NextChat);
        assert_eq!(named(Named::Space, Modifiers::empty()), ChatKey::MarkChat);
        assert_eq!(
            named(Named::Delete, Modifiers::empty()),
            ChatKey::DeleteChat
        );
        assert_eq!(named(Named::ArrowUp, Modifiers::empty()), ChatKey::Type);
    }

    #[test]
    fn the_space_card_has_its_keys_before_the_input_submits() {
        let command = Modifiers::COMMAND;
        assert_eq!(
            named(Named::Enter, command),
            ChatKey::SpaceDeleteSafe
        );
        assert_eq!(
            named(Named::Enter, command | Modifiers::SHIFT),
            ChatKey::SpaceRemoveWorktrees
        );
        assert_eq!(
            chat_key(&Key::Character("o".into()), command),
            ChatKey::SpaceOverview
        );
        assert_eq!(
            chat_key(&Key::Character("l".into()), command),
            ChatKey::FocusInput
        );
        assert_eq!(named(Named::Enter, Modifiers::empty()), ChatKey::Type);
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
