//! The two input sources the keyboard route cannot see: the Option tap and the
//! pointer.

use iced::keyboard::{self, key::Named, Key, Modifiers};
use iced::{event, mouse, window, Point};

use crate::messages::PanelMessage;
use crate::messages::{
    CommandLineMessage, CommandPaletteMessage, KeyBindingsMessage, Message, QuickOpenMessage,
    ReorganisationPlanMessage, ReviewListMessage, RuleSuggestionsMessage, RulesMessage,
    SavedQueriesMessage, SearchMessage, SettingsMessage, TagsMessage, ThemeFetchMessage,
};
use crate::transfer_dialog::TransferDialogMessage;

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
    /// The command line has the focus: its text field captures Escape before
    /// the key routing.
    pub command_line: bool,
    /// The search view is up and nothing is on top of it: Escape closes it,
    /// and an Option tap opens no menu on the panel it hides.
    pub search_open: bool,
    /// A job is running, so Escape stops it before it closes the search.
    pub job_running: bool,
    /// Quick Open is up: Escape closes it, even with a job running, and an
    /// Option tap opens no menu.
    pub quick_open: bool,
    /// The command palette is up: same as Quick Open.
    pub command_palette: bool,
    /// The keyboard shortcuts dialog is up and records no key: same as Quick
    /// Open. While it records, Escape is the routing's, which cancels.
    pub key_bindings: bool,
    /// The theme download card is up: same as Quick Open.
    pub theme_fetch: bool,
    /// The rule suggestions view is up: same as Quick Open.
    pub rule_suggestions: bool,
    /// The rules page is up: same as Quick Open.
    pub rules: bool,
    pub review_list: bool,
    pub saved_queries: bool,
    /// The tags page is up: same as Quick Open.
    pub tags: bool,
    /// The document search page is up: same as Quick Open.
    /// The sidebar of the Hermes card has the arrows, not the page.
    pub hermes_sidebar: bool,
    /// The copy or move confirmation is up: its target field takes Escape.
    pub transfer_dialog: bool,
    /// The reorganisation plan view is up: same as Quick Open.
    pub reorganisation_plan: bool,
    /// The settings tab is up with nothing on top: Escape closes it, even from
    /// its search field, and an Option tap opens no menu.
    pub settings: bool,
    /// A row is held with the left button: the pointer and the button decide
    /// whether it becomes a drag.
    pub panel_drag_armed: bool,
}

impl InputState {
    /// The overlay Escape closes, first match wins: the order is the stacking
    /// order, so an overlay on top closes before the one below it.
    fn closes_overlay(&self, key: &Key) -> Option<Message> {
        if !matches!(key, Key::Named(Named::Escape)) {
            return None;
        }
        [
            (
                self.command_line,
                Message::CommandLine(CommandLineMessage::Leave),
            ),
            (self.quick_open, Message::QuickOpen(QuickOpenMessage::Close)),
            (
                self.command_palette,
                Message::CommandPalette(CommandPaletteMessage::Close),
            ),
            (
                self.key_bindings,
                Message::KeyBindings(KeyBindingsMessage::Close),
            ),
            (
                self.theme_fetch,
                Message::ThemeFetch(ThemeFetchMessage::Close),
            ),
            (
                self.rule_suggestions,
                Message::RuleSuggestions(RuleSuggestionsMessage::Close),
            ),
            (self.rules, Message::Rules(RulesMessage::Close)),
            (
                self.review_list,
                Message::ReviewList(ReviewListMessage::Close),
            ),
            (
                self.transfer_dialog,
                Message::TransferDialog(TransferDialogMessage::Cancel),
            ),
            (
                self.saved_queries,
                Message::SavedQueries(SavedQueriesMessage::Close),
            ),
            (self.tags, Message::Tags(TagsMessage::Close)),
            (
                self.reorganisation_plan,
                Message::ReorganisationPlan(ReorganisationPlanMessage::Close),
            ),
            (self.settings, Message::Settings(SettingsMessage::Close)),
        ]
        .into_iter()
        .find_map(|(open, close)| open.then_some(close))
    }

    pub fn handle(&mut self, event: &event::Event, status: event::Status) -> Option<Message> {
        match event {
            event::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                self.pointer = *position;
                self.panel_drag_armed
                    .then_some(Message::Panel(PanelMessage::PanelDragMoved(*position)))
            }
            event::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if self.panel_drag_armed =>
            {
                Some(Message::Panel(PanelMessage::PanelDragReleased))
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
                if self.path_editing && leaves_field {
                    return Some(Message::PathFieldCancel);
                }
                if let Some(close) = self.closes_overlay(key) {
                    return Some(close);
                }
                // The search's text fields capture Escape, so the routing
                // never sees it there.
                if !(self.search_open && matches!(key, Key::Named(Named::Escape))) {
                    return None;
                }
                Some(if self.job_running {
                    Message::AbortJob
                } else {
                    Message::Search(SearchMessage::Close)
                })
            }
            event::Event::Mouse(mouse::Event::WheelScrolled { .. }) => {
                self.alt_tap.cancel();
                None
            }
            event::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                // Still fed, so the tap's bookkeeping stays right, but a tap
                // while a path is being typed opens nothing.
                let tapped = self.alt_tap.modifiers_changed(*modifiers);
                (tapped
                    && !self.path_editing
                    && !self.command_line
                    && !self.search_open
                    && !self.quick_open
                    && !self.command_palette
                    && !self.key_bindings
                    && !self.theme_fetch
                    && !self.rule_suggestions
                    && !self.rules
                    && !self.review_list
                    && !self.saved_queries
                    && !self.tags
                    && !self.transfer_dialog
                    && !self.reorganisation_plan
                    && !self.settings)
                    .then_some(Message::ContextMenuKey)
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
            Key::Named(Named::ArrowLeft),
            Key::Named(Named::ArrowRight),
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
    fn escape_closes_the_search_whatever_captured_it_and_a_tap_opens_no_menu_over_it() {
        let mut input = InputState::default();
        let escape = key_pressed(Key::Named(Named::Escape));
        assert_eq!(input.handle(&escape, event::Status::Captured), None);

        input.search_open = true;
        for status in [event::Status::Captured, event::Status::Ignored] {
            assert_eq!(
                input.handle(&escape, status),
                Some(Message::Search(SearchMessage::Close))
            );
        }
        input.job_running = true;
        assert_eq!(
            input.handle(&escape, event::Status::Ignored),
            Some(Message::AbortJob),
            "a running job is stopped before the search is closed"
        );
        input.job_running = false;
        let letter = key_pressed(Key::Character("a".into()));
        assert_eq!(input.handle(&letter, event::Status::Ignored), None);
        let tap = feed(
            &mut input,
            &[modifiers(Modifiers::ALT), modifiers(Modifiers::empty())],
        );
        assert!(tap.is_empty(), "{tap:?}");
    }

    #[test]
    fn escape_closes_quick_open_even_with_a_job_running_and_a_tap_opens_no_menu() {
        let mut input = InputState::default();
        let escape = key_pressed(Key::Named(Named::Escape));
        input.quick_open = true;
        input.job_running = true;
        for status in [event::Status::Captured, event::Status::Ignored] {
            assert_eq!(
                input.handle(&escape, status),
                Some(Message::QuickOpen(QuickOpenMessage::Close))
            );
        }
        let tap = feed(
            &mut input,
            &[modifiers(Modifiers::ALT), modifiers(Modifiers::empty())],
        );
        assert!(tap.is_empty(), "{tap:?}");
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
    fn escape_leaves_the_command_line_and_tab_does_not() {
        let mut input = InputState {
            command_line: true,
            ..InputState::default()
        };
        assert_eq!(
            input.handle(
                &key_pressed(Key::Named(Named::Escape)),
                event::Status::Captured
            ),
            Some(Message::CommandLine(CommandLineMessage::Leave))
        );
        assert_eq!(
            input.handle(
                &key_pressed(Key::Named(Named::Tab)),
                event::Status::Captured
            ),
            None
        );
    }

    #[test]
    fn a_tap_while_the_command_line_has_the_focus_opens_nothing() {
        let mut input = InputState {
            command_line: true,
            ..InputState::default()
        };
        let tap = [modifiers(Modifiers::ALT), modifiers(Modifiers::empty())];
        assert!(feed(&mut input, &tap).is_empty());
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
