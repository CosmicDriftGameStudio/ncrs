//! What a key does while a menu of the panels is open, and which typed
//! character jumps to a file.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use crate::message::PanelMessage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMenuKey {
    CloseSub,
    Close,
    OpenSub,
    Activate,
    Move(isize),
    First,
    Last,
}

/// The keys of the open context menu; Left and Right walk between the menu
/// and its Hermes submenu.
pub fn context_menu_key(submenu_is_open: bool, key: &Key) -> Option<ContextMenuKey> {
    match key.as_ref() {
        Key::Named(Named::Escape) if submenu_is_open => Some(ContextMenuKey::CloseSub),
        Key::Named(Named::Escape) => Some(ContextMenuKey::Close),
        Key::Named(Named::ArrowRight) => Some(ContextMenuKey::OpenSub),
        Key::Named(Named::ArrowLeft) => Some(ContextMenuKey::CloseSub),
        Key::Named(Named::Enter) => Some(ContextMenuKey::Activate),
        Key::Named(Named::ArrowUp) => Some(ContextMenuKey::Move(-1)),
        Key::Named(Named::ArrowDown) => Some(ContextMenuKey::Move(1)),
        Key::Named(Named::Home) => Some(ContextMenuKey::First),
        Key::Named(Named::End) => Some(ContextMenuKey::Last),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeMenuKey {
    Close,
    Activate,
    Move(isize),
    First,
    Last,
    Remove,
}

/// The keys of the drive menu; any other key does nothing until it is closed.
pub fn volume_menu_key(key: &Key) -> Option<VolumeMenuKey> {
    match key.as_ref() {
        Key::Named(Named::Escape) => Some(VolumeMenuKey::Close),
        Key::Named(Named::Enter) => Some(VolumeMenuKey::Activate),
        Key::Named(Named::ArrowUp) => Some(VolumeMenuKey::Move(-1)),
        Key::Named(Named::ArrowDown) => Some(VolumeMenuKey::Move(1)),
        Key::Named(Named::Home) => Some(VolumeMenuKey::First),
        Key::Named(Named::End) => Some(VolumeMenuKey::Last),
        Key::Named(Named::Delete | Named::Backspace) => Some(VolumeMenuKey::Remove),
        _ => None,
    }
}

/// A printable character with at most Shift held. With Option held a Mac types
/// another character, so Alt, Ctrl and Cmd all rule it out.
pub fn type_ahead_message(key: &Key, modifiers: Modifiers) -> Option<PanelMessage> {
    let Key::Character(typed) = key else {
        return None;
    };
    ((modifiers - Modifiers::SHIFT).is_empty() && typed.chars().all(|c| !c.is_control()))
        .then(|| PanelMessage::TypeAhead(typed.to_string()))
}
