//! A press on a row that turns into a drag once the pointer has moved a few
//! pixels, so a plain click keeps selecting.

use iced::keyboard::Modifiers;
use iced::Point;

use crate::messages::PanelSide;

/// Logical pixels the pointer must travel before a press counts as a drag.
pub const DRAG_THRESHOLD: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelDrag {
    source: PanelSide,
    origin: Point,
    dragging: bool,
}

impl PanelDrag {
    pub fn new(source: PanelSide, origin: Point) -> Self {
        Self {
            source,
            origin,
            dragging: false,
        }
    }

    pub fn moved_to(&mut self, position: Point) {
        if self.origin.distance(position) >= DRAG_THRESHOLD {
            self.dragging = true;
        }
    }

    pub fn source(&self) -> PanelSide {
        self.source
    }

    pub fn is_dragging(&self) -> bool {
        self.dragging
    }
}

/// Cmd on macOS, as in the Finder; Shift on Windows and Linux, as in Explorer.
pub fn move_modifier_held(modifiers: Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        modifiers.command()
    } else {
        modifiers.shift()
    }
}

/// The pointer left the window: macOS and Windows keep reporting positions
/// beyond the view while a button is held.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn is_outside(position: Point, window: iced::Size) -> bool {
    position.x < 0.0
        || position.y < 0.0
        || position.x >= window.width
        || position.y >= window.height
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_becomes_a_drag_only_after_the_threshold() {
        let mut drag = PanelDrag::new(PanelSide::Left, Point::new(10.0, 10.0));
        drag.moved_to(Point::new(12.0, 11.0));
        assert!(!drag.is_dragging());
        drag.moved_to(Point::new(10.0, 16.0));
        assert!(drag.is_dragging());
    }

    #[test]
    fn a_drag_stays_a_drag_when_the_pointer_comes_back() {
        let mut drag = PanelDrag::new(PanelSide::Right, Point::new(10.0, 10.0));
        drag.moved_to(Point::new(40.0, 10.0));
        drag.moved_to(Point::new(10.0, 10.0));
        assert!(drag.is_dragging());
    }

    #[test]
    fn the_move_modifier_is_command_on_macos_and_shift_elsewhere() {
        let (held, other) = if cfg!(target_os = "macos") {
            (Modifiers::COMMAND, Modifiers::SHIFT)
        } else {
            (Modifiers::SHIFT, Modifiers::COMMAND)
        };
        assert!(move_modifier_held(held));
        assert!(!move_modifier_held(other));
        assert!(!move_modifier_held(Modifiers::empty()));
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn positions_beyond_the_view_are_outside() {
        let window = iced::Size::new(100.0, 50.0);
        assert!(!is_outside(Point::new(0.0, 0.0), window));
        assert!(!is_outside(Point::new(99.0, 49.0), window));
        assert!(is_outside(Point::new(-1.0, 10.0), window));
        assert!(is_outside(Point::new(10.0, -1.0), window));
        assert!(is_outside(Point::new(100.0, 10.0), window));
        assert!(is_outside(Point::new(10.0, 50.0), window));
    }
}
