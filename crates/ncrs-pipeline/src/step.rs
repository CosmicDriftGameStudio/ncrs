use std::fmt;
use std::path::PathBuf;
use std::time::SystemTime;

use ncrs_index::{DocumentId, Index, ReviewReason};
use ncrs_intake::{FilingPlan, Location};

/// One file on its way through the steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub location: Location,
    pub path: PathBuf,
    /// Set by the step that takes the file into the index.
    pub document: Option<DocumentId>,
    /// Set by the text step when the file has text; read by later steps.
    pub text: Option<String>,
    /// Set by the rules step when a person has to look at the document.
    pub review: Option<ReviewReason>,
    /// Set by the rules step when a rule says where the document goes.
    /// Boxed: a plan holds all fields and would make every queued job large.
    pub plan: Option<Box<FilingPlan>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
    /// Done here; the next step gets the item.
    Continue,
    /// Done, and there is nothing left for later steps to do.
    Finished,
    /// Nothing new about this file. It leaves the pipeline and counts neither
    /// as done nor in the document rate.
    Unchanged,
    /// Try this step again later, for example while the file is still being
    /// written.
    NotReady,
}

pub trait Step: Send + Sync + fmt::Debug {
    fn name(&self) -> &'static str;

    fn run(&self, item: &mut Item, now: SystemTime) -> Result<StepOutcome, String>;

    /// Called once the files found by a scan of the location have all passed
    /// the first step.
    fn after_scan(&self, _location: &Location) -> Result<(), String> {
        Ok(())
    }
}

/// The index the steps work on. The owner decides when it may be reached.
pub trait IndexAccess: Send + Sync + fmt::Debug {
    /// False when there is no index to work on; `work` has not run then.
    fn with_index(&self, work: &mut dyn FnMut(&mut Index)) -> bool;
}

/// `None` when there is no index.
pub fn with_index_value<R>(
    access: &dyn IndexAccess,
    work: impl FnOnce(&mut Index) -> R,
) -> Option<R> {
    let mut pending = Some(work);
    let mut value = None;
    access.with_index(&mut |index| {
        if let Some(once) = pending.take() {
            value = Some(once(index));
        }
    });
    value
}
