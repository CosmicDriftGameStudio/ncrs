//! The work behind the tags page: reading the tags and the exceptions of the
//! file list from the index and changing them. It blocks, so `App` runs it on a
//! blocking thread, never in `update`.

use std::sync::{Arc, Mutex, PoisonError};

use ncrs_index::{Index, IndexError, RuleSpec, TagTally};

use crate::file_list_tags::TagExceptions;
use crate::suggestion_work::INDEX_NOT_OPEN;

type IndexSlot = Arc<Mutex<Option<Index>>>;

/// What the page shows: every tag with its documents, and the tags that differ
/// from the default of their group.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagsLoaded {
    pub tally: TagTally,
    pub exceptions: TagExceptions,
}

/// Why a change did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagsFailure {
    /// The tag is a condition of these rules, which have to change first.
    InUse(Vec<RuleSpec>),
    Other(String),
}

/// What the tags page may do with the index.
#[derive(Clone)]
pub struct TagsAccess {
    slot: IndexSlot,
}

impl TagsAccess {
    pub fn new(slot: IndexSlot) -> Self {
        Self { slot }
    }

    pub fn load(&self) -> Result<TagsLoaded, TagsFailure> {
        self.with_index(|_| Ok(()))
    }

    /// The tags the file list shows against their group, and nothing else.
    pub fn exceptions(&self) -> Result<TagExceptions, String> {
        let guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_ref().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        index.file_list_tag_exceptions().map_err(failure_text)
    }

    /// `None` makes the tag follow its group again.
    pub fn set_shown(&self, tag: &str, shown: Option<bool>) -> Result<TagsLoaded, TagsFailure> {
        self.with_index(|index| index.set_file_list_tag(tag, shown))
    }

    pub fn clear_exceptions(&self, tags: &[String]) -> Result<TagsLoaded, TagsFailure> {
        self.with_index(|index| index.clear_file_list_tags(tags))
    }

    pub fn rename(&self, old_name: &str, new_name: &str) -> Result<TagsLoaded, TagsFailure> {
        self.with_index(|index| index.rename_own_tag(old_name, new_name))
    }

    pub fn delete(&self, name: &str) -> Result<TagsLoaded, TagsFailure> {
        self.with_index(|index| index.delete_own_tag(name))
    }

    /// Runs `change`, then reads what the page shows. A tag that rules ask for
    /// comes back with those rules.
    fn with_index(
        &self,
        change: impl FnOnce(&mut Index) -> Result<(), IndexError>,
    ) -> Result<TagsLoaded, TagsFailure> {
        let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard
            .as_mut()
            .ok_or_else(|| TagsFailure::Other(INDEX_NOT_OPEN.to_owned()))?;
        match change(index) {
            Ok(()) => {}
            Err(IndexError::TagInUse { rules }) => {
                let specs = rules
                    .into_iter()
                    .filter_map(|id| index.rule(id).ok().flatten())
                    .map(|rule| rule.spec)
                    .collect();
                return Err(TagsFailure::InUse(specs));
            }
            Err(err) => return Err(TagsFailure::Other(failure_text(err))),
        }
        Ok(TagsLoaded {
            tally: index
                .tag_tally()
                .map_err(|err| TagsFailure::Other(failure_text(err)))?,
            exceptions: index
                .file_list_tag_exceptions()
                .map_err(|err| TagsFailure::Other(failure_text(err)))?,
        })
    }
}

fn failure_text(err: IndexError) -> String {
    err.to_string()
}
