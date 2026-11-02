//! The trial: where each file of a folder would go, decided without touching
//! anything.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ncrs_index::{ContentHash, Field, Kinds};
use ncrs_intake::{render_pattern_in, NeighbourIndex, OwnNames};

use super::change::{apply_changes, OrderChange};
use super::{ChatAccess, FolderReading};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StayReason {
    NoNeighbours,
    MissingField(Field),
    NoPattern,
    NameTaken,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrialOutcome {
    /// Relative to the target.
    Filed {
        to: PathBuf,
    },
    /// Folders below the target, the one with most similar files first.
    Unsure {
        choices: Vec<PathBuf>,
        kind_label: Option<String>,
    },
    /// Relative to the target.
    AlreadyThere {
        at: PathBuf,
    },
    Stays(StayReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrialEntry {
    pub from: PathBuf,
    pub name: String,
    pub hash: ContentHash,
    pub outcome: TrialOutcome,
}

/// In the order the chat shows them: one filed file per area, the unsure
/// ones, the other filed files, the ones already there, the ones that stay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrialPlan {
    pub source_root: PathBuf,
    pub target_root: PathBuf,
    pub entries: Vec<TrialEntry>,
    pub filed: usize,
    pub unsure: usize,
    pub already_there: usize,
    pub stays: usize,
    /// Paths below the target that are in use: what lies there and what the
    /// filed entries claim.
    pub taken: BTreeSet<PathBuf>,
}

impl TrialPlan {
    /// Claims `to` for a file; false when the name is in use.
    pub fn claim(&mut self, to: &Path) -> bool {
        self.taken.insert(to.to_path_buf())
    }
}

fn with_extension_of(rendered: PathBuf, name: &str) -> PathBuf {
    let Some(stem) = rendered.file_name().and_then(|stem| stem.to_str()) else {
        return rendered;
    };
    match Path::new(name).extension().and_then(|ext| ext.to_str()) {
        Some(extension) => rendered.with_file_name(format!("{stem}.{extension}")),
        None => rendered,
    }
}

pub fn trial(
    access: &ChatAccess,
    source: &FolderReading,
    target: &FolderReading,
    changes: &[OrderChange],
) -> Result<TrialPlan, String> {
    let kinds = access.kinds()?;
    let own = OwnNames {
        names: &access.own.names,
        ibans: &access.own.ibans,
        tax_ids: &access.own.tax_ids,
    };
    let neighbours = NeighbourIndex::new(&target.files, &own, &target.structure);
    let mut taken: BTreeSet<PathBuf> = target
        .files
        .iter()
        .map(|file| file.relative_path.clone())
        .collect();

    let mut entries = Vec::with_capacity(source.files.len());
    for file in &source.files {
        let name = file
            .relative_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let outcome = if let Some(at) = target.hashes.get(&file.hash) {
            TrialOutcome::AlreadyThere { at: at.clone() }
        } else {
            decide(&neighbours, &kinds, file, &name, changes, &mut taken)
        };
        entries.push(TrialEntry {
            from: source.root.join(&file.relative_path),
            name,
            hash: file.hash,
            outcome,
        });
    }

    let count = |matches_outcome: fn(&TrialOutcome) -> bool| {
        entries
            .iter()
            .filter(|entry| matches_outcome(&entry.outcome))
            .count()
    };
    let filed = count(|outcome| matches!(outcome, TrialOutcome::Filed { .. }));
    let unsure = count(|outcome| matches!(outcome, TrialOutcome::Unsure { .. }));
    let already_there = count(|outcome| matches!(outcome, TrialOutcome::AlreadyThere { .. }));
    let stays = count(|outcome| matches!(outcome, TrialOutcome::Stays(_)));
    Ok(TrialPlan {
        source_root: source.root.clone(),
        target_root: target.root.clone(),
        entries: in_sample_order(entries),
        filed,
        unsure,
        already_there,
        stays,
        taken,
    })
}

fn decide(
    neighbours: &NeighbourIndex,
    kinds: &Kinds,
    file: &ncrs_intake::PlanFile,
    name: &str,
    changes: &[OrderChange],
    taken: &mut BTreeSet<PathBuf>,
) -> TrialOutcome {
    let Some(placement) = neighbours.place(&file.fields, file.text.as_deref()) else {
        return TrialOutcome::Stays(StayReason::NoNeighbours);
    };
    if !placement.is_sure() {
        let mut choices = vec![placement.folder];
        choices.extend(placement.runner_up);
        return TrialOutcome::Unsure {
            choices,
            kind_label: file
                .fields
                .kind
                .as_deref()
                .map(|kind| kinds.kind_word(kind).into_owned()),
        };
    }
    if let Some(field) = placement.missing {
        return TrialOutcome::Stays(StayReason::MissingField(field));
    }
    let Some(rendered) = placement
        .pattern
        .as_deref()
        .and_then(|pattern| render_pattern_in(pattern, &file.fields, kinds))
    else {
        return TrialOutcome::Stays(StayReason::NoPattern);
    };
    let to = apply_changes(with_extension_of(rendered, name), file.fields.date, changes);
    if taken.insert(to.clone()) {
        TrialOutcome::Filed { to }
    } else {
        TrialOutcome::Stays(StayReason::NameTaken)
    }
}

fn first_segment(to: &Path) -> String {
    to.iter()
        .next()
        .map(|segment| segment.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn in_sample_order(entries: Vec<TrialEntry>) -> Vec<TrialEntry> {
    let mut first_of_area: BTreeMap<String, usize> = BTreeMap::new();
    for (position, entry) in entries.iter().enumerate() {
        if let TrialOutcome::Filed { to } = &entry.outcome {
            first_of_area.entry(first_segment(to)).or_insert(position);
        }
    }
    let sampled: BTreeSet<usize> = first_of_area.into_values().collect();
    let rank = |position: usize, entry: &TrialEntry| match &entry.outcome {
        TrialOutcome::Filed { .. } if sampled.contains(&position) => 0,
        TrialOutcome::Unsure { .. } => 1,
        TrialOutcome::Filed { .. } => 2,
        TrialOutcome::AlreadyThere { .. } => 3,
        TrialOutcome::Stays(_) => 4,
    };
    // Sampled files go in the order of their area, the rest in path order.
    let mut ranked: Vec<(u8, usize, TrialEntry)> = entries
        .into_iter()
        .enumerate()
        .map(|(position, entry)| (rank(position, &entry), position, entry))
        .collect();
    let area_of = |entry: &TrialEntry| match &entry.outcome {
        TrialOutcome::Filed { to } => first_segment(to),
        _ => String::new(),
    };
    ranked.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| {
                if left.0 == 0 {
                    area_of(&left.2).cmp(&area_of(&right.2))
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .then(left.1.cmp(&right.1))
    });
    ranked.into_iter().map(|(_, _, entry)| entry).collect()
}
