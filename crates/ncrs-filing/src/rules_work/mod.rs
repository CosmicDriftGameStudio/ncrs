//! The work behind the rules page: reading the confirmed rules with their hits,
//! switching, moving and deleting one. It blocks, so `App` runs it on a
//! blocking thread, never in `update`.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use ncrs_index::{ContentHash, Index, IndexError, Rule, RuleId, RuleOrigin, RuleSpec, TargetId};

use ncrs_core::INDEX_NOT_OPEN;

use crate::chat_work::Snapshots;

/// What to write to the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleSave {
    Create {
        spec: RuleSpec,
        origin: RuleOrigin,
        /// Appended to the rules of this target.
        target: TargetId,
    },
    /// With a target, the rule goes there as its last.
    Update(RuleId, RuleSpec, Option<TargetId>),
    /// The suggestion becomes a confirmed rule with these values.
    ConfirmSuggestion(RuleId, RuleSpec, Option<TargetId>),
}

/// Another confirm or a discard got to the suggestion first.
const NO_SUCH_SUGGESTION: &str = "the suggested rule is not there any more";

type IndexSlot = Arc<Mutex<Option<Index>>>;

/// A confirmed rule as the page lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedRule {
    pub rule: Rule,
    /// Where the rule stands among all rules, suggestions included: the
    /// index orders them together.
    pub global_index: usize,
    pub hits: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RulesLoad {
    pub rules: Vec<ListedRule>,
}

/// What the user asked to change about one rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleChange {
    SetEnabled {
        id: RuleId,
        enabled: bool,
    },
    /// `to_index` counts in the order of all rules.
    Move {
        id: RuleId,
        to_index: usize,
    },
    Delete {
        id: RuleId,
    },
}

/// The rule the form describes, to try against the documents.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleTry {
    /// The stored rule the candidate takes the place of; `None` for a rule
    /// that is appended.
    pub replaces: Option<RuleId>,
    pub spec: RuleSpec,
}

/// A document sorting renames or moves within its target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveMove {
    /// The target the document lies in.
    pub root: PathBuf,
    /// Relative to `root`.
    pub from: PathBuf,
    /// Relative to `root`.
    pub to: PathBuf,
    pub hash: ContentHash,
}

#[derive(Clone)]
pub struct RulesAccess {
    slot: IndexSlot,
    snapshots: Snapshots,
}

impl std::fmt::Debug for RulesAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RulesAccess").finish_non_exhaustive()
    }
}

impl RulesAccess {
    pub fn new(slot: IndexSlot) -> Self {
        Self {
            slot,
            snapshots: Snapshots::default(),
        }
    }

    /// Writes through this handle drop what the chat handles sharing these
    /// snapshots would still show while the index is busy.
    #[must_use]
    pub(crate) fn with_snapshots(self, snapshots: Snapshots) -> Self {
        Self { snapshots, ..self }
    }

    pub fn load(&self) -> Result<RulesLoad, String> {
        self.with_index(|index| {
            let hits = index.rule_hits()?;
            let rules = index
                .rules()?
                .into_iter()
                .enumerate()
                .filter(|(_, rule)| rule.spec.confirmed)
                .map(|(global_index, rule)| ListedRule {
                    hits: hits.get(&rule.id).copied().unwrap_or(0),
                    global_index,
                    rule,
                })
                .collect();
            Ok(RulesLoad { rules })
        })
    }

    pub fn apply(&self, change: RuleChange) -> Result<(), String> {
        let applied = self.with_index(|index| match change {
            RuleChange::SetEnabled { id, enabled } => {
                index.set_rule_enabled(id, enabled).map(|_| ())
            }
            RuleChange::Move { id, to_index } => index.move_rule(id, to_index),
            RuleChange::Delete { id } => index.delete_rule(id).map(|_| ()),
        });
        self.snapshots.forget();
        applied
    }

    /// Deletes every rule of a removed target; how many went.
    pub fn delete_rules_of(&self, target: &TargetId) -> Result<usize, String> {
        let deleted = self.with_index(|index| index.delete_rules_of(target));
        self.snapshots.forget();
        deleted
    }

    /// Index refusals, such as a regex that does not compile, come back as
    /// the text of the error. The rule that was made or changed comes back.
    pub fn save(&self, save: RuleSave) -> Result<RuleId, String> {
        let written = self.with_index(|index| match save {
            RuleSave::Create {
                spec,
                origin,
                target,
            } => index.create_rule_in(&spec, origin, &target).map(Some),
            RuleSave::Update(id, spec, target) => {
                index.update_rule(id, &spec)?;
                if let Some(target) = target {
                    index.set_rule_target(id, &target)?;
                }
                Ok(Some(id))
            }
            RuleSave::ConfirmSuggestion(id, spec, target) => {
                if !index.confirm_suggested_rule(id, &spec)? {
                    return Ok(None);
                }
                if let Some(target) = target {
                    index.set_rule_target(id, &target)?;
                }
                Ok(Some(id))
            }
        });
        self.snapshots.forget();
        let saved = written?;
        saved.ok_or_else(|| NO_SUCH_SUGGESTION.to_owned())
    }

    /// What a rule made from the document starts with; `None` when the index
    /// does not know the document.
    pub fn document_for_rule(
        &self,
        id: ncrs_index::DocumentId,
    ) -> Result<Option<DocumentForRule>, String> {
        self.with_index(|index| {
            let Some(document) = index.document(id)? else {
                return Ok(None);
            };
            Ok(Some(DocumentForRule {
                sender: document.fields.sender.clone().unwrap_or_default(),
                kind: document.fields.kind.clone().unwrap_or_default(),
                tags: document.fields.tags.clone(),
                text: index.document_text(id)?,
            }))
        })
    }

    fn with_index<T>(
        &self,
        work: impl FnOnce(&mut Index) -> Result<T, IndexError>,
    ) -> Result<T, String> {
        let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_mut().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        work(index).map_err(|err| err.to_string())
    }
}

/// The facts of a filed document a new rule is made from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentForRule {
    pub sender: String,
    pub kind: String,
    pub tags: std::collections::BTreeSet<String>,
    pub text: Option<String>,
}

/// The stored rules with the candidate in the place of `replaces`, or appended
/// when there is nothing to replace; and the id the candidate has there.
fn with_candidate(
    stored: &[Rule],
    replaces: Option<RuleId>,
    spec: RuleSpec,
) -> (Vec<Rule>, RuleId) {
    let replaced = replaces.filter(|id| stored.iter().any(|rule| rule.id == *id));
    if let Some(id) = replaced {
        let rules = stored
            .iter()
            .map(|rule| {
                if rule.id == id {
                    Rule {
                        spec: spec.clone(),
                        ..rule.clone()
                    }
                } else {
                    rule.clone()
                }
            })
            .collect();
        return (rules, id);
    }
    let id = RuleId::unused_in(stored);
    let position = stored
        .iter()
        .map(|rule| rule.position)
        .max()
        .map_or(1, |last| last.saturating_add(1));
    let mut rules = stored.to_vec();
    rules.push(Rule {
        id,
        position,
        origin: RuleOrigin::Hand,
        target: TargetId::main(),
        spec,
    });
    (rules, id)
}

#[cfg(test)]
mod tests;

mod sort_try;
pub use sort_try::{try_in_target, SortTry, SortTryRow, SortTryState, MAX_SORT_TRY_ROWS};

/// The moves in the order they can run, and how many cannot run at all: a
/// name that two documents want goes to the first, and a swap or a circle of
/// renames has no first step. A move whose target is the source of another
/// move waits for that one.
pub(crate) fn order_moves(mut moves: Vec<ArchiveMove>) -> (Vec<ArchiveMove>, usize) {
    moves.sort_by(|left, right| (&left.root, &left.from).cmp(&(&right.root, &right.from)));
    let mut wanted: HashSet<(PathBuf, PathBuf)> = HashSet::new();
    let total = moves.len();
    moves.retain(|found| wanted.insert((found.root.clone(), found.to.clone())));
    let mut blocked = total - moves.len();
    let mut ordered = Vec::with_capacity(moves.len());
    while !moves.is_empty() {
        let leaving: HashSet<(PathBuf, PathBuf)> = moves
            .iter()
            .map(|found| (found.root.clone(), found.from.clone()))
            .collect();
        let (ready, waiting): (Vec<_>, Vec<_>) = moves
            .into_iter()
            .partition(|found| !leaving.contains(&(found.root.clone(), found.to.clone())));
        if ready.is_empty() {
            blocked += waiting.len();
            break;
        }
        ordered.extend(ready);
        moves = waiting;
    }
    (ordered, blocked)
}
