//! The frame every dialog is drawn in: a header with the title and the close
//! cross, the content in a scrollable of fixed size, and a footer with key
//! hints on the left and the buttons on the right.
//!
//! The width comes from the dialog's [`DialogSize`], the height from the
//! content up to the most the window and the size allow: a longer content
//! scrolls. Only the card keeps its height, whatever it shows.

use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, responsive, row, scrollable, text};
use iced::{alignment, Element, Length, Padding, Size};

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
/// Measured on German running text and paths in the UI font at 12 px: 5.6 to
/// 5.9 px per glyph; a little more, so capitals do not overflow.
pub(in crate::ui) const AVERAGE_GLYPH_WIDTH: f32 = 6.0;

/// Average width of a glyph of a key cap, at its 10 px.
const KEY_CAP_GLYPH_WIDTH: f32 = 6.0;

/// Room kept right of the content for the scrollbar, counted from the edge of
/// the frame. Only a scrolling page takes it: the scrollbar is
/// [`SCROLLBAR_WIDTH`] plus its margin on both sides.
const SCROLLBAR_ROOM: f32 = 22.0;

pub(in crate::ui) const SCROLLBAR_WIDTH: f32 = 6.0;
pub(in crate::ui) const SCROLLBAR_MARGIN: f32 = 2.0;

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
    /// Lists: the command palette and Quick Open.
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
    /// Everything but the card grows with the content up to its height.
    const fn grows_with_content(self) -> bool {
        !matches!(self, Self::Card)
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

/// A key of the page the footer names, with its verb.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageHint {
    pub key: &'static str,
    pub verb: Msg,
}

pub struct Frame<'a> {
    title: String,
    title_theme: Option<String>,
    subtitle: Option<String>,
    content: Element<'a, Message>,
    sidebar: Option<Element<'a, Message>>,
    footer_note: Option<String>,
    buttons: Vec<Element<'a, Message>>,
    on_close: Option<Message>,
    size: DialogSize,
    window: Size,
    hints: KeyHints,
    page_hints: &'static [PageHint],
    /// The sidebar can take the arrow keys, so the footer names Left and Right.
    sidebar_keys: bool,
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
            buttons: Vec::new(),
            on_close: None,
            size,
            window,
            hints: KeyHints::default(),
            page_hints: &[],
            sidebar_keys: false,
            lang,
            palette: *palette,
        }
    }

    /// A second name for the title, dimmed after it.
    pub fn title_theme(mut self, name: Option<&str>) -> Self {
        self.title_theme = name.map(str::to_owned);
        self
    }

    /// A line of explanation, dimmed after the title like the second name.
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
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

    /// The keys of the page itself, named after Esc and Tab where there is
    /// room.
    pub const fn page_hints(mut self, hints: &'static [PageHint]) -> Self {
        self.page_hints = hints;
        self
    }

    fn header(&self) -> Element<'a, Message> {
        let palette = self.palette;
        let mut title = row![text(self.title.clone())
            .size(font_size::HEADER)
            .color(palette.text)
            .wrapping(Wrapping::None)]
        .spacing(spacing::S)
        .align_y(alignment::Vertical::Center);
        for part in [&self.title_theme, &self.subtitle].into_iter().flatten() {
            title = title.push(
                text(part.clone())
                    .size(font_size::HEADER)
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
            None => {
                let (hints, page, closable, sidebar_keys, lang) = (
                    self.hints,
                    self.page_hints,
                    self.on_close.is_some(),
                    self.sidebar_keys,
                    self.lang,
                );
                // The buttons take what they need; the hints get what is left
                // and drop from the right when it is short.
                responsive(move |room| {
                    container(key_hint_row(
                        hints,
                        page,
                        closable,
                        sidebar_keys,
                        lang,
                        room.width,
                        &palette,
                    ))
                    .center_y(Length::Fill)
                    .into()
                })
                .into()
            }
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
        let grows = self.size.grows_with_content();
        let header = self.header();
        let footer = self.footer();

        let page = scrollable(
            container(self.content)
                .width(Length::Fill)
                .padding(Padding {
                    left: spacing::L,
                    right: spacing::L,
                    top: spacing::L,
                    bottom: spacing::L,
                }),
        )
        .direction(Direction::Vertical(
            Scrollbar::new()
                .width(SCROLLBAR_WIDTH)
                .scroller_width(SCROLLBAR_WIDTH)
                .margin(SCROLLBAR_MARGIN)
                .spacing(0.0),
        ))
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

/// The keys the footer names with the verb of each, in the order shown: Esc
/// first, because it is the last to go. Enter is not named: the Enter cap on
/// the main button says it.
fn key_hints(
    hints: KeyHints,
    page: &[PageHint],
    closable: bool,
    sidebar_keys: bool,
) -> Vec<(&'static str, Msg)> {
    let mut named = Vec::new();
    if closable {
        named.push((ESCAPE_KEY, Msg::DialogHintEscape));
    }
    if hints.tab {
        named.push((TAB_KEY, Msg::DialogHintTab));
    }
    named.extend(page.iter().map(|hint| (hint.key, hint.verb)));
    if sidebar_keys {
        named.push((SIDEBAR_KEYS, Msg::DialogHintSidebar));
    }
    named
}

const ESCAPE_KEY: &str = "Esc";
const TAB_KEY: &str = "Tab";
const SIDEBAR_KEYS: &str = "← →";

/// The width of a key cap with its label: the label, the padding either side
/// and the outline.
fn cap_width(key: &str) -> f32 {
    key.chars().count() as f32 * KEY_CAP_GLYPH_WIDTH
        + 2.0 * (spacing::XS - 1.0)
        + 2.0 * spacing::BORDER_WIDTH
}

fn hint_width(key: &str, verb: &str) -> f32 {
    cap_width(key) + spacing::XS + verb.chars().count() as f32 * AVERAGE_GLYPH_WIDTH
}

/// The hints that fit into `room`, from the left. The first stays whatever the
/// room: it is Esc, the one way out.
fn hints_that_fit<'h>(
    hints: &'h [(&'static str, &'static str)],
    room: f32,
) -> &'h [(&'static str, &'static str)] {
    let mut used = 0.0;
    let mut count = 0;
    for (key, verb) in hints {
        used += hint_width(key, verb) + if count > 0 { spacing::L } else { 0.0 };
        if count > 0 && used > room {
            break;
        }
        count += 1;
    }
    hints.get(..count).unwrap_or_default()
}

/// The one wording of the keys, the same in every dialog, as many as fit into
/// `room` pixels.
fn key_hint_row<'a>(
    hints: KeyHints,
    page: &[PageHint],
    closable: bool,
    sidebar_keys: bool,
    lang: Language,
    room: f32,
    palette: &Palette,
) -> Element<'a, Message> {
    let worded: Vec<(&'static str, &'static str)> = key_hints(hints, page, closable, sidebar_keys)
        .into_iter()
        .map(|(key, verb)| (key, lang.text(verb)))
        .collect();
    let groups = hints_that_fit(&worded, room).iter().map(|(key, verb)| {
        row![
            kit::key_cap(key, palette),
            text(*verb)
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
        assert_eq!(chars_that_fit(600.0), 100);
        assert_eq!(chars_that_fit(0.0), 1);
        assert_eq!(chars_that_fit(-50.0), 1);
    }

    fn names(hints: KeyHints, closable: bool) -> Vec<&'static str> {
        key_hints(hints, &[], closable, false)
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
        let verbs: Vec<&str> = key_hints(both, &[], true, false)
            .into_iter()
            .map(|(_, verb)| Language::English.text(verb))
            .collect();
        assert_eq!(verbs, ["close", "next field"]);
    }

    #[test]
    fn hints_drop_from_the_right_and_esc_stays() {
        let hints = [("Esc", "close"), ("Tab", "next field"), ("P", "pause")];
        let all = hint_width("Esc", "close")
            + hint_width("Tab", "next field")
            + hint_width("P", "pause")
            + 2.0 * spacing::L;
        assert_eq!(hints_that_fit(&hints, all).len(), 3);
        assert_eq!(hints_that_fit(&hints, all - 1.0).len(), 2);
        assert_eq!(hints_that_fit(&hints, 0.0).len(), 1);
    }
}
