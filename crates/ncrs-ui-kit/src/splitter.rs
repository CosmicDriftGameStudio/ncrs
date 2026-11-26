//! The bar between two panes that can be dragged: the width a pane has, how
//! far a drag or a key moves it, and the bar itself.

use iced::mouse::Interaction;
use iced::widget::{column, container, mouse_area, text, Space};
use iced::{alignment, Element, Length};
use ncrs_i18n::{fill_template, Language, Msg};

use crate::kit::{focused, tooltip};
use crate::palette::Palette;
use crate::theme::git::{self, SplitterLook};
use crate::theme::{font_size, size, spacing, vertical_rule};

/// What a press of an arrow key moves a pane by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// An arrow key.
    Fine,
    /// An arrow key with Shift.
    Coarse,
}

impl Step {
    pub const fn pixels(self) -> f32 {
        match self {
            Self::Fine => 16.0,
            Self::Coarse => 64.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Wider,
    Narrower,
}

/// How narrow and how wide a pane gets, and the width Home restores.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneBounds {
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

impl PaneBounds {
    pub const fn new(min: f32, max: f32, default: f32) -> Self {
        Self { min, max, default }
    }

    pub fn clamp(self, width: f32) -> f32 {
        width.clamp(self.min, self.max)
    }
}

/// The width of one pane, always inside its bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneWidth {
    bounds: PaneBounds,
    width: f32,
}

impl PaneWidth {
    /// A pane at its default width.
    pub fn new(bounds: PaneBounds) -> Self {
        Self::at(bounds, bounds.default)
    }

    /// A pane at `width`, brought into the bounds (a saved width may be out of
    /// date).
    pub fn at(bounds: PaneBounds, width: f32) -> Self {
        Self {
            bounds,
            width: bounds.clamp(width),
        }
    }

    pub const fn width(self) -> f32 {
        self.width
    }

    /// The pane after the pointer moved by `delta` pixels (negative: left).
    pub fn dragged(self, delta: f32) -> Self {
        Self::at(self.bounds, self.width + delta)
    }

    pub fn nudged(self, step: Step, direction: Direction) -> Self {
        let pixels = match direction {
            Direction::Wider => step.pixels(),
            Direction::Narrower => -step.pixels(),
        };
        self.dragged(pixels)
    }

    pub fn reset(self) -> Self {
        Self::new(self.bounds)
    }
}

/// What the bar sends. A press starts the drag; the caller then lays
/// [`crate::git_rows::drag_capture`] over the view and ends it on release.
#[derive(Debug, Clone)]
pub struct SplitterMessages<M> {
    pub press: M,
    /// A double click.
    pub reset: M,
    /// The pointer came onto the bar.
    pub hover: M,
    /// The pointer left the bar.
    pub leave: M,
}

/// The bar, [`size::SPLITTER`] wide and as high as its room: a thin line at
/// rest, so that it is not taken for a scrollbar, and a purple bar with a grip
/// while hovered or dragged. `has_focus` draws the keyboard focus around it.
pub fn splitter<'a, M: Clone + 'a>(
    look: SplitterLook,
    has_focus: bool,
    messages: SplitterMessages<M>,
    lang: Language,
    palette: &Palette,
) -> Element<'a, M> {
    let dot = || {
        container(Space::new())
            .width(size::SPLITTER - 2.0)
            .height(size::SPLITTER - 2.0)
            .style(git::splitter_dot(palette))
    };
    let content: Element<'a, M> = match look {
        SplitterLook::Idle => vertical_rule(palette.border_inactive),
        SplitterLook::Hovered | SplitterLook::Dragged => column![dot(), dot(), dot()]
            .spacing(spacing::XS)
            .align_x(alignment::Horizontal::Center)
            .into(),
    };
    let bar = container(content)
        .width(size::SPLITTER)
        .height(Length::Fill)
        .align_x(alignment::Horizontal::Center)
        .align_y(alignment::Vertical::Center)
        .style(git::splitter(palette, look));
    let area = mouse_area(focused(bar, has_focus, palette))
        .on_press(messages.press)
        .on_double_click(messages.reset)
        .on_enter(messages.hover)
        .on_exit(messages.leave)
        .interaction(Interaction::ResizingHorizontally);
    if look == SplitterLook::Dragged {
        return area.into();
    }
    tooltip(
        area,
        lang.text(Msg::SplitterHint),
        iced::widget::tooltip::Position::FollowCursor,
        palette,
    )
}

/// The small label that shows the width while the bar is dragged.
pub fn width_label<'a, M: 'a>(width: f32, lang: Language, palette: &Palette) -> Element<'a, M> {
    let words = fill_template(
        lang.text(Msg::SplitterWidth),
        &[("{width}", &format!("{width:.0}"))],
    );
    container(text(words).size(font_size::SMALL))
        .padding([spacing::XS, spacing::M])
        .style(crate::theme::dialog(palette))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOUNDS: PaneBounds = PaneBounds::new(200.0, 600.0, 420.0);

    #[test]
    fn a_width_stays_between_the_bounds() {
        assert_eq!(PaneWidth::at(BOUNDS, 100.0).width(), 200.0);
        assert_eq!(PaneWidth::at(BOUNDS, 900.0).width(), 600.0);
        let pane = PaneWidth::new(BOUNDS);
        assert_eq!(pane.dragged(-1000.0).width(), 200.0);
        assert_eq!(pane.dragged(1000.0).width(), 600.0);
    }

    #[test]
    fn an_arrow_moves_16_and_shift_arrow_64() {
        let pane = PaneWidth::new(BOUNDS);
        assert_eq!(pane.nudged(Step::Fine, Direction::Wider).width(), 436.0);
        assert_eq!(pane.nudged(Step::Fine, Direction::Narrower).width(), 404.0);
        assert_eq!(pane.nudged(Step::Coarse, Direction::Wider).width(), 484.0);
        assert_eq!(
            pane.nudged(Step::Coarse, Direction::Narrower).width(),
            356.0
        );
    }

    #[test]
    fn a_nudge_at_the_edge_stops_there() {
        let widest = PaneWidth::at(BOUNDS, 590.0);
        assert_eq!(widest.nudged(Step::Coarse, Direction::Wider).width(), 600.0);
    }

    #[test]
    fn reset_restores_the_default_width() {
        let pane = PaneWidth::at(BOUNDS, 333.0);
        assert_eq!(pane.reset().width(), 420.0);
    }

    #[test]
    fn a_drag_adds_the_pointer_delta() {
        let pane = PaneWidth::new(BOUNDS).dragged(30.0).dragged(-10.0);
        assert_eq!(pane.width(), 440.0);
    }
}
