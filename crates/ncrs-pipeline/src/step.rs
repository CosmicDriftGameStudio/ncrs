use std::fmt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ncrs_index::{DocumentId, Index};
use ncrs_intake::Location;

/// One file on its way through the steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub location: Location,
    pub path: PathBuf,
    /// Set by the step that takes the file into the index.
    pub document: Option<DocumentId>,
    /// Set by the text step when the file has text; read by later steps. Not
    /// kept for an item that waits for a step that `waits_for_the_rest`: that
    /// step loads it from the index.
    pub text: Option<String>,
    /// A person asked to run the file again: a known file does not stop at
    /// the intake step as unchanged.
    pub force: bool,
    /// Set by the intake step when the stored text of an unchanged document
    /// can be used as it is.
    pub stored_text_is_current: bool,
}

impl Item {
    pub(crate) fn at_first_step(location: &Location, path: &Path, force: bool) -> Self {
        Self {
            location: location.clone(),
            path: path.to_path_buf(),
            document: None,
            text: None,
            force,
            stored_text_is_current: false,
        }
    }
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

    /// True for a step that only enriches what the earlier steps stored (a
    /// model call): items waiting for it queue behind all other work and
    /// arrive without `Item::text`. As the last step, its `Continue` means it
    /// changed something and its `Unchanged` means it did not.
    fn waits_for_the_rest(&self) -> bool {
        false
    }

    /// Called once the files found by a scan of the location have all passed
    /// the first step. Returns the paths it could not settle.
    fn after_scan(&self, _location: &Location) -> Result<Vec<PathBuf>, String> {
        Ok(Vec::new())
    }

    /// Called when the watcher reports `path`, a file or a folder, deleted or
    /// moved out of the location. Returns the paths it could not settle.
    fn path_gone(&self, _location: &Location, _path: &Path) -> Result<Vec<PathBuf>, String> {
        Ok(Vec::new())
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
