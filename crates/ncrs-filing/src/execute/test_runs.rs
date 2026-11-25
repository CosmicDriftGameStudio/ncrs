//! A whole run in one call, as the tests of the journal and the undo need it.

use ncrs_index::{BatchKind, Transfer};

use super::*;

#[derive(Debug)]
pub(crate) struct ExecutionReport {
    pub(crate) batch: BatchId,
    /// In the order of the input.
    pub(crate) outcomes: Vec<ItemOutcome>,
}

impl ExecutionReport {
    pub(crate) fn moved(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|outcome| **outcome == ItemOutcome::Moved)
            .count()
    }

    pub(crate) fn skipped(&self) -> usize {
        self.outcomes.len() - self.moved()
    }
}

pub(crate) fn execute_run(
    index: &Index,
    kind: BatchKind,
    name: &str,
    transfer: Transfer,
    places: RunPlaces<'_>,
    items: &[ExecutionItem],
) -> Result<ExecutionReport, ExecuteError> {
    let batch = open_run_batch(index, kind, name, transfer, &places)?;
    let outcomes = items
        .iter()
        .map(|item| run_one(index, batch, transfer, &places, item))
        .collect::<Result<_, _>>()?;
    index.finish_batch(batch)?;
    Ok(ExecutionReport { batch, outcomes })
}
