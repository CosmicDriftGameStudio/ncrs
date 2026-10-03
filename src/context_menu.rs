//! The context menu of a panel row: which entries it has, which are usable, and
//! the two input sources the keyboard route cannot see (Option tap, pointer).

use iced::keyboard::{self, key::Named, Key, Modifiers};
use iced::{event, mouse, window, Point, Size};

use crate::fs::OpenKind;
use crate::i18n::Msg;
use crate::messages::{ClipboardKind, Message, TransferKind};
use crate::ui::layout::{MENU_ITEM_HEIGHT, MENU_PADDING, MENU_SEPARATOR_HEIGHT, MENU_WIDTH};

/// What an entry does. Each one is exactly the message its key sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextAction {
    Open,
    View,
    Edit,
    Copy,
    Move,
    NewFolder,
    CopyPath,
    CopyName,
    Trash,
    DeletePermanently,
}

impl ContextAction {
    pub fn message(self) -> Message {
        match self {
            ContextAction::Open => Message::OpenSelected,
            ContextAction::View => Message::OpenExternal(OpenKind::View),
            ContextAction::Edit => Message::OpenExternal(OpenKind::Edit),
            ContextAction::Copy => Message::Transfer(TransferKind::Copy),
            ContextAction::Move => Message::Transfer(TransferKind::Move),
            ContextAction::NewFolder => Message::CreateDirPrompt,
            ContextAction::CopyPath => Message::CopyToClipboard(ClipboardKind::Path),
            ContextAction::CopyName => Message::CopyToClipboard(ClipboardKind::Name),
            ContextAction::Trash => Message::Delete { permanent: false },
            ContextAction::DeletePermanently => Message::Delete { permanent: true },
        }
    }

    pub fn label(self) -> Msg {
        match self {
            ContextAction::Open => Msg::ContextMenuOpen,
            ContextAction::View => Msg::ContextMenuView,
            ContextAction::Edit => Msg::ContextMenuEdit,
            ContextAction::Copy => Msg::ContextMenuCopy,
            ContextAction::Move => Msg::ContextMenuMove,
            ContextAction::NewFolder => Msg::ContextMenuNewFolder,
            ContextAction::CopyPath => Msg::ContextMenuCopyPath,
            ContextAction::CopyName => Msg::ContextMenuCopyName,
            ContextAction::Trash => Msg::ContextMenuTrash,
            ContextAction::DeletePermanently => Msg::ContextMenuDeletePermanently,
        }
    }
}

const GROUPS: [&[ContextAction]; 4] = [
    &[
        ContextAction::Open,
        ContextAction::View,
        ContextAction::Edit,
    ],
    &[
        ContextAction::Copy,
        ContextAction::Move,
        ContextAction::NewFolder,
    ],
    &[ContextAction::CopyPath, ContextAction::CopyName],
    &[ContextAction::Trash, ContextAction::DeletePermanently],
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuEntry {
    Separator,
    Item {
        action: ContextAction,
        enabled: bool,
    },
}

impl MenuEntry {
    fn is_usable(self) -> bool {
        matches!(self, MenuEntry::Item { enabled: true, .. })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContextMenu {
    origin: Point,
    entries: Vec<MenuEntry>,
    /// Index into `entries`; always a usable item.
    selected: usize,
}

impl ContextMenu {
    /// A menu next to `anchor` that stays inside `window`: it opens to the
    /// right and below, and folds to the left or up where there is no room.
    pub fn new(anchor: Point, window: Size, is_enabled: impl Fn(ContextAction) -> bool) -> Self {
        let mut entries = Vec::new();
        for (group_index, group) in GROUPS.iter().enumerate() {
            if group_index > 0 {
                entries.push(MenuEntry::Separator);
            }
            entries.extend(group.iter().map(|&action| MenuEntry::Item {
                action,
                enabled: is_enabled(action),
            }));
        }
        let selected = entries
            .iter()
            .position(|entry| entry.is_usable())
            .unwrap_or(0);
        let size = Self::size_of(&entries);
        Self {
            origin: Point::new(
                place(anchor.x, size.width, window.width),
                place(anchor.y, size.height, window.height),
            ),
            entries,
            selected,
        }
    }

    pub fn size_of(entries: &[MenuEntry]) -> Size {
        let height: f32 = entries
            .iter()
            .map(|entry| match entry {
                MenuEntry::Separator => MENU_SEPARATOR_HEIGHT,
                MenuEntry::Item { .. } => MENU_ITEM_HEIGHT,
            })
            .sum();
        Size::new(MENU_WIDTH, height + 2.0 * MENU_PADDING)
    }

    pub fn origin(&self) -> Point {
        self.origin
    }

    pub fn entries(&self) -> &[MenuEntry] {
        &self.entries
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn selected_action(&self) -> Option<ContextAction> {
        match self.entries.get(self.selected) {
            Some(MenuEntry::Item {
                action,
                enabled: true,
            }) => Some(*action),
            _ => None,
        }
    }

    /// Highlights the entry at `index` if it can be used.
    pub fn select(&mut self, index: usize) {
        if self
            .entries
            .get(index)
            .is_some_and(|entry| entry.is_usable())
        {
            self.selected = index;
        }
    }

    /// Moves to the next usable entry in the direction of `delta`, skipping
    /// separators and greyed-out ones; stays put at either end.
    pub fn move_by(&mut self, delta: isize) {
        let candidates: Vec<usize> = if delta < 0 {
            (0..self.selected).rev().collect()
        } else {
            (self.selected + 1..self.entries.len()).collect()
        };
        if let Some(next) = candidates
            .into_iter()
            .find(|&index| self.entries.get(index).is_some_and(|e| e.is_usable()))
        {
            self.selected = next;
        }
    }

    pub fn select_first(&mut self) {
        if let Some(index) = self.entries.iter().position(|entry| entry.is_usable()) {
            self.selected = index;
        }
    }

    pub fn select_last(&mut self) {
        if let Some(index) = self.entries.iter().rposition(|entry| entry.is_usable()) {
            self.selected = index;
        }
    }
}

/// Where a span of `length` starts so it stays within `0..limit`: at `anchor`,
/// or ending there when it would not fit.
fn place(anchor: f32, length: f32, limit: f32) -> f32 {
    let start = if anchor + length > limit {
        anchor - length
    } else {
        anchor
    };
    start.min(limit - length).max(0.0)
}

/// A tap of Option (Alt) on its own: pressed and released with no other key,
/// modifier or mouse button in between.
#[derive(Debug, Default)]
pub struct AltTap {
    armed: bool,
    held: Modifiers,
}

impl AltTap {
    /// Returns true when the release completes a tap.
    pub fn modifiers_changed(&mut self, now: Modifiers) -> bool {
        let mut tapped = false;
        if now == Modifiers::ALT {
            if self.held.is_empty() {
                self.armed = true;
            }
        } else if now.is_empty() {
            tapped = self.armed;
            self.armed = false;
        } else {
            self.armed = false;
        }
        self.held = now;
        tapped
    }

    /// Any key other than the Alt key itself spoils the tap, bound or not.
    pub fn key_pressed(&mut self, key: &Key) {
        if !matches!(key, Key::Named(Named::Alt | Named::AltGraph)) {
            self.armed = false;
        }
    }

    pub fn cancel(&mut self) {
        self.armed = false;
    }
}

/// What the window's raw events leave behind that no widget reports: where the
/// pointer is, and whether Option was just tapped.
#[derive(Debug, Default)]
pub struct InputState {
    pub pointer: Point,
    pub alt_tap: AltTap,
    /// The path field is being edited, so Escape and Tab leave it. Kept here
    /// because the text field captures those keys before the key routing.
    pub path_editing: bool,
}

impl InputState {
    pub fn handle(&mut self, event: &event::Event, status: event::Status) -> Option<Message> {
        match event {
            event::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                self.pointer = *position;
                None
            }
            event::Event::Mouse(mouse::Event::ButtonPressed(_)) => {
                self.alt_tap.cancel();
                // A press nothing took (not a row, not a menu, not the field)
                // is a click elsewhere.
                (status == event::Status::Ignored).then_some(Message::ClickedOutside)
            }
            event::Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) => {
                self.alt_tap.key_pressed(key);
                let leaves_field = matches!(key, Key::Named(Named::Escape | Named::Tab));
                (self.path_editing && leaves_field).then_some(Message::PathFieldCancel)
            }
            event::Event::Mouse(mouse::Event::WheelScrolled { .. }) => {
                self.alt_tap.cancel();
                None
            }
            event::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                // Still fed, so the tap's bookkeeping stays right, but a tap
                // while a path is being typed opens nothing.
                let tapped = self.alt_tap.modifiers_changed(*modifiers);
                (tapped && !self.path_editing).then_some(Message::ContextMenuKey)
            }
            event::Event::Window(window::Event::Unfocused) => {
                self.alt_tap.cancel();
                None
            }
            _ => None,
        }
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::{key::Physical, Location};

    const WINDOW: Size = Size::new(1200.0, 760.0);

    fn menu_with(disabled: &[ContextAction]) -> ContextMenu {
        ContextMenu::new(Point::new(10.0, 10.0), WINDOW, |action| {
            !disabled.contains(&action)
        })
    }

    fn selected_action(menu: &ContextMenu) -> ContextAction {
        menu.selected_action().unwrap()
    }

    fn key_pressed(key: Key) -> event::Event {
        event::Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
            location: Location::Standard,
            modifiers: Modifiers::ALT,
            text: None,
            repeat: false,
        })
    }

    fn modifiers(modifiers: Modifiers) -> event::Event {
        event::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers))
    }

    fn feed(input: &mut InputState, events: &[event::Event]) -> Vec<Message> {
        events
            .iter()
            .filter_map(|event| input.handle(event, event::Status::Ignored))
            .collect()
    }

    #[test]
    fn the_menu_has_the_four_groups_with_separators() {
        let menu = menu_with(&[]);
        let separators = menu
            .entries()
            .iter()
            .filter(|entry| **entry == MenuEntry::Separator)
            .count();
        assert_eq!(separators, 3);
        assert_eq!(menu.entries().len(), 10 + 3);
        assert_eq!(selected_action(&menu), ContextAction::Open);
    }

    #[test]
    fn navigation_skips_separators_and_greyed_out_entries() {
        let mut menu = menu_with(&[ContextAction::View, ContextAction::Edit]);
        menu.move_by(1);
        assert_eq!(selected_action(&menu), ContextAction::Copy);
        menu.move_by(-1);
        assert_eq!(selected_action(&menu), ContextAction::Open);
        menu.move_by(-1);
        assert_eq!(
            selected_action(&menu),
            ContextAction::Open,
            "stays at the top"
        );
        menu.select_last();
        assert_eq!(selected_action(&menu), ContextAction::DeletePermanently);
        menu.move_by(1);
        assert_eq!(selected_action(&menu), ContextAction::DeletePermanently);
        menu.move_by(-1);
        assert_eq!(selected_action(&menu), ContextAction::Trash);
        menu.move_by(-1);
        assert_eq!(selected_action(&menu), ContextAction::CopyName);
        menu.select_first();
        assert_eq!(selected_action(&menu), ContextAction::Open);
    }

    #[test]
    fn only_usable_entries_can_be_highlighted() {
        let mut menu = menu_with(&[ContextAction::View]);
        menu.select(1);
        assert_eq!(selected_action(&menu), ContextAction::Open);
        menu.select(4);
        assert_eq!(selected_action(&menu), ContextAction::Copy);
        let separator = menu
            .entries()
            .iter()
            .position(|entry| *entry == MenuEntry::Separator)
            .unwrap();
        menu.select(separator);
        assert_eq!(selected_action(&menu), ContextAction::Copy);
    }

    #[test]
    fn the_first_usable_entry_is_highlighted_when_open_is_unusable() {
        let menu = menu_with(&[ContextAction::Open, ContextAction::View]);
        assert_eq!(selected_action(&menu), ContextAction::Edit);
    }

    #[test]
    fn the_menu_folds_back_into_the_window() {
        let size = ContextMenu::size_of(menu_with(&[]).entries());
        let corner = ContextMenu::new(
            Point::new(WINDOW.width - 5.0, WINDOW.height - 5.0),
            WINDOW,
            |_| true,
        );
        assert_eq!(
            corner.origin(),
            Point::new(
                WINDOW.width - 5.0 - size.width,
                WINDOW.height - 5.0 - size.height
            )
        );
        let inside = ContextMenu::new(Point::new(40.0, 50.0), WINDOW, |_| true);
        assert_eq!(inside.origin(), Point::new(40.0, 50.0));
        let tiny = ContextMenu::new(Point::new(40.0, 50.0), Size::new(100.0, 100.0), |_| true);
        assert_eq!(tiny.origin(), Point::ORIGIN);
    }

    #[test]
    fn each_entry_sends_the_message_of_its_key() {
        assert_eq!(ContextAction::Open.message(), Message::OpenSelected);
        assert_eq!(
            ContextAction::Trash.message(),
            Message::Delete { permanent: false }
        );
        assert_eq!(
            ContextAction::DeletePermanently.message(),
            Message::Delete { permanent: true }
        );
        for action in GROUPS.iter().flat_map(|group| group.iter()) {
            assert!(
                crate::keymap::Keymap::built_in()
                    .shortcut_label(&action.message())
                    .is_some(),
                "{action:?} has no key to show"
            );
        }
    }

    #[test]
    fn tapping_option_opens_the_menu() {
        let mut input = InputState::default();
        let messages = feed(
            &mut input,
            &[
                modifiers(Modifiers::ALT),
                key_pressed(Key::Named(Named::Alt)),
                modifiers(Modifiers::empty()),
            ],
        );
        assert_eq!(messages, [Message::ContextMenuKey]);
    }

    #[test]
    fn option_with_any_key_does_not_open_it() {
        for key in [
            Key::Named(Named::F1),
            Key::Character("c".into()),
            Key::Named(Named::F11),
            Key::Named(Named::Super),
        ] {
            let mut input = InputState::default();
            let messages = feed(
                &mut input,
                &[
                    modifiers(Modifiers::ALT),
                    key_pressed(key.clone()),
                    modifiers(Modifiers::empty()),
                ],
            );
            assert!(messages.is_empty(), "{key:?} did not cancel the tap");
        }
    }

    #[test]
    fn option_with_another_modifier_does_not_open_it() {
        let mut input = InputState::default();
        let with_alt_first = feed(
            &mut input,
            &[
                modifiers(Modifiers::ALT),
                modifiers(Modifiers::ALT | Modifiers::SHIFT),
                modifiers(Modifiers::ALT),
                modifiers(Modifiers::empty()),
            ],
        );
        assert!(with_alt_first.is_empty());

        let with_shift_first = feed(
            &mut input,
            &[
                modifiers(Modifiers::SHIFT),
                modifiers(Modifiers::SHIFT | Modifiers::ALT),
                modifiers(Modifiers::SHIFT),
                modifiers(Modifiers::empty()),
            ],
        );
        assert!(with_shift_first.is_empty());
    }

    #[test]
    fn option_with_a_mouse_click_does_not_open_it() {
        let mut input = InputState::default();
        let _ = feed(&mut input, &[modifiers(Modifiers::ALT)]);
        let click = event::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        assert_eq!(
            input.handle(&click, event::Status::Captured),
            None,
            "a click on a row is not a click beside the menu"
        );
        assert!(feed(&mut input, &[modifiers(Modifiers::empty())]).is_empty());
    }

    #[test]
    fn losing_focus_while_option_is_held_does_not_open_it_later() {
        let mut input = InputState::default();
        let _ = feed(&mut input, &[modifiers(Modifiers::ALT)]);
        let _ = feed(
            &mut input,
            &[event::Event::Window(window::Event::Unfocused)],
        );
        assert!(feed(&mut input, &[modifiers(Modifiers::empty())]).is_empty());
    }

    #[test]
    fn a_wheel_turn_while_option_is_held_spoils_the_tap() {
        let mut input = InputState::default();
        let wheel = event::Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
        });
        let messages = feed(
            &mut input,
            &[
                modifiers(Modifiers::ALT),
                wheel,
                modifiers(Modifiers::empty()),
            ],
        );
        assert!(messages.is_empty(), "{messages:?}");
    }

    #[test]
    fn a_tap_while_the_path_is_edited_opens_nothing() {
        let mut input = InputState {
            path_editing: true,
            ..InputState::default()
        };
        let while_editing = feed(
            &mut input,
            &[modifiers(Modifiers::ALT), modifiers(Modifiers::empty())],
        );
        assert!(while_editing.is_empty(), "{while_editing:?}");
        input.path_editing = false;
        let after_editing = feed(
            &mut input,
            &[modifiers(Modifiers::ALT), modifiers(Modifiers::empty())],
        );
        assert_eq!(
            after_editing,
            [Message::ContextMenuKey],
            "the next tap still works"
        );
    }

    #[test]
    fn the_pointer_position_is_remembered_and_unclaimed_clicks_close() {
        let mut input = InputState::default();
        let moved = event::Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(12.0, 34.0),
        });
        assert_eq!(input.handle(&moved, event::Status::Ignored), None);
        assert_eq!(input.pointer, Point::new(12.0, 34.0));
        let click = event::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right));
        assert_eq!(
            input.handle(&click, event::Status::Ignored),
            Some(Message::ClickedOutside)
        );
    }

    #[test]
    fn escape_and_tab_leave_the_path_field_whatever_captured_them() {
        let mut input = InputState::default();
        let escape = key_pressed(Key::Named(Named::Escape));
        assert_eq!(input.handle(&escape, event::Status::Captured), None);
        input.path_editing = true;
        for key in [Named::Escape, Named::Tab] {
            assert_eq!(
                input.handle(&key_pressed(Key::Named(key)), event::Status::Captured),
                Some(Message::PathFieldCancel)
            );
        }
        let letter = key_pressed(Key::Character("a".into()));
        assert_eq!(input.handle(&letter, event::Status::Ignored), None);
    }
}
