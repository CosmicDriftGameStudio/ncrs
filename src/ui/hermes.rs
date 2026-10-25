//! The sidebar of the Hermes card: the pages of Hermes under one frame. Each
//! entry is its plain name, the theme name dimmed behind it, and a badge with
//! a count where the app already knows one.

use iced::widget::{button, container, row, text, Column};
use iced::{alignment, Element, Length};

use super::format;
use super::frame::{chars_that_fit, SIDEBAR_ROOM};
use super::theme::{self, font_size, size, spacing};
use crate::i18n::{Language, Msg};
use crate::messages::{HermesPage, Message};
use crate::palette::Palette;

/// Room for a badge: a count of up to three digits.
const BADGE_CHARS: usize = 3;

/// The counts the app has at hand; a page without one shows no badge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Badges {
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

const PAGES: [HermesPage; 5] = [
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
    let line_chars =
        chars_that_fit(SIDEBAR_ROOM - 2.0 * spacing::S - 2.0 * spacing::CELL_PADDING_X);
    let items = PAGES.map(|page| {
        let active = page == current;
        let (plain_color, theme_color) = if active {
            (palette.cursor_text, palette.cursor_text)
        } else {
            (palette.text, palette.dim_text)
        };
        let badge = badges.of(page);
        let (plain_msg, theme_msg) = names(page);
        let plain = format::ellipsize_end(lang.text(plain_msg), line_chars);
        let room = line_chars
            .saturating_sub(plain.chars().count())
            .saturating_sub(1)
            .saturating_sub(if badge.is_some() { BADGE_CHARS } else { 0 });
        let theme_name = lang.text(theme_msg);
        let mut content = row![label(plain, plain_color)]
            .spacing(spacing::S)
            .align_y(alignment::Vertical::Center);
        if theme_name.chars().count() <= room {
            content = content.push(label(theme_name.to_owned(), theme_color));
        }
        content = content.push(container("").width(Length::Fill));
        if let Some(count) = badge {
            content = content.push(label(count.to_string(), theme_color));
        }
        button(content)
            .on_press(Message::HermesPage(page))
            .width(Length::Fill)
            .height(size::SIDEBAR_ITEM)
            .padding([0.0, spacing::CELL_PADDING_X])
            .style(theme::row(palette, false, active, true))
            .into()
    });
    Column::with_children(items).into()
}

fn label<'a>(content: String, color: iced::Color) -> Element<'a, Message> {
    text(content)
        .size(font_size::BODY)
        .color(color)
        .wrapping(text::Wrapping::None)
        .into()
}
