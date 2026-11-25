//! The building blocks of the filing pages and the chat. Every size of the
//! pages lives here; the pages put the blocks together and never pick a size
//! themselves.

use iced::font::Weight;
use iced::widget::text::LineHeight;
use iced::widget::text::Wrapping;
use iced::widget::{container, text, Space};
use iced::{Background, Color, Element, Font, Length, Pixels, Theme};

pub const ROW_ONE_LINE: f32 = 28.0;
pub const ROW_TWO_LINE: f32 = 48.0;
pub const LABEL_WIDTH: f32 = 160.0;
pub const CONTROL_HEIGHT: f32 = 28.0;
pub const CHIP_HEIGHT: f32 = 20.0;
pub const PIECE_HEIGHT: f32 = 24.0;
pub const TEXT_SMALL: f32 = 12.0;
pub const TEXT_BODY: f32 = 14.0;
pub const TEXT_TITLE: f32 = 16.0;
pub const GAP_XS: f32 = crate::theme::spacing::XS;
pub const GAP_S: f32 = crate::theme::spacing::M;
pub const GAP_M: f32 = crate::theme::spacing::XL;
pub const GAP_L: f32 = crate::theme::spacing::XXL;
pub const SELECTION_BAR: f32 = 2.0;

const RADIUS: f32 = crate::theme::radius::CORNER;
const SMALL_LINE: f32 = 16.0;
const BODY_LINE: f32 = 18.0;
/// A control of `CONTROL_HEIGHT` is a line of 18 px with 5 px above and below.
const CONTROL_PAD_Y: f32 = 5.0;
const PIECE_PAD_Y: f32 = 3.0;
const ICON_PAD_X: f32 = 9.0;
const BUTTON_PAD_X: f32 = 12.0;
const MENU_PAD_X: f32 = 8.0;
const KEY_HINT_ALPHA: f32 = 0.7;
const ELLIPSIS: &str = "…";
const REMOVE: &str = "×";
const FILTER_BORDER: f32 = 1.0;
const SEPARATOR_HEIGHT: f32 = 1.0;

// The system mono font has no semibold face, so a semibold request picks whichever installed mono font has one.
fn semibold() -> Font {
    Font {
        weight: Weight::Bold,
        ..crate::APP_FONT
    }
}

fn small<'a, M: 'a>(
    content: impl iced::widget::text::IntoFragment<'a>,
    color: Color,
) -> Element<'a, M> {
    text(content)
        .size(TEXT_SMALL)
        .line_height(LineHeight::Absolute(Pixels(SMALL_LINE)))
        .color(color)
        .wrapping(Wrapping::None)
        .into()
}

fn body<'a, M: 'a>(
    content: impl iced::widget::text::IntoFragment<'a>,
    color: Color,
) -> Element<'a, M> {
    text(content)
        .size(TEXT_BODY)
        .line_height(LineHeight::Absolute(Pixels(BODY_LINE)))
        .color(color)
        .wrapping(Wrapping::None)
        .into()
}

fn fill_style(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(color)),
        ..container::Style::default()
    }
}

pub fn bar<'a, M: 'a>(color: Color) -> Element<'a, M> {
    container(Space::new())
        .width(SELECTION_BAR)
        .height(Length::Fill)
        .style(fill_style(color))
        .into()
}

mod buttons;
mod chat;
mod chips;
mod forms;
mod lists;
mod page;

pub use buttons::*;
pub use chat::*;
pub use chips::*;
pub use forms::*;
pub use lists::*;
use page::*;
