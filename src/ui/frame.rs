//! The frame every dialog is drawn in: a header with the title and the close
//! cross, the content in a scrollable of fixed size, and a footer with key
//! hints on the left and the buttons on the right.
//!
//! The size comes from the window and the dialog's [`DialogSize`], never from
//! the content: a long file name or a longer list scrolls or is cut, the frame
//! stays where it is.

use iced::font::Weight;
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, scrollable, text};
use iced::{alignment, Element, Font, Length, Padding, Size};

use super::kit;
use super::theme::{self, font_size, size, spacing};
use crate::i18n::{Language, Msg};
use crate::messages::Message;
use crate::palette::Palette;

/// Lets a test find the frame and read its bounds.
pub const FRAME_ID: &str = "dialog-frame";

/// The close cross. U+00D7 rather than U+2715: the UI font has no glyph for
/// the latter.
const CLOSE_GLYPH: &str = "×";

/// The side of the square close button.
const CLOSE_BUTTON: f32 = 22.0;

/// Space kept free between the frame and every edge of the window.
const WINDOW_MARGIN: f32 = 24.0;

/// Average width of one glyph at the dialog font size, used to turn a width in
/// pixels into a number of characters for text that must stay on one line.
/// Measured: the monospace UI font at 12 px advances 7.22 px per glyph.
const AVERAGE_GLYPH_WIDTH: f32 = 7.25;

/// Room kept right of the content for the scrollbar drawn over it, counted
/// from the edge of the frame.
const SCROLLBAR_ROOM: f32 = 22.0;

/// The border of the frame on each side.
const FRAME_BORDER: f32 = 2.0 * spacing::BORDER_WIDTH;

/// What header, footer, the two dividers and the border take of the height.
const VERTICAL_CHROME: f32 =
    size::HEADER + size::FOOTER + 2.0 * spacing::BORDER_WIDTH + FRAME_BORDER;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogSize {
    /// Confirmations.
    Small,
    /// Forms, the master key, the setup.
    Medium,
    /// Lists and the Inspection.
    Large,
    /// The card of Hermes: a sidebar and a page.
    Card,
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

    const fn fixed(pixels: f32) -> Self {
        Self::new(0.0, pixels, pixels)
    }

    /// Never more than the window minus the margin on both sides, even where
    /// that is less than the minimum.
    fn of(&self, window: f32) -> f32 {
        let room = (window - 2.0 * WINDOW_MARGIN).max(0.0);
        (window * self.share).clamp(self.min, self.max).min(room)
    }
}

impl DialogSize {
    /// Large and the card keep their height; the others grow with the content
    /// up to it.
    const fn grows_with_content(self) -> bool {
        matches!(self, Self::Small | Self::Medium)
    }

    const fn extents(self) -> (Extent, Extent) {
        match self {
            Self::Small => (Extent::fixed(400.0), Extent::new(0.34, 240.0, 320.0)),
            Self::Medium => (Extent::fixed(520.0), Extent::new(0.62, 360.0, 600.0)),
            Self::Large => (Extent::fixed(640.0), Extent::fixed(640.0)),
            Self::Card => (Extent::fixed(1000.0), Extent::fixed(690.0)),
        }
    }
}

/// The outer size of the frame of a dialog of `size` in a window of `window`:
/// for a size that grows with its content, the most it grows to.
pub fn frame_size(size: DialogSize, window: Size) -> Size {
    let (width, height) = size.extents();
    Size::new(width.of(window.width), height.of(window.height))
}

/// How many characters of one line fit into `width` pixels.
pub fn chars_that_fit(width: f32) -> usize {
    (width / AVERAGE_GLYPH_WIDTH).floor().max(1.0) as usize
}

/// What a sidebar takes of the width of a line: the column and its border.
pub const SIDEBAR_ROOM: f32 = size::SIDEBAR_WIDTH + spacing::BORDER_WIDTH;

/// The width of a line of content: the frame less its border, its padding and
/// the room the scrollbar keeps. A sidebar takes
/// [`size::SIDEBAR_WIDTH`] and its line more: pass them as `reserved` to
/// [`content_chars`].
pub fn content_width(size: DialogSize, window: Size) -> f32 {
    frame_size(size, window).width - FRAME_BORDER - spacing::L - SCROLLBAR_ROOM
}

/// The characters one line of the content of a frame holds, with `reserved`
/// pixels of the line taken by other things.
pub fn content_chars(size: DialogSize, window: Size, reserved: f32) -> usize {
    chars_that_fit(content_width(size, window) - reserved)
}

/// Which keys the footer names. Esc is named whenever the frame can be closed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyHints {
    /// Enter presses the primary button, which then carries the Enter cap
    /// (`kit::enter_button`); without it `kit::button`.
    pub enter: bool,
    /// Tab moves the focus between more than one element.
    pub tab: bool,
}

pub struct Frame<'a> {
    title: String,
    title_theme: Option<String>,
    subtitle: Option<String>,
    content: Element<'a, Message>,
    sidebar: Option<Element<'a, Message>>,
    footer_note: Option<String>,
    fixed_height: bool,
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
            title_theme: None,
            subtitle: None,
            content: content.into(),
            sidebar: None,
            footer_note: None,
            fixed_height: false,
            buttons: Vec::new(),
            on_close: None,
            size,
            window,
            hints: KeyHints::default(),
            lang,
            palette: *palette,
        }
    }

    /// A second name for the title, dimmed after it.
    pub fn title_theme(mut self, name: impl Into<String>) -> Self {
        self.title_theme = Some(name.into());
        self
    }

    /// A line of explanation, dimmed after the title like the second name.
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// A column left of the content, [`size::SIDEBAR_WIDTH`] wide.
    pub fn sidebar(mut self, sidebar: impl Into<Element<'a, Message>>) -> Self {
        self.sidebar = Some(sidebar.into());
        self
    }

    /// Keeps the full height of the size whatever the content, for a card whose
    /// content changes between stages.
    pub const fn fixed_height(mut self) -> Self {
        self.fixed_height = true;
        self
    }

    /// Text in the footer where the key hints would be.
    pub fn footer_note(mut self, note: impl Into<String>) -> Self {
        self.footer_note = Some(note.into());
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

    fn header(&self) -> Element<'a, Message> {
        let palette = self.palette;
        let mut title = row![text(self.title.clone())
            .size(font_size::BODY)
            .color(palette.text)
            .font(Font {
                weight: Weight::Semibold,
                ..Font::MONOSPACE
            })
            .wrapping(Wrapping::None)]
        .spacing(spacing::S)
        .align_y(alignment::Vertical::Center);
        for part in [&self.title_theme, &self.subtitle].into_iter().flatten() {
            title = title.push(
                text(part.clone())
                    .size(font_size::BODY)
                    .color(palette.dim_text)
                    .wrapping(Wrapping::None),
            );
        }
        let mut header = row![container(title).width(Length::Fill).clip(true)]
            .spacing(spacing::S)
            .align_y(alignment::Vertical::Center)
            .height(size::HEADER)
            .padding(Padding {
                left: spacing::L,
                right: spacing::S,
                ..Padding::ZERO
            });
        if let Some(message) = self.on_close.clone() {
            header = header.push(
                button(container(text(CLOSE_GLYPH).size(font_size::TITLE)).center(Length::Fill))
                    .padding(0.0)
                    .width(CLOSE_BUTTON)
                    .height(CLOSE_BUTTON)
                    .on_press(message)
                    .style(theme::close_button(&palette)),
            );
        }
        header.into()
    }

    fn footer(&mut self) -> Element<'a, Message> {
        let palette = self.palette;
        let left: Element<'a, Message> = match self.footer_note.take() {
            Some(note) => text(note)
                .size(font_size::BODY)
                .color(palette.dim_text)
                .wrapping(Wrapping::None)
                .into(),
            None => key_hint_row(self.hints, self.on_close.is_some(), self.lang, &palette),
        };
        row![
            container(left).width(Length::Fill).clip(true),
            row(std::mem::take(&mut self.buttons))
                .spacing(spacing::S)
                .align_y(alignment::Vertical::Center),
        ]
        .spacing(spacing::S)
        .align_y(alignment::Vertical::Center)
        .height(size::FOOTER)
        .padding(Padding {
            left: spacing::L,
            right: spacing::M,
            ..Padding::ZERO
        })
        .into()
    }

    pub fn into_element(mut self) -> Element<'a, Message> {
        let palette = self.palette;
        let outer = frame_size(self.size, self.window);
        let grows = self.size.grows_with_content() && !self.fixed_height;
        let header = self.header();
        let footer = self.footer();

        let page = scrollable(
            container(self.content)
                .width(Length::Fill)
                .padding(Padding {
                    left: spacing::L,
                    right: SCROLLBAR_ROOM - FRAME_BORDER,
                    top: spacing::L,
                    bottom: spacing::L,
                }),
        )
        .width(Length::Fill)
        .height(if grows { Length::Shrink } else { Length::Fill });

        let middle: Element<'a, Message> = match self.sidebar {
            Some(sidebar) => row![
                container(sidebar)
                    .width(size::SIDEBAR_WIDTH)
                    .height(Length::Fill)
                    .padding(spacing::S)
                    .style(theme::sidebar(&palette)),
                container("")
                    .width(spacing::BORDER_WIDTH)
                    .height(Length::Fill)
                    .style(theme::divider(&palette)),
                page,
            ]
            .into(),
            None => page.into(),
        };
        let body: Element<'a, Message> = if grows {
            container(middle)
                .max_height((outer.height - VERTICAL_CHROME).max(0.0))
                .into()
        } else {
            middle
        };

        container(column![
            header,
            kit::divider(&palette),
            body,
            kit::divider(&palette),
            footer,
        ])
        .id(FRAME_ID)
        .width(outer.width)
        .height(if grows {
            Length::Shrink
        } else {
            Length::Fixed(outer.height)
        })
        .style(theme::dialog(&palette))
        .into()
    }
}

/// The keys the footer names with the verb of each, in the order shown. Enter
/// is not named: the Enter cap on the main button says it.
fn key_hints(hints: KeyHints, closable: bool) -> Vec<(&'static str, Msg)> {
    [
        (closable, ESCAPE_KEY, Msg::DialogHintEscape),
        (hints.tab, TAB_KEY, Msg::DialogHintTab),
    ]
    .into_iter()
    .filter(|(shown, _, _)| *shown)
    .map(|(_, key, verb)| (key, verb))
    .collect()
}

const ESCAPE_KEY: &str = "Esc";
const TAB_KEY: &str = "Tab";

/// The one wording of the keys, the same in every dialog.
fn key_hint_row<'a>(
    hints: KeyHints,
    closable: bool,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let groups = key_hints(hints, closable).into_iter().map(|(key, verb)| {
        row![
            kit::key_cap(key, palette),
            text(lang.text(verb))
                .size(font_size::BODY)
                .color(palette.dim_text)
                .wrapping(Wrapping::None),
        ]
        .spacing(spacing::XS)
        .align_y(alignment::Vertical::Center)
        .into()
    });
    row(groups)
        .spacing(spacing::L)
        .align_y(alignment::Vertical::Center)
        .into()
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
    fn the_sizes_at_the_initial_window() {
        // 1200 x 760: fixed widths, the heights of the small ones from a share.
        assert_size(DialogSize::Small, INITIAL_WINDOW_SIZE, 400.0, 258.4);
        assert_size(DialogSize::Medium, INITIAL_WINDOW_SIZE, 520.0, 471.2);
        assert_size(DialogSize::Large, INITIAL_WINDOW_SIZE, 640.0, 640.0);
        assert_size(DialogSize::Card, INITIAL_WINDOW_SIZE, 1000.0, 690.0);
    }

    #[test]
    fn a_huge_window_stops_at_the_maximum() {
        let huge = Size::new(5000.0, 4000.0);
        assert_eq!(sized(DialogSize::Small, huge), (400.0, 320.0));
        assert_eq!(sized(DialogSize::Medium, huge), (520.0, 600.0));
        assert_eq!(sized(DialogSize::Large, huge), (640.0, 640.0));
        assert_eq!(sized(DialogSize::Card, huge), (1000.0, 690.0));
    }

    #[test]
    fn a_tiny_window_keeps_the_margin_on_every_side() {
        let tiny = Size::new(300.0, 200.0);
        for size in [
            DialogSize::Small,
            DialogSize::Medium,
            DialogSize::Large,
            DialogSize::Card,
        ] {
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
        let card = frame_size(DialogSize::Card, INITIAL_WINDOW_SIZE);
        assert!(small.width < medium.width && medium.width < large.width);
        assert!(large.width < card.width);
        assert!(small.height < medium.height && medium.height < large.height);
        assert!(large.height < card.height);
    }

    #[test]
    fn a_line_holds_fewer_characters_than_pixels_and_at_least_one() {
        assert_eq!(chars_that_fit(725.0), 100);
        assert_eq!(chars_that_fit(0.0), 1);
        assert_eq!(chars_that_fit(-50.0), 1);
    }

    fn names(hints: KeyHints, closable: bool) -> Vec<&'static str> {
        key_hints(hints, closable)
            .into_iter()
            .map(|(key, _)| key)
            .collect()
    }

    #[test]
    fn the_hints_name_only_the_keys_that_work() {
        let both = KeyHints {
            enter: true,
            tab: true,
        };
        assert_eq!(names(both, true), ["Esc", "Tab"]);
        assert_eq!(names(KeyHints::default(), true), ["Esc"]);
        assert!(names(KeyHints::default(), false).is_empty());
    }

    #[test]
    fn the_hints_give_each_key_its_own_verb() {
        let both = KeyHints {
            enter: true,
            tab: true,
        };
        let verbs: Vec<&str> = key_hints(both, true)
            .into_iter()
            .map(|(_, verb)| Language::English.text(verb))
            .collect();
        assert_eq!(verbs, ["close", "next field"]);
    }
}
