//! The sentences of the chat, filled in.

use std::path::{Component, Path};
use std::time::{Duration, SystemTime};

use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_index::Field;
use ncrs_intake::{
    Area, ConditionProblem, Level, LocationStructure, PlaceholderLabel, RuleProblem,
};
use ncrs_ui_kit::format;

use crate::ctx::Ctx;
use crate::state::{AreaRow, AreasCard, CountRow};

pub fn text(lang: Language, msg: Msg) -> String {
    lang.text(msg).to_owned()
}

/// `values` are named without braces.
pub fn fill(lang: Language, msg: Msg, values: &[(&str, &str)]) -> String {
    let named: Vec<(String, &str)> = values
        .iter()
        .map(|(name, value)| (format!("{{{name}}}"), *value))
        .collect();
    let borrowed: Vec<(&str, &str)> = named
        .iter()
        .map(|(name, value)| (name.as_str(), *value))
        .collect();
    fill_template(lang.text(msg), &borrowed)
}

/// The reason a rule of the chat was refused, in plain words.
pub fn rule_problem_text(lang: Language, problem: &RuleProblem) -> String {
    match problem {
        RuleProblem::NoCondition => text(lang, Msg::HermesChatRuleProblemNoCondition),
        RuleProblem::Condition(ConditionProblem::Empty) => {
            text(lang, Msg::HermesChatRuleProblemEmptyValue)
        }
        RuleProblem::Condition(ConditionProblem::InvalidRegex(reason)) => fill(
            lang,
            Msg::HermesChatRuleProblemInvalidRegex,
            &[("reason", reason)],
        ),
        RuleProblem::Condition(ConditionProblem::InvalidIban) => {
            text(lang, Msg::HermesChatRuleProblemInvalidIban)
        }
        RuleProblem::NoFilingTarget => text(lang, Msg::HermesChatRuleProblemNoFilingTarget),
        RuleProblem::InvalidPattern => text(lang, Msg::HermesChatRuleProblemInvalidPattern),
    }
}

pub fn number(value: usize) -> String {
    value.to_string()
}

pub fn segments(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

/// The folder as the user knows it: the well-known ones by their name in the
/// language, others by their last segment.
pub fn folder_name(ctx: &Ctx, path: &Path) -> String {
    for (name, msg) in [
        ("Documents", Msg::HermesChatFolderDocuments),
        ("Downloads", Msg::HermesChatFolderDownloads),
        ("Desktop", Msg::HermesChatFolderDesktop),
    ] {
        if path == ctx.home.join(name) {
            return text(ctx.lang, msg);
        }
    }
    path.file_name().map_or_else(
        || path.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

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

pub fn plural_text(lang: Language, label: PlaceholderLabel) -> &'static str {
    lang.text(match label {
        PlaceholderLabel::Year => Msg::HermesChatPluralYear,
        PlaceholderLabel::Month => Msg::HermesChatPluralMonth,
        PlaceholderLabel::Date => Msg::HermesChatPluralDate,
        PlaceholderLabel::Number => Msg::HermesChatPluralNumber,
        PlaceholderLabel::Sender => Msg::HermesChatPluralSender,
        PlaceholderLabel::Kind => Msg::HermesChatPluralKind,
        PlaceholderLabel::Bank => Msg::HermesChatPluralBank,
        PlaceholderLabel::Insurer => Msg::HermesChatPluralInsurer,
        PlaceholderLabel::Vehicle => Msg::HermesChatPluralVehicle,
        PlaceholderLabel::Child => Msg::HermesChatPluralChild,
        PlaceholderLabel::Property => Msg::HermesChatPluralProperty,
        PlaceholderLabel::Subfolder => Msg::HermesChatPluralSubfolder,
    })
}

pub fn field_label(lang: Language, field: Field) -> &'static str {
    lang.text(match field {
        Field::Sender => Msg::ReviewListFieldSender,
        Field::Recipient => Msg::ReviewListFieldRecipient,
        Field::Kind => Msg::ReviewListFieldKind,
        Field::Date => Msg::ReviewListFieldDate,
        Field::Number => Msg::ReviewListFieldNumber,
        Field::Amount => Msg::ReviewListFieldAmount,
    })
}

/// "nach Bank, dann Jahr", from the biggest branch of the area.
fn area_description(lang: Language, area: &Area) -> String {
    let labels: Vec<&str> = area
        .branches
        .first()
        .map(|branch| {
            branch
                .levels
                .iter()
                .filter_map(|level| match level {
                    Level::Placeholder(placeholder) => Some(label_text(lang, placeholder.label)),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    if labels.is_empty() {
        return text(lang, Msg::HermesChatAreaFlat);
    }
    fill(
        lang,
        Msg::HermesChatAreaBy,
        &[(
            "levels",
            &labels.join(lang.text(Msg::HermesChatLevelJoiner)),
        )],
    )
}

const SHOWN_AREAS: usize = 3;
const NAMED_MORE_AREAS: usize = 4;

/// The folder's own name; files lying directly in the read folder have none.
fn folder_label(lang: Language, read_folder: &str, folder: &str) -> String {
    if folder.is_empty() {
        fill(lang, Msg::HermesChatDirectlyIn, &[("folder", read_folder)])
    } else {
        folder.to_owned()
    }
}

pub fn areas_card(
    lang: Language,
    read_folder: &str,
    structure: &LocationStructure,
    all: bool,
) -> AreasCard {
    let shown = if all {
        structure.areas.len()
    } else {
        SHOWN_AREAS
    };
    let rows = structure
        .areas
        .iter()
        .take(shown)
        .map(|area| AreaRow {
            name: folder_label(lang, read_folder, &area.folder),
            description: area_description(lang, area),
            documents: area.documents,
        })
        .collect();
    let rest = structure.areas.get(shown..).unwrap_or_default();
    let more = (!rest.is_empty()).then(|| {
        let listed: Vec<String> = rest
            .iter()
            .take(NAMED_MORE_AREAS)
            .map(|area| folder_label(lang, read_folder, &area.folder))
            .collect();
        let further = rest.len().saturating_sub(NAMED_MORE_AREAS);
        let names = listed.join(", ");
        CountRow {
            text: if further == 0 {
                names
            } else {
                fill(
                    lang,
                    Msg::HermesChatMoreAreas,
                    &[("names", &names), ("count", &number(further))],
                )
            },
            documents: rest.iter().map(|area| area.documents).sum(),
        }
    });
    let unordered = (!structure.unordered.is_empty()).then(|| {
        let names: Vec<String> = structure
            .unordered
            .iter()
            .map(|folder| folder_label(lang, read_folder, &folder.folder))
            .collect();
        CountRow {
            text: fill(
                lang,
                Msg::HermesChatUnordered,
                &[("names", &names.join(", "))],
            ),
            documents: structure
                .unordered
                .iter()
                .map(|folder| folder.documents)
                .sum(),
        }
    });
    AreasCard {
        rows,
        more,
        unordered,
    }
}

/// "heute 14:05" on the day of `ctx.now`, else "2026-10-30 14:05".
pub fn when_text(ctx: &Ctx, unix_seconds: i64) -> String {
    let at = u64::try_from(unix_seconds)
        .ok()
        .map(|seconds| SystemTime::UNIX_EPOCH + Duration::from_secs(seconds));
    let formatted = format::time(at);
    let (date, time) = formatted.split_once(' ').unwrap_or((&formatted, ""));
    if date == ctx.now.format("%Y-%m-%d").to_string() {
        fill(ctx.lang, Msg::HermesChatWhenToday, &[("time", time)])
    } else {
        fill(
            ctx.lang,
            Msg::HermesChatWhenDate,
            &[("date", date), ("time", time)],
        )
    }
}

#[cfg(test)]
mod tests {
    use ncrs_intake::{LocationStructure, UnorderedFolder};

    use super::*;

    #[test]
    fn files_lying_directly_in_the_read_folder_are_named_by_it_not_by_a_dot() {
        let structure = LocationStructure {
            areas: Vec::new(),
            unordered: vec![
                UnorderedFolder {
                    folder: String::new(),
                    documents: 4,
                },
                UnorderedFolder {
                    folder: "Misc".to_owned(),
                    documents: 2,
                },
            ],
        };
        let card = areas_card(Language::English, "Downloads", &structure, false);
        let unordered = card.unordered.map(|row| row.text).unwrap_or_default();
        assert!(unordered.contains("directly in Downloads"), "{unordered}");
        assert!(unordered.contains("Misc"), "{unordered}");
        assert!(!unordered.contains('.'), "{unordered}");
    }
}
