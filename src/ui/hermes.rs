//! The sidebar of the Hermes card: the pages of Hermes under one frame. Each
//! entry is its plain name, the theme name dimmed behind it, and a badge with
//! a count where the app already knows one.

use iced::widget::{button, container, row, text, Column};
use iced::{alignment, Element, Length};

use super::format;
use super::frame::SIDEBAR_ROOM;
use super::theme::{self, font_size, size, spacing};
use crate::i18n::{Language, Msg};
use ncrs_index::HermesCounts;

use crate::messages::{HermesFocus, HermesPage, Message};
use crate::palette::Palette;

/// What the app tells the card around a page: the counts it has at hand (a
/// page without one shows no badge) and where the keyboard focus is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Badges {
    pub focus: HermesFocus,
    pub counts: Option<HermesCounts>,
    pub review: Option<usize>,
    pub rule_suggestions: Option<usize>,
}

impl Badges {
    fn of(self, page: HermesPage) -> Option<usize> {
        match page {
            HermesPage::Review => self.review,
            HermesPage::RuleSuggestions => self.rule_suggestions,
            HermesPage::Inspection | HermesPage::Reorganisation | HermesPage::SavedQueries => None,
        }
        .filter(|count| *count > 0)
    }
}

pub const PAGES: [HermesPage; 5] = [
    HermesPage::Inspection,
    HermesPage::Review,
    HermesPage::RuleSuggestions,
    HermesPage::Reorganisation,
    HermesPage::SavedQueries,
];

/// The plain name and the theme name of a page.
fn names(page: HermesPage) -> (Msg, Msg) {
    match page {
        HermesPage::Inspection => (Msg::HermesNavInspection, Msg::HermesNavInspectionTheme),
        HermesPage::Review => (Msg::ActionReviewList, Msg::ActionReviewListTheme),
        HermesPage::RuleSuggestions => {
            (Msg::ActionRuleSuggestions, Msg::ActionRuleSuggestionsTheme)
        }
        HermesPage::SavedQueries => (Msg::ActionSavedQueries, Msg::ActionSavedQueriesTheme),
        HermesPage::Reorganisation => (
            Msg::ActionReorganisationPlan,
            Msg::ActionReorganisationPlanTheme,
        ),
    }
}

pub fn nav<'a>(
    current: HermesPage,
    badges: Badges,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let available = SIDEBAR_ROOM - 2.0 * spacing::S - 2.0 * spacing::CELL_PADDING_X;
    let items = PAGES.map(|page| {
        let active = page == current;
        let (plain_color, theme_color) = if active && badges.focus == HermesFocus::Sidebar {
            (palette.cursor_text, palette.cursor_text)
        } else {
            (palette.text, palette.dim_text)
        };
        let badge = badges.of(page).map(|count| count.to_string());
        let badge_room = badge.as_ref().map_or(0.0, |digits| {
            format::estimated_width(digits, font_size::BODY) + spacing::S
        });
        let (plain_msg, theme_msg) = names(page);
        let plain = format::fit_end(
            lang.text(plain_msg),
            available - badge_room,
            font_size::BODY,
        );
        let theme_name = lang.text(theme_msg);
        let theme_fits = format::estimated_width(&plain, font_size::BODY)
            + spacing::S
            + format::estimated_width(theme_name, font_size::BODY)
            + badge_room
            <= available;
        let mut content = row![label(plain, plain_color)]
            .spacing(spacing::S)
            .align_y(alignment::Vertical::Center);
        if theme_fits {
            content = content.push(label(theme_name.to_owned(), theme_color));
        }
        content = content.push(container("").width(Length::Fill));
        if let Some(digits) = badge {
            content = content.push(label(digits, theme_color));
        }
        button(content)
            .on_press(Message::HermesPage(page))
            .width(Length::Fill)
            .height(size::SIDEBAR_ITEM)
            .padding([0.0, spacing::CELL_PADDING_X])
            .clip(true)
            .style(theme::row(
                palette,
                false,
                active,
                badges.focus == HermesFocus::Sidebar,
            ))
            .into()
    });
    let heading = row![
        label(lang.text(Msg::GroupHermesTheme).to_owned(), palette.text),
        label(lang.text(Msg::HermesBeta).to_owned(), palette.dim_text),
    ]
    .spacing(spacing::S)
    .align_y(alignment::Vertical::Center)
    .height(size::SIDEBAR_ITEM)
    .padding([0.0, spacing::CELL_PADDING_X]);
    Column::new().push(heading).extend(items).into()
}

/// The page before (`step` below zero) or after the one open, or `None` at
/// either end of the sidebar.
pub fn neighbour(current: HermesPage, step: isize) -> Option<HermesPage> {
    let position = PAGES.iter().position(|page| *page == current)?;
    PAGES.get(position.checked_add_signed(step)?).copied()
}

fn label<'a>(content: String, color: iced::Color) -> Element<'a, Message> {
    text(content)
        .size(font_size::BODY)
        .color(color)
        .wrapping(text::Wrapping::None)
        .into()
}
