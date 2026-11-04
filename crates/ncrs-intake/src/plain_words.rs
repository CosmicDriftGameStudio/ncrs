//! The learned order of an area in plain words, "2024 › Deutsche Bank ›
//! Account, Loan or Messages". Shared by everything that tells a user how a
//! folder is ordered.

use ncrs_i18n::{fill_template, Language, Msg};

use crate::structure::{Area, Branch, Level, PlaceholderLabel};

const PLAIN_JOINER: &str = " › ";

pub fn label_text(lang: Language, label: PlaceholderLabel) -> &'static str {
    lang.text(match label {
        PlaceholderLabel::Year => Msg::HermesChatLabelYear,
        PlaceholderLabel::Month => Msg::HermesChatLabelMonth,
        PlaceholderLabel::Date => Msg::HermesChatLabelDate,
        PlaceholderLabel::Number => Msg::HermesChatLabelNumber,
        PlaceholderLabel::Sender => Msg::HermesChatLabelSender,
        PlaceholderLabel::Kind => Msg::HermesChatLabelKind,
        PlaceholderLabel::Bank => Msg::HermesChatLabelBank,
        PlaceholderLabel::Insurer => Msg::HermesChatLabelInsurer,
        PlaceholderLabel::Vehicle => Msg::HermesChatLabelVehicle,
        PlaceholderLabel::Child => Msg::HermesChatLabelChild,
        PlaceholderLabel::Property => Msg::HermesChatLabelProperty,
        PlaceholderLabel::Subfolder => Msg::HermesChatLabelSubfolder,
    })
}

/// The names of a level with several fixed choices, "Account, Loan or
/// Messages".
fn choice_words(lang: Language, names: &[String]) -> String {
    match names.split_last() {
        Some((last, before)) if !before.is_empty() => fill_template(
            lang.text(Msg::HermesChatLevelOr),
            &[("{list}", &before.join(", ")), ("{last}", last)],
        ),
        Some((only, _)) => only.clone(),
        None => String::new(),
    }
}

/// The folders below the area in words; "no subfolders" when the files lie in
/// the area folder itself. The file at the end is left out.
pub fn branch_plain(lang: Language, branch: &Branch) -> String {
    levels_plain(lang, &branch.levels)
}

/// The same for these levels alone.
pub fn levels_plain(lang: Language, levels: &[Level]) -> String {
    if levels.is_empty() {
        return lang.text(Msg::HermesChatAreaFlat).to_owned();
    }
    levels
        .iter()
        .map(|level| match level {
            Level::Literal(name) => name.clone(),
            Level::Choice(names) => choice_words(lang, names),
            Level::Placeholder(placeholder) => label_text(lang, placeholder.label).to_owned(),
        })
        .collect::<Vec<_>>()
        .join(PLAIN_JOINER)
}

/// The order of the area's main branch, the one with most documents.
pub fn area_order_plain(lang: Language, area: &Area) -> String {
    area.branches.first().map_or_else(
        || lang.text(Msg::HermesChatAreaFlat).to_owned(),
        |branch| branch_plain(lang, branch),
    )
}
