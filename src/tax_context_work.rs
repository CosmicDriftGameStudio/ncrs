//! What the tax contexts page of the settings may do with the index. It
//! blocks, so `App` runs it on a blocking thread, never in `update`.

use std::sync::{Arc, Mutex, PoisonError};

use ncrs_index::{
    Index, IndexError, RuleConditions, TaxContext, TaxContextEntry, TaxContextId,
    TaxContextProblem, TaxContextUsage,
};

type IndexSlot = Arc<Mutex<Option<Index>>>;

/// Why a read or change of the tax contexts did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaxContextFailure {
    IndexNotOpen,
    /// The index refused the name or the change; nothing was written.
    Refused(TaxContextProblem),
    Other(String),
}

impl From<IndexError> for TaxContextFailure {
    fn from(err: IndexError) -> Self {
        match err {
            IndexError::InvalidTaxContext(problem) => Self::Refused(problem),
            other => Self::Other(other.to_string()),
        }
    }
}

/// The contexts as they are after a change, so the page needs no second read.
pub type ContextsResult = Result<Vec<TaxContextEntry>, TaxContextFailure>;

#[derive(Clone)]
pub struct TaxContextAccess {
    slot: IndexSlot,
}

impl TaxContextAccess {
    pub fn new(slot: IndexSlot) -> Self {
        Self { slot }
    }

    pub fn list(&self) -> ContextsResult {
        self.with_index(|index| index.tax_contexts())
    }

    pub fn usage(&self, id: TaxContextId) -> Result<TaxContextUsage, TaxContextFailure> {
        self.with_index(|index| index.tax_context_usage(id))
    }

    pub fn conditions(&self, id: TaxContextId) -> Result<RuleConditions, TaxContextFailure> {
        self.with_index(|index| index.tax_context_conditions(id))
    }

    pub fn set_conditions(&self, id: TaxContextId, conditions: &RuleConditions) -> ContextsResult {
        self.with_index(|index| {
            index.set_tax_context_conditions(id, conditions)?;
            index.tax_contexts()
        })
    }

    pub fn create(&self, name: &str, short_name: &str) -> ContextsResult {
        self.with_index(|index| {
            index.create_tax_context(name, short_name)?;
            index.tax_contexts()
        })
    }

    pub fn rename(&self, id: TaxContextId, name: &str, short_name: &str) -> ContextsResult {
        self.with_index(|index| {
            index.rename_tax_context(id, name, short_name)?;
            index.tax_contexts()
        })
    }

    pub fn delete(&self, id: TaxContextId, move_to: TaxContext) -> ContextsResult {
        self.with_index(|index| {
            index.delete_tax_context(id, move_to)?;
            index.tax_contexts()
        })
    }

    fn with_index<T>(
        &self,
        work: impl FnOnce(&mut Index) -> Result<T, IndexError>,
    ) -> Result<T, TaxContextFailure> {
        let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_mut().ok_or(TaxContextFailure::IndexNotOpen)?;
        work(index).map_err(TaxContextFailure::from)
    }
}
