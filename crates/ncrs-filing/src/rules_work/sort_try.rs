//! The trial run of the rule dialog: the files of a source planned with the
//! rule in place of its stored version, never reading a file the index does
//! not know yet.

use std::path::PathBuf;
use std::sync::PoisonError;

use ncrs_index::{Field, RuleSpec, TargetId};
use ncrs_intake::SortSource;

use super::{with_candidate, RuleTry};
use crate::reading_support::ReadProgress;
use crate::sorting::{
    plan_with_rules, PlanError, PlanFiles, PlanRequest, ReadUnknown, RowState, SortingAccess,
};

/// Rows a trial lists; the rest is counted.
pub const MAX_SORT_TRY_ROWS: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortTryState {
    /// Goes here, below the target; the name has the extension of the file.
    Filed(PathBuf),
    /// The rule fits, but the pattern needs a value the file does not have;
    /// the file stays where it is.
    MissingValue(Field),
    /// The rule fits, but the path it gives is no name every system keeps.
    BadName,
    /// The rule fits, but another file has the place.
    NameTaken(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortTryRow {
    /// Below the source.
    pub relative: PathBuf,
    pub state: SortTryState,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SortTry {
    pub rows: Vec<SortTryRow>,
    /// Rows beyond [`MAX_SORT_TRY_ROWS`].
    pub more: usize,
    /// Files without text: they are asked about, not sorted.
    pub no_text: usize,
}

/// Plans `source` with `candidate` as a rule of `target`. A source the rule
/// cannot reach (it does not file into `target`) has no rows.
pub fn try_in_target(
    access: &SortingAccess,
    source: &SortSource,
    target: &TargetId,
    candidate: RuleTry,
) -> Result<SortTry, String> {
    let RuleTry { replaces, spec } = candidate;
    let spec = RuleSpec {
        confirmed: true,
        enabled: true,
        ..spec
    };
    let stored = {
        let guard = access.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard
            .as_ref()
            .ok_or_else(|| ncrs_core::INDEX_NOT_OPEN.to_owned())?;
        index.rules_of(target).map_err(|err| err.to_string())?
    };
    let (mut rules, candidate_id) = with_candidate(&stored, replaces, spec);
    for rule in &mut rules {
        if rule.id == candidate_id {
            rule.target = target.clone();
        }
    }
    let request = PlanRequest {
        source: source.clone(),
        files: PlanFiles::All,
        read: ReadUnknown::No,
    };
    let plan = plan_with_rules(
        access,
        &request,
        target,
        rules,
        &ReadProgress::default(),
        None,
    )
    .map_err(|err| match err {
        PlanError::Index(text) | PlanError::Filter(text) => text,
        other => other.to_string(),
    })?;
    let mut tried = SortTry::default();
    for row in plan.rows {
        let state = match row.state {
            RowState::NoText => {
                tried.no_text += 1;
                continue;
            }
            RowState::Place(place) if place.rule == candidate_id => {
                SortTryState::Filed(place.relative)
            }
            RowState::Several(places) => {
                match places.into_iter().find(|place| place.rule == candidate_id) {
                    Some(place) => SortTryState::Filed(place.relative),
                    None => continue,
                }
            }
            RowState::NameTaken { place, .. } if place.rule == candidate_id => {
                SortTryState::NameTaken(place.relative)
            }
            RowState::MissingValue { rule, field, .. } if rule == candidate_id => {
                SortTryState::MissingValue(field)
            }
            RowState::BadName { rule, .. } if rule == candidate_id => SortTryState::BadName,
            _ => continue,
        };
        if tried.rows.len() < MAX_SORT_TRY_ROWS {
            tried.rows.push(SortTryRow {
                relative: row.relative,
                state,
            });
        } else {
            tried.more += 1;
        }
    }
    Ok(tried)
}
