//! "Everywhere like this": one pattern of folders below every area, shown on
//! one document before it is adopted for the session.

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::Datelike as _;
use ncrs_index::{DocumentFields, Kinds};
use ncrs_intake::{Level, LocationStructure, PlaceholderKind};

use super::change::{folder_value, segments, ChangeOrder, OrderChange};
use super::FolderReading;

/// How many patterns the chat offers to adopt.
pub const OFFERED_PATTERNS: usize = 3;

/// One folder level of an adoptable pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PatternLevel {
    Year,
    Month,
    Sender,
    Kind,
}

/// A pattern some areas follow, and how many documents lie in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternChoice {
    pub levels: Vec<PatternLevel>,
    pub documents: u32,
}

/// A branch of an area whose trailing folders follow some pattern: the
/// documents lying exactly in it get the folders of the adopted one instead,
/// after the `keep` folders at its start (a bank, a property).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternTarget {
    pub area: String,
    pub levels: Vec<Level>,
    pub keep: usize,
}

/// The folders every document of the targets gets below the folders it keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternSpec {
    pub levels: Vec<PatternLevel>,
    pub targets: Vec<PatternTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternPreview {
    pub change: OrderChange,
    /// Documents whose place changes.
    pub moved: usize,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

fn level_of(level: &Level) -> Option<PatternLevel> {
    match level {
        Level::Placeholder(placeholder) => match placeholder.kind {
            PlaceholderKind::Year { .. } => Some(PatternLevel::Year),
            PlaceholderKind::Month => Some(PatternLevel::Month),
            PlaceholderKind::Sender => Some(PatternLevel::Sender),
            PlaceholderKind::Kind => Some(PatternLevel::Kind),
            PlaceholderKind::Date | PlaceholderKind::Number | PlaceholderKind::Subfolder => None,
        },
        Level::Literal(_) | Level::Choice(_) => None,
    }
}

/// The folders a branch ends in when they are all year, month, sender or kind
/// folders, and how many folders come before them.
fn trailing_pattern(levels: &[Level]) -> Option<(usize, Vec<PatternLevel>)> {
    let mut pattern: Vec<PatternLevel> = levels.iter().rev().map_while(level_of).collect();
    pattern.reverse();
    (!pattern.is_empty()).then(|| (levels.len() - pattern.len(), pattern))
}

/// The distinct patterns the branches end in, most documents first.
pub fn pattern_choices(structure: &LocationStructure) -> Vec<PatternChoice> {
    let mut choices = all_choices(structure);
    choices.truncate(OFFERED_PATTERNS);
    choices
}

/// The patterns that would move at least one file of the reading.
pub fn adoptable_patterns(kinds: &Kinds, reading: &FolderReading) -> Vec<PatternChoice> {
    let mut choices: Vec<PatternChoice> = all_choices(&reading.structure)
        .into_iter()
        .filter(|choice| preview(kinds, reading, &choice.levels).is_some())
        .collect();
    choices.truncate(OFFERED_PATTERNS);
    choices
}

fn all_choices(structure: &LocationStructure) -> Vec<PatternChoice> {
    let mut found: BTreeMap<Vec<PatternLevel>, u32> = BTreeMap::new();
    for branch in structure.areas.iter().flat_map(|area| &area.branches) {
        if let Some((_, levels)) = trailing_pattern(&branch.levels) {
            *found.entry(levels).or_default() += branch.documents;
        }
    }
    let mut choices: Vec<PatternChoice> = found
        .into_iter()
        .map(|(levels, documents)| PatternChoice { levels, documents })
        .collect();
    choices.sort_by(|left, right| {
        right
            .documents
            .cmp(&left.documents)
            .then_with(|| left.levels.cmp(&right.levels))
    });
    choices
}

impl PatternTarget {
    fn fits(&self, parts: &[String]) -> bool {
        parts.len() == self.levels.len() + 2
            && parts.first() == Some(&self.area)
            && self
                .levels
                .iter()
                .zip(parts.iter().skip(1))
                .all(|(level, part)| match level {
                    Level::Literal(name) => name == part,
                    Level::Choice(names) => names.contains(part),
                    Level::Placeholder(_) => true,
                })
    }
}

fn folder_of(level: PatternLevel, fields: &DocumentFields, kinds: &Kinds) -> Option<String> {
    match level {
        PatternLevel::Year => folder_value("year", fields, kinds),
        PatternLevel::Month => fields.date.map(|date| format!("{:02}", date.month())),
        PatternLevel::Sender => folder_value("sender", fields, kinds),
        PatternLevel::Kind => folder_value("kind", fields, kinds),
    }
}

/// Where a file goes in a target that has no order of its own: below the
/// folders of `levels`, in none for an empty list. `None` when the file lacks
/// what a level needs.
pub(in crate::chat_work) fn layout_path(
    levels: &[PatternLevel],
    fields: &DocumentFields,
    kinds: &Kinds,
    file_name: &str,
) -> Option<PathBuf> {
    let mut path = PathBuf::new();
    for level in levels {
        path.push(folder_of(*level, fields, kinds)?);
    }
    Some(path.join(file_name))
}

/// Replaces the folders between the area and the file. A document without a
/// value for one of the levels stays where it is.
pub(in crate::chat_work) fn apply(
    parts: &mut Vec<String>,
    spec: &PatternSpec,
    fields: &DocumentFields,
    kinds: &Kinds,
) {
    let Some(target) = spec.targets.iter().find(|target| target.fits(parts)) else {
        return;
    };
    let level_folders: Option<Vec<String>> = spec
        .levels
        .iter()
        .map(|level| folder_of(*level, fields, kinds))
        .collect();
    if let Some(folders) = level_folders {
        let file_at = parts.len() - 1;
        parts.splice(1 + target.keep..file_at, folders);
    }
}

pub fn preview(
    kinds: &Kinds,
    reading: &FolderReading,
    levels: &[PatternLevel],
) -> Option<PatternPreview> {
    let targets: Vec<PatternTarget> = reading
        .structure
        .areas
        .iter()
        .flat_map(|area| {
            area.branches.iter().filter_map(|branch| {
                let (keep, _) = trailing_pattern(&branch.levels)?;
                Some(PatternTarget {
                    area: area.folder.clone(),
                    levels: branch.levels.clone(),
                    keep,
                })
            })
        })
        .collect();
    let change = OrderChange {
        area: PathBuf::new(),
        depth: 1,
        order: ChangeOrder::Pattern,
        only: None,
        kind_names: Vec::new(),
        pattern: Some(PatternSpec {
            levels: levels.to_vec(),
            targets,
        }),
    };
    let mut moved = 0;
    let mut sample: Option<(&ncrs_intake::PlanFile, PathBuf)> = None;
    for file in &reading.files {
        let after = super::change::apply_changes(
            file.relative_path.clone(),
            &file.fields,
            kinds,
            std::slice::from_ref(&change),
        );
        if after == file.relative_path {
            continue;
        }
        moved += 1;
        let newer = sample
            .as_ref()
            .is_none_or(|(current, _)| file.fields.date > current.fields.date);
        if newer {
            sample = Some((file, after));
        }
    }
    let (file, after) = sample?;
    Some(PatternPreview {
        change,
        moved,
        before: segments(&file.relative_path),
        after: segments(&after),
    })
}
