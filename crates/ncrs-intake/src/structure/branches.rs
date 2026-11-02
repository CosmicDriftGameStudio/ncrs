use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use chrono::Datelike as _;
use ncrs_index::KindCatalog;

use super::folder_type::{folder_type, FolderType};
use super::name_scheme::{dominant_shape, learn_scheme};
use super::tree::{Node, NodeFile};
use super::types::{
    Branch, Level, Placeholder, PlaceholderKind, PlaceholderLabel, StructureDocument,
};
use crate::archive_profile::year_of;

const MAX_EXAMPLES: usize = 3;
/// Folders with similar content are a placeholder from this many on; fewer
/// are a choice, or a constant when alone.
const MIN_SIMILAR_FOR_PLACEHOLDER: usize = 3;
/// Folders that recur under this many merged folders form a closed list.
const MIN_RECURRING_FOR_CHOICE: usize = 2;

/// Folder names that give a sender or subfolder placeholder a better label.
const ANCESTOR_LABELS: [(&[&str], PlaceholderLabel); 5] = [
    (&["bank", "banken"], PlaceholderLabel::Bank),
    (
        &["versicherung", "versicherungen", "insurance"],
        PlaceholderLabel::Insurer,
    ),
    (
        &["auto", "autos", "fahrzeuge", "car", "cars"],
        PlaceholderLabel::Vehicle,
    ),
    (&["kinder", "children"], PlaceholderLabel::Child),
    (
        &["finanzierung", "immobilien", "wohnung", "property"],
        PlaceholderLabel::Property,
    ),
];

/// Where a walk down the folders is.
#[derive(Clone)]
struct Trail {
    /// The nearest constant folder above.
    ancestor: String,
    used: Vec<PlaceholderLabel>,
    below_year: bool,
}

/// A folder name of the siblings with every folder of that name merged.
struct Candidate<'a> {
    name: &'a str,
    folder_type: FolderType,
    nodes: Vec<&'a Node<'a>>,
}

impl Candidate<'_> {
    fn document_count(&self) -> usize {
        self.nodes.iter().map(|node| node.documents.len()).sum()
    }
}

/// What the next level of a branch is and the folders it merges.
struct Step<'a> {
    level: Level,
    nodes: Vec<&'a Node<'a>>,
    is_year: bool,
}

/// The branches below the merged folders `pool` of the area `area_folder`.
pub(super) fn expand<'a>(
    pool: &[&'a Node<'a>],
    kinds: &KindCatalog,
    area_folder: &str,
) -> Vec<Branch> {
    let trail = Trail {
        ancestor: area_folder.to_owned(),
        used: Vec::new(),
        below_year: false,
    };
    expand_below(pool, kinds, &trail)
}

fn expand_below<'a>(pool: &[&'a Node<'a>], kinds: &KindCatalog, trail: &Trail) -> Vec<Branch> {
    let mut branches = Vec::new();
    let files: Vec<&NodeFile<'_>> = pool.iter().flat_map(|node| node.files.iter()).collect();
    if !files.is_empty() {
        let (name, following) = learn_scheme(&files);
        branches.push(Branch {
            levels: Vec::new(),
            name,
            documents: count(files.len()),
            following: count(following),
        });
    }
    for step in steps(pool, kinds, trail) {
        let mut next = trail.clone();
        match &step.level {
            Level::Literal(name) => name.clone_into(&mut next.ancestor),
            Level::Placeholder(placeholder) => next.used.push(placeholder.label),
            Level::Choice(_) => {}
        }
        next.below_year = step.is_year;
        for mut below in expand_below(&step.nodes, kinds, &next) {
            below.levels.insert(0, step.level.clone());
            branches.push(below);
        }
    }
    branches
}

fn steps<'a>(pool: &[&'a Node<'a>], kinds: &KindCatalog, trail: &Trail) -> Vec<Step<'a>> {
    let mut by_name: BTreeMap<&str, Vec<&'a Node<'a>>> = BTreeMap::new();
    for node in pool {
        for (name, child) in &node.children {
            by_name.entry(name.as_str()).or_default().push(child);
        }
    }
    let candidates: Vec<Candidate<'a>> = by_name
        .into_iter()
        .map(|(name, nodes)| {
            let documents: Vec<&StructureDocument> = nodes
                .iter()
                .flat_map(|node| node.documents.iter().copied())
                .collect();
            Candidate {
                name,
                folder_type: folder_type(name, &documents, trail.below_year, kinds),
                nodes,
            }
        })
        .collect();

    let mut levels = Vec::new();
    let mut open = candidates;
    if pool.len() >= MIN_RECURRING_FOR_CHOICE {
        let (recurring, rest): (Vec<_>, Vec<_>) = open.into_iter().partition(|candidate| {
            candidate.nodes.len() >= MIN_RECURRING_FOR_CHOICE
                && !candidate.folder_type.is_counting()
        });
        open = rest;
        if recurring.len() >= MIN_RECURRING_FOR_CHOICE {
            levels.push(choice_step(recurring));
        } else {
            open.extend(recurring);
        }
    }

    for counting in [
        FolderType::Year,
        FolderType::Month,
        FolderType::Date,
        FolderType::Number,
    ] {
        let (group, rest): (Vec<_>, Vec<_>) = open
            .into_iter()
            .partition(|candidate| candidate.folder_type == counting);
        open = rest;
        place_group(group, counting, trail, &mut levels);
    }

    let same_type = open.first().map(|first| first.folder_type).filter(|first| {
        open.iter().all(|candidate| candidate.folder_type == *first)
            && matches!(first, FolderType::Sender | FolderType::Kind)
    });
    match same_type {
        Some(shared_type) if open.len() >= MIN_RECURRING_FOR_CHOICE => {
            place_group(open, shared_type, trail, &mut levels);
        }
        _ => levels.extend(similar_content_steps(open, kinds, trail)),
    }
    levels
}

/// One placeholder for all folders of a type, or one literal each when the
/// placeholder's label is taken already.
fn place_group<'a>(
    group: Vec<Candidate<'a>>,
    group_type: FolderType,
    trail: &Trail,
    found: &mut Vec<Step<'a>>,
) {
    if group.is_empty() {
        return;
    }
    let kind = match group_type {
        FolderType::Year => PlaceholderKind::Year {
            follows_date: follows_date(&group),
        },
        FolderType::Month => PlaceholderKind::Month,
        FolderType::Date => PlaceholderKind::Date,
        FolderType::Number => PlaceholderKind::Number,
        FolderType::Sender => PlaceholderKind::Sender,
        FolderType::Kind => PlaceholderKind::Kind,
        FolderType::Literal => PlaceholderKind::Subfolder,
    };
    let label = label_of(kind, &trail.ancestor);
    if trail.used.contains(&label) {
        found.extend(group.into_iter().map(literal_step));
        return;
    }
    found.push(placeholder_step(group, kind, label));
}

/// Siblings that are none of the same placeholder type: folders with similar
/// content are a placeholder (three or more) or a choice (two).
fn similar_content_steps<'a>(
    open: Vec<Candidate<'a>>,
    kinds: &KindCatalog,
    trail: &Trail,
) -> Vec<Step<'a>> {
    let mut similar: BTreeMap<String, Vec<Candidate<'a>>> = BTreeMap::new();
    for candidate in open {
        similar
            .entry(signature(&candidate, kinds))
            .or_default()
            .push(candidate);
    }
    let mut found = Vec::new();
    for (_signature, group) in similar {
        if group.len() >= MIN_SIMILAR_FOR_PLACEHOLDER {
            place_group(group, FolderType::Literal, trail, &mut found);
        } else if group.len() >= MIN_RECURRING_FOR_CHOICE {
            found.push(choice_step(group));
        } else {
            found.extend(group.into_iter().map(literal_step));
        }
    }
    found
}

/// The names of the folders below and the shape of the files in them.
fn signature(candidate: &Candidate<'_>, kinds: &KindCatalog) -> String {
    let mut child_names = BTreeSet::new();
    for node in &candidate.nodes {
        for (name, child) in &node.children {
            let documents: Vec<&StructureDocument> = child.documents.clone();
            child_names.insert(match folder_type(name, &documents, false, kinds) {
                FolderType::Year => "{Year}".to_owned(),
                FolderType::Month => "{Month}".to_owned(),
                FolderType::Date => "{Date}".to_owned(),
                FolderType::Number => "{Number}".to_owned(),
                FolderType::Sender => "{Sender}".to_owned(),
                FolderType::Kind | FolderType::Literal => name.clone(),
            });
        }
    }
    let files: Vec<&NodeFile<'_>> = candidate
        .nodes
        .iter()
        .flat_map(|node| node.files.iter())
        .collect();
    format!(
        "{}#{}",
        child_names.into_iter().collect::<Vec<_>>().join("|"),
        dominant_shape(&files).unwrap_or_default()
    )
}

fn literal_step(candidate: Candidate<'_>) -> Step<'_> {
    Step {
        level: Level::Literal(candidate.name.to_owned()),
        is_year: false,
        nodes: candidate.nodes,
    }
}

fn choice_step(candidates: Vec<Candidate<'_>>) -> Step<'_> {
    let mut names: Vec<String> = candidates
        .iter()
        .map(|candidate| candidate.name.to_owned())
        .collect();
    names.sort();
    Step {
        level: Level::Choice(names),
        is_year: false,
        nodes: candidates
            .into_iter()
            .flat_map(|candidate| candidate.nodes)
            .collect(),
    }
}

fn placeholder_step(
    mut candidates: Vec<Candidate<'_>>,
    kind: PlaceholderKind,
    label: PlaceholderLabel,
) -> Step<'_> {
    candidates.sort_by_key(|candidate| (Reverse(candidate.document_count()), candidate.name));
    let examples = candidates
        .iter()
        .take(MAX_EXAMPLES)
        .map(|candidate| candidate.name.to_owned())
        .collect();
    Step {
        level: Level::Placeholder(Placeholder {
            kind,
            label,
            examples,
        }),
        is_year: matches!(kind, PlaceholderKind::Year { .. }),
        nodes: candidates
            .into_iter()
            .flat_map(|candidate| candidate.nodes)
            .collect(),
    }
}

/// Whether more than half of the dated documents lie in the folder of their
/// own year.
fn follows_date(years: &[Candidate<'_>]) -> bool {
    let mut dated: usize = 0;
    let mut in_own_year: usize = 0;
    for candidate in years {
        let folder_year = year_of(candidate.name);
        for document in candidate.nodes.iter().flat_map(|node| &node.documents) {
            if let Some(date) = document.date {
                dated += 1;
                in_own_year += usize::from(Some(date.year()) == folder_year);
            }
        }
    }
    in_own_year * 2 > dated
}

fn label_of(kind: PlaceholderKind, ancestor: &str) -> PlaceholderLabel {
    let from_ancestor = || {
        let lowered = ancestor.to_lowercase();
        ANCESTOR_LABELS
            .iter()
            .find(|(names, _)| names.contains(&lowered.as_str()))
            .map(|(_, label)| *label)
    };
    match kind {
        PlaceholderKind::Year { .. } => PlaceholderLabel::Year,
        PlaceholderKind::Month => PlaceholderLabel::Month,
        PlaceholderKind::Date => PlaceholderLabel::Date,
        PlaceholderKind::Number => PlaceholderLabel::Number,
        PlaceholderKind::Kind => PlaceholderLabel::Kind,
        PlaceholderKind::Sender => from_ancestor().unwrap_or(PlaceholderLabel::Sender),
        PlaceholderKind::Subfolder => from_ancestor().unwrap_or(PlaceholderLabel::Subfolder),
    }
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
