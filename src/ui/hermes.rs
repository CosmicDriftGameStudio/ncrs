//! The sidebar of the Hermes card: the pages of Hermes under one frame. Each
//! entry is its plain name, the theme name dimmed behind it, and a badge with
//! a count where the app already knows one.

use iced::widget::{button, container, row, text, Column};
use iced::{alignment, Element, Length};

use super::format;
use super::frame::SIDEBAR_ROOM;
use super::hermes_kit::{self, NavEntry};
use super::theme::{self, font_size, size, spacing};
use crate::i18n::{FuturamaName, Language, Msg};
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
            HermesPage::Overview
            | HermesPage::Inspection
            | HermesPage::Reorganisation
            | HermesPage::SavedQueries
            | HermesPage::Rules => None,
        }
        .filter(|count| *count > 0)
    }
}

pub const PAGES: [HermesPage; 7] = [
    HermesPage::Overview,
    HermesPage::Review,
    HermesPage::Rules,
    HermesPage::RuleSuggestions,
    HermesPage::Reorganisation,
    HermesPage::SavedQueries,
    HermesPage::Inspection,
];

/// The name of a page in the sidebar.
fn name(page: HermesPage) -> Msg {
    match page {
        HermesPage::Overview => Msg::HermesNavOverview,
        HermesPage::Inspection => Msg::HermesNavInspection,
        HermesPage::Review => Msg::ActionReviewList,
        HermesPage::Rules => Msg::ActionRules,
        HermesPage::RuleSuggestions => Msg::ActionRuleSuggestions,
        HermesPage::SavedQueries => Msg::ActionSavedQueries,
        HermesPage::Reorganisation => Msg::ActionReorganisationPlan,
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
        let plain = format::fit_end(
            lang.text(name(page)),
            available - badge_room,
            font_size::BODY,
        );
        let mut content = row![label(plain, plain_color)]
            .spacing(spacing::S)
            .align_y(alignment::Vertical::Center);
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
        label(
            lang.futurama_name(FuturamaName::GroupHermes)
                .unwrap_or_else(|| lang.text(Msg::GroupHermes))
                .to_owned(),
            palette.text
        ),
        label(lang.text(Msg::HermesBeta).to_owned(), palette.dim_text),
    ]
    .spacing(spacing::S)
    .align_y(alignment::Vertical::Center)
    .height(size::SIDEBAR_ITEM)
    .padding([0.0, spacing::CELL_PADDING_X]);
    Column::new().push(heading).extend(items).into()
}

/// The navigation of the pages that are tabs: the same entries as [`nav`], in
/// the layout of the filing kit.
pub fn tab_nav<'a>(
    current: HermesPage,
    badges: Badges,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let entries = PAGES
        .iter()
        .map(|&page| NavEntry {
            label: lang.text(name(page)).to_owned(),
            count: badges.of(page).map(|count| count.to_string()),
            active: page == current,
            on_press: Message::HermesPage(page),
        })
        .collect();
    let heading = lang
        .futurama_name(FuturamaName::GroupHermes)
        .unwrap_or_else(|| lang.text(Msg::GroupHermes));
    hermes_kit::nav(
        heading,
        lang.text(Msg::HermesBeta),
        entries,
        badges.focus == HermesFocus::Sidebar,
        palette,
    )
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
