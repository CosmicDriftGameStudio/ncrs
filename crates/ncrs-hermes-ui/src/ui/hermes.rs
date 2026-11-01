//! The sidebar of the Hermes card: the pages of Hermes under one frame. Each
//! entry is its plain name, the theme name dimmed behind it, and a badge with
//! a count where the app already knows one.

use iced::Element;

use ncrs_i18n::{FuturamaName, Language, Msg};
use ncrs_index::HermesCounts;
use ncrs_ui_kit::hermes_kit::{self, NavEntry};

use crate::message::{HermesFocus, HermesPage, Message};
use ncrs_config::palette::Palette;

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
            | HermesPage::Tags
            | HermesPage::Rules => None,
        }
        .filter(|count| *count > 0)
    }
}

pub const PAGES: [HermesPage; 8] = [
    HermesPage::Overview,
    HermesPage::Review,
    HermesPage::Rules,
    HermesPage::RuleSuggestions,
    HermesPage::Reorganisation,
    HermesPage::SavedQueries,
    HermesPage::Tags,
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
        HermesPage::Tags => Msg::ActionTags,
        HermesPage::Reorganisation => Msg::ActionReorganisationPlan,
    }
}

/// The navigation of the pages that are tabs, in the layout of the filing kit.
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
