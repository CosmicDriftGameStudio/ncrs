//! The work behind the reorganisation plan view. It blocks, so `App` runs it
//! on a blocking thread, never in `update`.
//!
//! Making a plan reads the index and the folder tree and stores the plan; the
//! files of the location are not touched. The index is the one the intake
//! shares, so it is held for as long as one location is planned.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use chrono::NaiveDate;
use ncrs_index::{
    BatchKind, Finding, Index, IndexError, KindLanguage, PlanItem, PlanOutcome, RuleId, RuleSpec,
};
use ncrs_intake::{plan_location, FINDING_PRIORITY};

use crate::execute::{self, ExecutionItem, UndoReport};
use crate::suggestion_work::INDEX_NOT_OPEN;
use ncrs_fs::transfer::Progress;

type IndexSlot = Arc<Mutex<Option<Index>>>;

/// One entry of a plan, with the location it belongs to and what the view
/// needs to know about the rule behind it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedItem {
    /// The archive location, as the intake sees it; `item` paths are below it.
    pub location: PathBuf,
    pub item: PlanItem,
    /// The rule behind a `Misplaced` finding that decides the action.
    pub rule: Option<PlannedRule>,
    /// The date of the document, for a `YearMismatch` finding that decides
    /// the action.
    pub date: Option<NaiveDate>,
}

impl PlannedItem {
    /// The finding that decides the action rests on a rule the user has not
    /// confirmed, or on one the index no longer knows.
    pub fn unconfirmed_rule(&self) -> bool {
        match deciding_finding(&self.item) {
            Some(Finding::Misplaced { .. }) => {
                self.rule.as_ref().is_none_or(|rule| !rule.confirmed)
            }
            _ => false,
        }
    }
}

/// A rule a plan entry rests on.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedRule {
    pub id: RuleId,
    /// What the rule looks for, short enough for a sentence.
    pub label: String,
    pub confirmed: bool,
}

/// Longest label of a rule, in characters.
const RULE_LABEL_CHARS: usize = 40;

fn rule_label(spec: &RuleSpec) -> String {
    let phrase = spec
        .conditions
        .list
        .iter()
        .filter(|condition| !condition.negated)
        .map(|condition| condition.value.trim())
        .find(|value| !value.is_empty());
    let name = spec
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    match name.or(phrase) {
        Some(value) if value.chars().count() > RULE_LABEL_CHARS => {
            let cut: String = value.chars().take(RULE_LABEL_CHARS).collect();
            format!("{cut}…")
        }
        Some(value) => value.to_owned(),
        None => spec.kind.clone().unwrap_or_default(),
    }
}

/// The finding the outcome of `item` follows: the first of the priority.
pub fn deciding_finding(item: &PlanItem) -> Option<&Finding> {
    FINDING_PRIORITY
        .iter()
        .find_map(|&kind| item.findings.iter().find(|finding| finding.kind() == kind))
}

/// What the view may do with the index: plan the archive locations.
#[derive(Clone)]
pub struct PlanAccess {
    slot: IndexSlot,
    archive_roots: Vec<PathBuf>,
    kind_language: KindLanguage,
}

impl std::fmt::Debug for PlanAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlanAccess").finish_non_exhaustive()
    }
}

impl PlanAccess {
    pub fn new(slot: IndexSlot, archive_roots: Vec<PathBuf>) -> Self {
        Self {
            slot,
            archive_roots,
            kind_language: KindLanguage::default(),
        }
    }

    pub fn with_kind_language(self, kind_language: KindLanguage) -> Self {
        Self {
            kind_language,
            ..self
        }
    }

    /// Plans every archive location and returns all entries, location by
    /// location, each sorted by source.
    pub fn plan(&self) -> Result<Vec<PlannedItem>, String> {
        let mut planned = Vec::new();
        for root in &self.archive_roots {
            let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
            let index = guard.as_mut().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
            let plan =
                plan_location(index, root, self.kind_language).map_err(|err| err.to_string())?;
            let rules: HashMap<RuleId, PlannedRule> = index
                .rules()
                .map_err(|err| err.to_string())?
                .into_iter()
                .map(|rule| {
                    let planned_rule = PlannedRule {
                        id: rule.id,
                        label: rule_label(&rule.spec),
                        confirmed: rule.spec.confirmed,
                    };
                    (rule.id, planned_rule)
                })
                .collect();
            for item in plan.items {
                let (rule, date) = match deciding_finding(&item) {
                    Some(Finding::Misplaced { rule }) => (rules.get(rule).cloned(), None),
                    Some(Finding::YearMismatch { .. }) => {
                        let found = index
                            .document_by_path(&root.join(&item.source))
                            .map_err(|err| err.to_string())?;
                        (None, found.and_then(|document| document.fields.date))
                    }
                    _ => (None, None),
                };
                planned.push(PlannedItem {
                    location: root.clone(),
                    item,
                    rule,
                    date,
                });
            }
        }
        Ok(planned)
    }
}

/// What a run of the plan did, over all locations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionSummary {
    pub moved: usize,
    pub skipped: usize,
}

/// Counts over the whole selection, although the executor runs one location at
/// a time.
struct Overall<'a, P> {
    inner: &'a mut P,
    done_before: usize,
    total: usize,
}

impl<P: Progress> Progress for Overall<'_, P> {
    fn advanced(&mut self, done: usize, _total: usize) -> std::io::Result<()> {
        self.inner.advanced(self.done_before + done, self.total)
    }
}

/// The entries of the plan that move something, with the document the index
/// knows the file as. The plan holds paths below the location; the index and
/// the executor work with whole paths.
fn execution_items(
    index: &Index,
    entries: &[&PlannedItem],
) -> Result<Vec<ExecutionItem>, IndexError> {
    let mut items = Vec::new();
    for entry in entries {
        let (PlanOutcome::Move { target } | PlanOutcome::Rename { target }) = &entry.item.outcome
        else {
            continue;
        };
        let from = entry.location.join(&entry.item.source);
        let (document_id, duplicate) = match index.document_by_path(&from)? {
            Some(document) => (Some(document.id), false),
            None => match index.document_by_duplicate_path(&from)? {
                Some((id, _stamp)) => (Some(id), true),
                None => (None, false),
            },
        };
        items.push(ExecutionItem {
            from,
            to: entry.location.join(target),
            expected_hash: entry.item.hash,
            document_id,
            duplicate,
        });
    }
    Ok(items)
}

impl PlanAccess {
    /// Carries out the selected entries, location by location, and marks the
    /// plans executed. The index stays locked for the whole run, which keeps
    /// the intake from looking at files while they move.
    pub fn execute<P: Progress>(
        &self,
        selection: &[PlannedItem],
        progress: &mut P,
    ) -> Result<ExecutionSummary, String> {
        let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_mut().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        let mut locations: Vec<&PathBuf> = Vec::new();
        for entry in selection {
            if !locations.contains(&&entry.location) {
                locations.push(&entry.location);
            }
        }
        let total = selection.len();
        let mut summary = ExecutionSummary {
            moved: 0,
            skipped: 0,
        };
        for location in locations {
            let entries: Vec<&PlannedItem> = selection
                .iter()
                .filter(|entry| &entry.location == location)
                .collect();
            let items = execution_items(index, &entries).map_err(|err| err.to_string())?;
            let mut overall = Overall {
                inner: progress,
                done_before: summary.moved + summary.skipped,
                total,
            };
            let report = execute::execute(
                index,
                BatchKind::Reorganisation,
                location,
                &items,
                &mut overall,
            )
            .map_err(|err| err.to_string())?;
            summary.moved += report.moved();
            summary.skipped += report.skipped();
            if let Some(plan) = index
                .reorganisation_plan(location)
                .map_err(|err| err.to_string())?
            {
                index
                    .mark_executed(plan.id)
                    .map_err(|err| err.to_string())?;
            }
        }
        Ok(summary)
    }

    /// Undoes the last carried out reorganisation that is not undone yet;
    /// `None` when there is none.
    pub fn undo_last(&self) -> Result<Option<UndoReport>, String> {
        let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_mut().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        let Some(batch) = index
            .last_undoable_batch(BatchKind::Reorganisation)
            .map_err(|err| err.to_string())?
        else {
            return Ok(None);
        };
        execute::undo(index, batch, &self.archive_roots)
            .map(Some)
            .map_err(|err| err.to_string())
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(any(test, feature = "test-support"))]
pub mod fixtures;
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests;
