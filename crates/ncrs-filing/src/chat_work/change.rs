//! "Bank statements by month": a change of order for one session, shown on
//! an example before it is applied. Never stored.

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use chrono::{Datelike as _, NaiveDate};
use ncrs_index::{DocumentFields, KindEntry, Kinds};
use ncrs_intake::{
    folder_label, render_pattern_in, Area, Branch, Level, NamePart, PlaceholderKind,
    PlaceholderLabel, PlanFile,
};

use super::pattern::{self, PatternSpec};
use super::{same_word, FolderReading};

/// How the folders below an area are ordered after a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeOrder {
    /// A month folder after the year folder.
    ByMonth,
    /// A year folder straight below the area.
    ByYear,
    /// A folder for the sender straight below the area.
    BySender,
    /// A folder for the kind of document straight below the area.
    ByKind,
    /// No folders between the area and the file.
    Flat,
    /// Every area gets the folders of `OrderChange::pattern`.
    Pattern,
}

/// A change of order for the files below `area`. Depths count path segments
/// below the target, the area folder being 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderChange {
    /// The literal start of the branch.
    pub area: PathBuf,
    /// For `ByMonth` the depth of the year folder the month folder follows;
    /// for the others the number of segments (the area and its literal
    /// start) that stay before the change.
    pub depth: usize,
    pub order: ChangeOrder,
    /// Only files with this folder name at this depth.
    pub only: Option<(usize, String)>,
    /// Every spelling that names the kind the change is for: the catalog ID
    /// and words of a known kind, else the word as typed. Empty: any kind.
    pub kind_names: Vec<String>,
    /// What `ChangeOrder::Pattern` puts below the areas.
    pub pattern: Option<PatternSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub label: PlaceholderLabel,
    /// The path segment the folders are at, for `OrderChange::only`.
    pub depth: usize,
    /// Folder names, most documents first.
    pub values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangePreview {
    pub kind_label: String,
    pub change: OrderChange,
    /// Path segments including the file name.
    pub example: Vec<String>,
    /// The new month segment of `example`.
    pub highlighted: usize,
    pub scope: Option<Scope>,
}

pub(super) fn segments(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

fn is_year_folder(segment: &str) -> bool {
    segment.len() == 4 && segment.chars().all(|character| character.is_ascii_digit())
}

fn month_folder(date: NaiveDate) -> String {
    format!("{:02}", date.month())
}

fn is_of_kind(change: &OrderChange, kind: Option<&str>, folders: &[String]) -> bool {
    change.kind_names.is_empty()
        || kind.is_some_and(|stored| change.kind_names.iter().any(|name| same_word(name, stored)))
        || folders.iter().any(|folder| {
            change
                .kind_names
                .iter()
                .any(|name| names_the_kind(name, folder))
        })
}

/// The one folder name a document gets for `placeholder`, written the way a
/// filing pattern writes it.
pub(super) fn folder_value(
    placeholder: &str,
    fields: &DocumentFields,
    kinds: &Kinds,
) -> Option<String> {
    let path = render_pattern_in(&format!("{{{placeholder}}}"), fields, kinds)?;
    let mut names = segments(&path);
    (names.len() == 1).then(|| names.remove(0))
}

/// The folder a change adds for a document, `None` when the document has no
/// value for it or has the folder already.
fn folder_to_add(
    change: &OrderChange,
    parts: &[String],
    fields: &DocumentFields,
    kinds: &Kinds,
) -> Option<String> {
    let folders = parts.get(change.depth..parts.len().saturating_sub(1))?;
    match change.order {
        ChangeOrder::ByYear => {
            let year = folder_value("year", fields, kinds)?;
            (!folders.iter().any(|folder| is_year_folder(folder))).then_some(year)
        }
        ChangeOrder::BySender => {
            let sender = folder_value("sender", fields, kinds)?;
            (!folders.iter().any(|folder| same_word(folder, &sender))).then_some(sender)
        }
        ChangeOrder::ByKind => {
            let kind = folder_value("kind", fields, kinds)?;
            (!folders.iter().any(|folder| same_word(folder, &kind))).then_some(kind)
        }
        ChangeOrder::ByMonth | ChangeOrder::Flat | ChangeOrder::Pattern => None,
    }
}

/// Puts the document's new path together for every change that applies.
/// Without the field a change needs (the date for a year or month, the sender,
/// the kind) there is nothing to put. A change only touches documents of its
/// kind, or those in a folder that names it.
pub fn apply_changes(
    to: PathBuf,
    fields: &DocumentFields,
    kinds: &Kinds,
    changes: &[OrderChange],
) -> PathBuf {
    let mut parts = segments(&to);
    for change in changes {
        let current: PathBuf = parts.iter().collect();
        let allowed = change
            .only
            .as_ref()
            .is_none_or(|(depth, value)| parts.get(*depth).is_some_and(|part| part == value));
        let folders = parts.split_last().map_or(&[][..], |(_, folders)| folders);
        if !(current.starts_with(&change.area)
            && allowed
            && is_of_kind(change, fields.kind.as_deref(), folders))
        {
            continue;
        }
        match change.order {
            ChangeOrder::ByMonth => {
                let year_is_folder = parts
                    .get(change.depth)
                    .is_some_and(|part| is_year_folder(part))
                    && change.depth + 1 < parts.len();
                if let (true, Some(date)) = (year_is_folder, fields.date) {
                    parts.insert(change.depth + 1, month_folder(date));
                }
            }
            ChangeOrder::ByYear | ChangeOrder::BySender | ChangeOrder::ByKind => {
                if let Some(folder) = folder_to_add(change, &parts, fields, kinds) {
                    parts.insert(change.depth, folder);
                }
            }
            ChangeOrder::Pattern => {
                if let Some(spec) = &change.pattern {
                    pattern::apply(&mut parts, spec, fields, kinds);
                }
            }
            ChangeOrder::Flat => {
                let file_at = parts.len().saturating_sub(1);
                if change.depth < file_at {
                    parts.drain(change.depth..file_at);
                }
            }
        }
    }
    parts.iter().collect()
}

fn matches_branch(parts: &[String], area: &Area, branch: &Branch) -> bool {
    parts.len() == branch.levels.len() + 2
        && parts.first() == Some(&area.folder)
        && branch
            .levels
            .iter()
            .zip(parts.iter().skip(1))
            .all(|(level, part)| match level {
                Level::Literal(name) => name == part,
                Level::Choice(names) => names.contains(part),
                Level::Placeholder(_) => true,
            })
}

/// A real document of the branch to show its pattern with: the newest dated
/// one, else the first by path.
pub fn branch_example<'a>(
    reading: &'a FolderReading,
    area: &Area,
    branch: &Branch,
) -> Option<&'a PlanFile> {
    reading
        .files
        .iter()
        .filter(|file| matches_branch(&segments(&file.relative_path), area, branch))
        .max_by(|left, right| {
            left.fields
                .date
                .cmp(&right.fields.date)
                .then_with(|| right.relative_path.cmp(&left.relative_path))
        })
}

fn names_the_kind(kind_word: &str, name: &str) -> bool {
    same_word(kind_word, name)
        || kind_word
            .split_whitespace()
            .any(|word| same_word(word, name))
}

/// A branch the word names by its structure: a folder or a file name prefix
/// that says the word, like "Kontoauszüge" and "Kontoauszug_".
fn branch_names_the_kind(kind_word: &str, branch: &Branch) -> bool {
    let in_levels = branch.levels.iter().any(|level| match level {
        Level::Literal(name) => names_the_kind(kind_word, name),
        Level::Choice(names) => names.iter().any(|name| names_the_kind(kind_word, name)),
        Level::Placeholder(_) => false,
    });
    in_levels
        || branch.name.as_ref().is_some_and(|scheme| {
            scheme.parts.iter().any(|part| {
                matches!(part, NamePart::Literal(literal) if names_the_kind(kind_word, literal.trim_matches(|c: char| !c.is_alphanumeric())))
            })
        })
}

fn kind_entry<'a>(kinds: &'a Kinds, kind_word: &str) -> Option<&'a KindEntry> {
    kinds.catalog.entries().iter().find(|entry| {
        [
            entry.id(),
            entry.word(ncrs_index::KindLanguage::German),
            entry.word(ncrs_index::KindLanguage::English),
        ]
        .into_iter()
        .any(|name| names_the_kind(kind_word, name))
    })
}

struct Candidate<'a> {
    area: &'a Area,
    year_at: usize,
    branch: &'a Branch,
    documents: Vec<&'a PlanFile>,
}

pub fn preview(
    kinds: &Kinds,
    reading: &FolderReading,
    kind_word: &str,
    order: ChangeOrder,
) -> Option<ChangePreview> {
    let entry = kind_entry(kinds, kind_word);
    let of_kind = |file: &PlanFile| {
        file.fields.kind.as_deref().is_some_and(|kind| match entry {
            Some(wanted) => kinds
                .catalog
                .resolve(kind)
                .is_some_and(|found| found.id() == wanted.id()),
            None => names_the_kind(kind_word, kind),
        })
    };
    let parts_of: Vec<Vec<String>> = reading
        .files
        .iter()
        .map(|file| segments(&file.relative_path))
        .collect();

    let mut best: Option<Candidate<'_>> = None;
    for area in &reading.structure.areas {
        for branch in &area.branches {
            let Some(year_at) = change_point(order, branch) else {
                continue;
            };
            let in_branch: Vec<&PlanFile> = reading
                .files
                .iter()
                .zip(&parts_of)
                .filter(|(_, parts)| matches_branch(parts, area, branch))
                .map(|(file, _)| file)
                .collect();
            let documents: Vec<&PlanFile> = if branch_names_the_kind(kind_word, branch) {
                in_branch.clone()
            } else {
                in_branch
                    .iter()
                    .copied()
                    .filter(|file| of_kind(file))
                    .collect()
            };
            let mostly = documents.len() * 2 > in_branch.len();
            let better = best
                .as_ref()
                .is_none_or(|current| documents.len() > current.documents.len());
            if mostly && better {
                best = Some(Candidate {
                    area,
                    year_at,
                    branch,
                    documents,
                });
            }
        }
    }
    let chosen = best?;

    let sample = chosen
        .documents
        .iter()
        .copied()
        .filter(|file| has_what_the_order_needs(order, &file.fields, kinds))
        .max_by(|left, right| {
            left.fields
                .date
                .cmp(&right.fields.date)
                .then_with(|| right.relative_path.cmp(&left.relative_path))
        })?;
    let depth = chosen.year_at + 1;

    let literal_start = chosen
        .branch
        .levels
        .iter()
        .take(chosen.year_at)
        .take_while(|level| matches!(level, Level::Literal(_)))
        .filter_map(|level| match level {
            Level::Literal(name) => Some(name.as_str()),
            _ => None,
        });
    let area: PathBuf = std::iter::once(chosen.area.folder.as_str())
        .chain(literal_start)
        .collect();

    let kind_names = match entry {
        Some(found) => {
            let mut names = vec![
                found.id().to_owned(),
                found.word(ncrs_index::KindLanguage::German).to_owned(),
                found.word(ncrs_index::KindLanguage::English).to_owned(),
                kind_word.to_owned(),
            ];
            names.dedup();
            names
        }
        None => vec![kind_word.to_owned()],
    };
    let (change_area, scope) = match scope_of(&chosen, &parts_of_documents(&chosen)) {
        Some(scope) => (area, Some(scope)),
        None => match area_scope(
            &chosen.area.folder,
            &reading.files,
            &parts_of,
            &kind_names,
            &of_kind,
        ) {
            Some(scope) => (PathBuf::from(&chosen.area.folder), Some(scope)),
            None => (area, None),
        },
    };

    let change = OrderChange {
        area: change_area,
        depth,
        order,
        only: None,
        kind_names,
        pattern: None,
    };
    let (example, highlighted) = example_of(&change, sample, kinds)?;
    Some(ChangePreview {
        kind_label: entry.map_or_else(
            || kind_word.to_owned(),
            |found| found.word(kinds.language).to_owned(),
        ),
        change,
        example,
        highlighted,
        scope,
    })
}

/// Where in the branch the change happens: the position of the year level
/// for a month folder, else the number of literal levels at its start. `None`
/// when the branch is ordered that way already or has nothing to change.
fn change_point(order: ChangeOrder, branch: &Branch) -> Option<usize> {
    let has = |wanted: fn(&PlaceholderKind) -> bool| {
        branch.levels.iter().any(
            |level| matches!(level, Level::Placeholder(placeholder) if wanted(&placeholder.kind)),
        )
    };
    let leading_literals = branch
        .levels
        .iter()
        .take_while(|level| matches!(level, Level::Literal(_)))
        .count();
    match order {
        ChangeOrder::ByMonth => {
            let year_at = branch.levels.iter().position(|level| {
                matches!(level, Level::Placeholder(placeholder) if matches!(placeholder.kind, PlaceholderKind::Year { .. }))
            })?;
            (!has(|kind| *kind == PlaceholderKind::Month)).then_some(year_at)
        }
        ChangeOrder::ByYear => {
            (!has(|kind| matches!(kind, PlaceholderKind::Year { .. }))).then_some(leading_literals)
        }
        ChangeOrder::BySender => {
            (!has(|kind| *kind == PlaceholderKind::Sender)).then_some(leading_literals)
        }
        ChangeOrder::ByKind => {
            (!has(|kind| *kind == PlaceholderKind::Kind)).then_some(leading_literals)
        }
        ChangeOrder::Flat => (branch.levels.len() > leading_literals).then_some(leading_literals),
        ChangeOrder::Pattern => None,
    }
}

fn has_what_the_order_needs(order: ChangeOrder, fields: &DocumentFields, kinds: &Kinds) -> bool {
    match order {
        ChangeOrder::ByMonth | ChangeOrder::ByYear => fields.date.is_some(),
        ChangeOrder::BySender => folder_value("sender", fields, kinds).is_some(),
        ChangeOrder::ByKind => folder_value("kind", fields, kinds).is_some(),
        ChangeOrder::Flat | ChangeOrder::Pattern => true,
    }
}

/// The sample document's path after the change, and the segment that is new
/// in it (for a flat order, the file name that moved up).
fn example_of(
    change: &OrderChange,
    sample: &PlanFile,
    kinds: &Kinds,
) -> Option<(Vec<String>, usize)> {
    let moved = apply_changes(
        sample.relative_path.clone(),
        &sample.fields,
        kinds,
        std::slice::from_ref(change),
    );
    if moved == sample.relative_path {
        return None;
    }
    let highlighted = match change.order {
        ChangeOrder::ByMonth => change.depth + 1,
        ChangeOrder::ByYear
        | ChangeOrder::BySender
        | ChangeOrder::ByKind
        | ChangeOrder::Flat
        | ChangeOrder::Pattern => change.depth,
    };
    Some((segments(&moved), highlighted))
}

fn parts_of_documents(candidate: &Candidate<'_>) -> Vec<Vec<String>> {
    candidate
        .documents
        .iter()
        .map(|file| segments(&file.relative_path))
        .collect()
}

/// The nearest placeholder folder above the year that holds at least two
/// different names.
fn scope_of(candidate: &Candidate<'_>, parts: &[Vec<String>]) -> Option<Scope> {
    (0..candidate.year_at).rev().find_map(|level_at| {
        let Level::Placeholder(placeholder) = candidate.branch.levels.get(level_at)? else {
            return None;
        };
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for document in parts {
            if let Some(name) = document.get(level_at + 1) {
                *counts.entry(name.as_str()).or_default() += 1;
            }
        }
        let mut values: Vec<(&str, usize)> = counts.into_iter().collect();
        values.sort_by_key(|(name, count)| (Reverse(*count), *name));
        (values.len() >= 2).then(|| Scope {
            label: placeholder.label,
            depth: level_at + 1,
            values: values
                .into_iter()
                .map(|(name, _)| name.to_owned())
                .collect(),
        })
    })
}

/// The first-level folders of the area when the branch has no placeholder to
/// ask about: those that hold documents of the kind or lie in a folder that
/// names it, most files first; when that is fewer than two, all of them.
/// Year folders are no scope.
fn area_scope(
    area_folder: &str,
    files: &[PlanFile],
    parts_of: &[Vec<String>],
    kind_names: &[String],
    of_kind: &dyn Fn(&PlanFile) -> bool,
) -> Option<Scope> {
    let below_area = |parts: &Vec<String>| {
        (parts.first().map(String::as_str) == Some(area_folder) && parts.len() >= 3)
            .then(|| parts.get(1).cloned())
            .flatten()
            .filter(|first| !is_year_folder(first))
    };
    let names_kind = |parts: &[String]| {
        parts
            .iter()
            .skip(1)
            .take(parts.len().saturating_sub(2))
            .any(|folder| kind_names.iter().any(|name| names_the_kind(name, folder)))
    };
    let mut of_the_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut all: BTreeMap<String, usize> = BTreeMap::new();
    for (file, parts) in files.iter().zip(parts_of) {
        let Some(first) = below_area(parts) else {
            continue;
        };
        *all.entry(first.clone()).or_default() += 1;
        if of_kind(file) || names_kind(parts) {
            *of_the_kind.entry(first).or_default() += 1;
        }
    }
    let counted = if of_the_kind.len() >= 2 {
        of_the_kind
    } else {
        all
    };
    if counted.len() < 2 {
        return None;
    }
    let mut values: Vec<(String, usize)> = counted.into_iter().collect();
    values.sort_by_key(|(name, count)| (Reverse(*count), name.clone()));
    Some(Scope {
        label: folder_label(area_folder),
        depth: 1,
        values: values.into_iter().map(|(name, _)| name).collect(),
    })
}
