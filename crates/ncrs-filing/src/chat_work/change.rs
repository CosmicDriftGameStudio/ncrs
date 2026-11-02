//! "Bank statements by month": a change of order for one session, shown on
//! an example before it is applied. Never stored.

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use chrono::{Datelike as _, NaiveDate};
use ncrs_index::{KindEntry, Kinds};
use ncrs_intake::{Area, Branch, Level, NamePart, PlaceholderKind, PlaceholderLabel, PlanFile};

use super::{same_word, FolderReading};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeUnit {
    Year,
    Month,
}

/// Files below `area` that lie in a year folder at `year_depth` get a month
/// folder after it. Depths count path segments below the target, the area
/// folder being 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderChange {
    /// The literal start of the branch.
    pub area: PathBuf,
    pub year_depth: usize,
    pub unit: TimeUnit,
    /// Only files with this folder name at this depth.
    pub only: Option<(usize, String)>,
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

fn segments(path: &Path) -> Vec<String> {
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

/// Puts the month folder into `to` for every change that applies. Without a
/// date there is nothing to put.
pub fn apply_changes(to: PathBuf, dated: Option<NaiveDate>, changes: &[OrderChange]) -> PathBuf {
    let Some(date) = dated else {
        return to;
    };
    let mut parts = segments(&to);
    for change in changes {
        let current: PathBuf = parts.iter().collect();
        let year_is_folder = parts
            .get(change.year_depth)
            .is_some_and(|part| is_year_folder(part))
            && change.year_depth + 1 < parts.len();
        let allowed = change
            .only
            .as_ref()
            .is_none_or(|(depth, value)| parts.get(*depth).is_some_and(|part| part == value));
        if current.starts_with(&change.area) && year_is_folder && allowed {
            parts.insert(change.year_depth + 1, month_folder(date));
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
    unit: TimeUnit,
) -> Option<ChangePreview> {
    if unit != TimeUnit::Month {
        return None;
    }
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
            let Some(year_at) = branch.levels.iter().position(|level| {
                matches!(level, Level::Placeholder(placeholder) if matches!(placeholder.kind, PlaceholderKind::Year { .. }))
            }) else {
                continue;
            };
            let has_month = branch.levels.iter().any(|level| {
                matches!(level, Level::Placeholder(placeholder) if placeholder.kind == PlaceholderKind::Month)
            });
            if has_month {
                continue;
            }
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
        .filter(|file| file.fields.date.is_some())
        .max_by(|left, right| {
            left.fields
                .date
                .cmp(&right.fields.date)
                .then_with(|| right.relative_path.cmp(&left.relative_path))
        })?;
    let date = sample.fields.date?;
    let year_depth = chosen.year_at + 1;
    let mut example = segments(&sample.relative_path);
    example.insert(year_depth + 1, month_folder(date));

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

    Some(ChangePreview {
        kind_label: entry.map_or_else(
            || kind_word.to_owned(),
            |found| found.word(kinds.language).to_owned(),
        ),
        change: OrderChange {
            area,
            year_depth,
            unit,
            only: None,
        },
        example,
        highlighted: year_depth + 1,
        scope: scope_of(&chosen, &parts_of_documents(&chosen)),
    })
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
