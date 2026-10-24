//! The frame every dialog is drawn in: title and close cross on top, the
//! content in a scrollable of fixed height, key hints and buttons below.
//!
//! The size comes from the window and the dialog's [`DialogSize`], never from
//! the content: a long file name or a longer list scrolls or is cut, the frame
//! stays where it is.

use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, rule, scrollable, text};
use iced::{alignment, Element, Length, Padding, Size};

use super::theme::{self, font_size, spacing};
use crate::i18n::{Language, Msg};
use crate::messages::Message;
use crate::palette::Palette;

/// Lets a test find the frame and read its bounds.
pub const FRAME_ID: &str = "dialog-frame";

/// The close cross. U+00D7 rather than U+2715: the UI font has no glyph for
/// the latter.
const CLOSE_GLYPH: &str = "×";

/// Space kept free between the frame and every edge of the window.
const WINDOW_MARGIN: f32 = 24.0;

/// Average width of one glyph at the UI font sizes, used to turn a width in
/// pixels into a number of characters for text that must stay on one line.
const AVERAGE_GLYPH_WIDTH: f32 = 7.0;

/// Room kept right of the content for the scrollbar drawn over it.
const SCROLLBAR_ROOM: f32 = 18.0;

const HINT_SEPARATOR: &str = " · ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogSize {
    /// Confirmations.
    Small,
    /// Forms, the master key, the setup.
    Medium,
    /// Lists and the Inspection.
    Large,
}

/// A share of the window, kept between a minimum and a maximum.
struct Extent {
    share: f32,
    min: f32,
    max: f32,
}

impl Extent {
    const fn new(share: f32, min: f32, max: f32) -> Self {
        Self { share, min, max }
    }

    /// Never more than the window minus the margin on both sides, even where
    /// that is less than the minimum.
    fn of(&self, window: f32) -> f32 {
        let room = (window - 2.0 * WINDOW_MARGIN).max(0.0);
        (window * self.share).clamp(self.min, self.max).min(room)
    }
}

impl DialogSize {
    const fn extents(self) -> (Extent, Extent) {
        match self {
            Self::Small => (
                Extent::new(0.36, 380.0, 520.0),
                Extent::new(0.34, 240.0, 320.0),
            ),
            Self::Medium => (
                Extent::new(0.50, 460.0, 680.0),
                Extent::new(0.62, 360.0, 600.0),
            ),
            Self::Large => (
                Extent::new(0.72, 600.0, 960.0),
                Extent::new(0.78, 440.0, 720.0),
            ),
        }
    }
}

/// The outer size of the frame of a dialog of `size` in a window of `window`.
pub fn frame_size(size: DialogSize, window: Size) -> Size {
    let (width, height) = size.extents();
    Size::new(width.of(window.width), height.of(window.height))
}

/// How many characters of one line fit into `width` pixels.
pub fn chars_that_fit(width: f32) -> usize {
    (width / AVERAGE_GLYPH_WIDTH).floor().max(1.0) as usize
}

/// The width of a line of content: the frame less its padding and the room
/// the scrollbar keeps.
pub fn content_width(size: DialogSize, window: Size) -> f32 {
    frame_size(size, window).width - 4.0 * spacing::OUTER_PADDING - SCROLLBAR_ROOM
}

/// The characters one line of the content of a frame holds, with `reserved`
/// pixels of the line taken by other things.
pub fn content_chars(size: DialogSize, window: Size, reserved: f32) -> usize {
    chars_that_fit(content_width(size, window) - reserved)
}

/// Which keys the footer names. Esc is named whenever the frame can be closed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyHints {
    /// Enter presses the primary button.
    pub enter: bool,
    /// Tab moves the focus between more than one element.
    pub tab: bool,
}

pub struct Frame<'a> {
    title: String,
    subtitle: Option<String>,
    content: Element<'a, Message>,
    buttons: Vec<Element<'a, Message>>,
    on_close: Option<Message>,
    size: DialogSize,
    window: Size,
    hints: KeyHints,
    lang: Language,
    palette: Palette,
}

impl<'a> Frame<'a> {
    pub fn new(
        title: impl Into<String>,
        content: impl Into<Element<'a, Message>>,
        size: DialogSize,
        window: Size,
        lang: Language,
        palette: &Palette,
    ) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            content: content.into(),
            buttons: Vec::new(),
            on_close: None,
            size,
            window,
            hints: KeyHints::default(),
            lang,
            palette: *palette,
        }
    }

    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Secondary buttons first, the primary one last: it ends up rightmost.
    pub fn buttons(mut self, buttons: Vec<Element<'a, Message>>) -> Self {
        self.buttons = buttons;
        self
    }

    /// What the close cross sends, which has to be what Esc does.
    pub fn on_close(mut self, message: Message) -> Self {
        self.on_close = Some(message);
        self
    }

    pub const fn hints(mut self, hints: KeyHints) -> Self {
        self.hints = hints;
        self
    }

    pub fn into_element(self) -> Element<'a, Message> {
        let palette = self.palette;
        let outer = frame_size(self.size, self.window);

        let mut header = row![text(self.title)
            .size(font_size::TITLE)
            .color(palette.accent)
            .wrapping(Wrapping::None)
            .width(Length::Fill)]
        .align_y(alignment::Vertical::Center);
        if let Some(message) = self.on_close.clone() {
            header = header.push(
                button(text(CLOSE_GLYPH).size(font_size::TITLE))
                    .padding([0.0, spacing::CELL_PADDING_X])
                    .on_press(message)
                    .style(theme::dialog_button(&palette, false)),
            );
        }
        let mut top = column![header].spacing(spacing::SECTION_GAP);
        if let Some(subtitle) = self.subtitle {
            top = top.push(
                text(subtitle)
                    .size(font_size::STATUS)
                    .color(palette.dim_text)
                    .wrapping(Wrapping::None),
            );
        }

        let hint_line = key_hint_line(self.hints, self.on_close.is_some(), self.lang);
        // The hints sit above the buttons: beside them a long German line
        // would run under three buttons.
        let mut footer = column![container(
            text(hint_line)
                .size(font_size::STATUS)
                .color(palette.dim_text)
                .wrapping(Wrapping::None)
        )
        .width(Length::Fill)
        .clip(true)]
        .spacing(spacing::SECTION_GAP);
        if !self.buttons.is_empty() {
            footer = footer.push(
                container(row(self.buttons).spacing(spacing::CELL_PADDING_X))
                    .width(Length::Fill)
                    .align_x(alignment::Horizontal::Right),
            );
        }

        let body = column![
            top,
            separator(&palette),
            scrollable(
                container(self.content)
                    .width(Length::Fill)
                    .padding(Padding {
                        right: SCROLLBAR_ROOM,
                        ..Padding::ZERO
                    })
            )
            .width(Length::Fill)
            .height(Length::Fill),
            separator(&palette),
            footer,
        ]
        .spacing(spacing::SECTION_GAP);

        container(body)
            .id(FRAME_ID)
            .padding(spacing::OUTER_PADDING * 2.0)
            .width(outer.width)
            .height(outer.height)
            .style(theme::dialog(&palette))
            .into()
    }
}

/// A button of the footer or the content. `highlighted` is the one Enter
/// presses, drawn filled; `on_press` of `None` disables it.
pub fn action<'a>(
    label: &str,
    on_press: Option<Message>,
    highlighted: bool,
    palette: &Palette,
) -> Element<'a, Message> {
    button(text(label.to_owned()))
        .on_press_maybe(on_press)
        .style(theme::dialog_button(palette, highlighted))
        .into()
}

fn separator<'a>(palette: &Palette) -> Element<'a, Message> {
    let color = palette.border;
    rule::horizontal(1)
        .style(move |_theme| rule::Style {
            color,
            radius: 0.0.into(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        })
        .into()
}

/// The one wording of the keys, the same in every dialog.
fn key_hint_line(hints: KeyHints, closable: bool, lang: Language) -> String {
    [
        (closable, Msg::DialogHintEscape),
        (hints.enter, Msg::DialogHintEnter),
        (hints.tab, Msg::DialogHintTab),
    ]
    .into_iter()
    .filter(|(shown, _)| *shown)
    .map(|(_, hint)| lang.text(hint))
    .collect::<Vec<_>>()
    .join(HINT_SEPARATOR)
}

// reason: a failing assertion is the signal in a test
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::layout::INITIAL_WINDOW_SIZE;

    fn sized(size: DialogSize, window: Size) -> (f32, f32) {
        let framed = frame_size(size, window);
        (framed.width, framed.height)
    }

    fn assert_size(size: DialogSize, window: Size, width: f32, height: f32) {
        let framed = frame_size(size, window);
        assert!(
            (framed.width - width).abs() < 0.01 && (framed.height - height).abs() < 0.01,
            "{size:?}: {framed:?}"
        );
    }

    #[test]
    fn the_three_sizes_at_the_initial_window() {
        // 1200 x 760: the shares, clamped.
        assert_size(DialogSize::Small, INITIAL_WINDOW_SIZE, 432.0, 258.4);
        assert_size(DialogSize::Medium, INITIAL_WINDOW_SIZE, 600.0, 471.2);
        assert_size(DialogSize::Large, INITIAL_WINDOW_SIZE, 864.0, 592.8);
    }

    #[test]
    fn a_huge_window_stops_at_the_maximum() {
        let huge = Size::new(5000.0, 4000.0);
        assert_eq!(sized(DialogSize::Small, huge), (520.0, 320.0));
        assert_eq!(sized(DialogSize::Medium, huge), (680.0, 600.0));
        assert_eq!(sized(DialogSize::Large, huge), (960.0, 720.0));
    }

    #[test]
    fn a_tiny_window_keeps_the_margin_on_every_side() {
        let tiny = Size::new(300.0, 200.0);
        for size in [DialogSize::Small, DialogSize::Medium, DialogSize::Large] {
            let framed = frame_size(size, tiny);
            assert!((framed.width - 252.0).abs() < 0.01, "{framed:?}");
            assert!((framed.height - 152.0).abs() < 0.01, "{framed:?}");
        }
    }

    #[test]
    fn a_window_smaller_than_the_margin_gives_an_empty_frame_not_a_negative_one() {
        let framed = frame_size(DialogSize::Large, Size::new(10.0, 10.0));
        assert_eq!((framed.width, framed.height), (0.0, 0.0));
    }

    #[test]
    fn the_sizes_grow_from_small_to_large() {
        let small = frame_size(DialogSize::Small, INITIAL_WINDOW_SIZE);
        let medium = frame_size(DialogSize::Medium, INITIAL_WINDOW_SIZE);
        let large = frame_size(DialogSize::Large, INITIAL_WINDOW_SIZE);
        assert!(small.width < medium.width && medium.width < large.width);
        assert!(small.height < medium.height && medium.height < large.height);
    }

    #[test]
    fn a_line_holds_fewer_characters_than_pixels_and_at_least_one() {
        assert_eq!(chars_that_fit(700.0), 100);
        assert_eq!(chars_that_fit(0.0), 1);
        assert_eq!(chars_that_fit(-50.0), 1);
    }

    #[test]
    fn the_hint_line_names_only_the_keys_that_work() {
        let both = KeyHints {
            enter: true,
            tab: true,
        };
        assert_eq!(
            key_hint_line(both, true, Language::English),
            "Esc: close · Enter: confirm · Tab: next field"
        );
        assert_eq!(
            key_hint_line(KeyHints::default(), true, Language::English),
            "Esc: close"
        );
        assert_eq!(
            key_hint_line(KeyHints::default(), false, Language::German),
            ""
        );
    }
}
