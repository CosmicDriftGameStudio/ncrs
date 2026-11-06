//! The sentences of the chat, filled in.

use std::path::{Component, Path};
use std::time::{Duration, SystemTime};

use ncrs_filing::chat_work::{branch_example, FolderReading};
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_index::Field;
use ncrs_intake::{branch_plain, Area, Branch, ConditionProblem, RuleProblem};
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

/// A folder the way the chat shows it where it must be told apart: from home
/// as `~/…`, else as it is.
pub fn shown_folder(ctx: &Ctx, path: &Path) -> String {
    match path.strip_prefix(&ctx.home) {
        Ok(rest) if !rest.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.to_string_lossy().into_owned(),
    }
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

/// "12 files" / "1 file".
pub fn documents_label(lang: Language, documents: u32) -> String {
    if documents == 1 {
        return text(lang, Msg::HermesChatFilesCountOne);
    }
    fill(
        lang,
        Msg::HermesChatFilesCount,
        &[("count", &documents.to_string())],
    )
}

/// The path of a real document of the branch.
pub fn branch_example_path(reading: &FolderReading, area: &Area, branch: &Branch) -> String {
    branch_example(reading, area, branch)
        .map(|file| {
            file.relative_path
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default()
}

pub const SHOWN_AREAS: usize = 4;
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
    reading: &FolderReading,
    all: bool,
) -> AreasCard {
    let structure = &reading.structure;
    let shown = if all {
        structure.areas.len()
    } else {
        SHOWN_AREAS
    };
    let rows = structure
        .areas
        .iter()
        .take(shown)
        .map(|area| {
            let first_branch = area.branches.first();
            AreaRow {
                name: folder_label(lang, read_folder, &area.folder),
                description: first_branch.map_or_else(
                    || text(lang, Msg::HermesChatAreaFlat),
                    |branch| branch_plain(lang, branch),
                ),
                example: first_branch
                    .map(|branch| branch_example_path(reading, area, branch))
                    .unwrap_or_default(),
                documents: area.documents,
            }
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
        CountRow {
            text: fill(
                lang,
                if further == 0 {
                    Msg::HermesChatMoreAreasAll
                } else {
                    Msg::HermesChatMoreAreas
                },
                &[
                    ("names", &listed.join(", ")),
                    ("count", &number(rest.len())),
                ],
            ),
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
        let reading = FolderReading {
            ai: Default::default(),
            kinds: Default::default(),
            root: "Downloads".into(),
            files: Vec::new(),
            photos: 0,
            without_text: 0,
            structure,
            duplicates: Vec::new(),
            hashes: Default::default(),
        };
        let card = areas_card(Language::English, "Downloads", &reading, false);
        let unordered = card.unordered.map(|row| row.text).unwrap_or_default();
        assert!(unordered.contains("directly in Downloads"), "{unordered}");
        assert!(unordered.contains("Misc"), "{unordered}");
        assert!(!unordered.contains('.'), "{unordered}");
    }
}
