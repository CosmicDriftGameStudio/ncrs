//! The work behind the rules page: reading the confirmed rules with their hits,
//! switching, moving and deleting one. It blocks, so `App` runs it on a
//! blocking thread, never in `update`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use ncrs_index::{
    ContentHash, DocumentFields, DocumentId, Field, Index, IndexError, KindLanguage, Kinds,
    ReviewReason, Rule, RuleId, RuleOrigin, RuleSpec,
};
use ncrs_intake::{plan, Location, LocationMode, PlanInput, Planning};

use ncrs_core::INDEX_NOT_OPEN;

use crate::filing_work::target_of;

/// What to write to the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleSave {
    Create {
        spec: RuleSpec,
        origin: RuleOrigin,
    },
    Update(RuleId, RuleSpec),
    /// The suggestion becomes a confirmed rule with these values.
    ConfirmSuggestion(RuleId, RuleSpec),
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

/// Where a document is now. The order is the order of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrySection {
    Inbox,
    Review,
    Archive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TryOutcome {
    /// Would be filed here, relative to the archive.
    Filed(PathBuf),
    Review(ReviewReason),
    /// An archive document stays where it is, relative to its archive.
    Lies(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriedDocument {
    pub section: TrySection,
    /// Relative to its location.
    pub path: PathBuf,
    pub now: TryOutcome,
    pub after: TryOutcome,
    /// The other rules that match the document as well.
    pub conflicts: Vec<RuleId>,
}

impl TriedDocument {
    /// Archive documents stay where they lie: filing only moves what comes
    /// from the source folders.
    pub fn is_in_archive(&self) -> bool {
        self.section == TrySection::Archive
    }
}

/// An archive document the rule would rename or move within its archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveMove {
    /// The archive location the document lies in.
    pub root: PathBuf,
    /// Relative to `root`.
    pub from: PathBuf,
    /// Relative to `root`.
    pub to: PathBuf,
    pub hash: ContentHash,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TryResult {
    pub documents: Vec<TriedDocument>,
    /// Matches beyond [`MAX_TRIED_DOCUMENTS`].
    pub more: usize,
    /// Matches that lie in the archive.
    pub in_archive: usize,
    /// The archive matches whose path differs from where the rule would put
    /// them: filing moves them like the documents of the inbox.
    pub archive_moves: Vec<ArchiveMove>,
    /// The given files that lie in a location but not in the index.
    pub not_indexed: usize,
    /// The 1-based place of each conflicting rule on the rules page.
    pub conflict_names: HashMap<RuleId, usize>,
}

pub const MAX_TRIED_DOCUMENTS: usize = 50;

/// A file of a folder as the caller read it from the disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TryFile {
    pub path: PathBuf,
    pub hash: ContentHash,
    pub fields: DocumentFields,
    pub text: String,
}

/// A document the rule is tried against, from the index or from a `TryFile`.
struct Subject<'a> {
    location: &'a Location,
    absolute: PathBuf,
    text: String,
    fields: DocumentFields,
    manual_fields: Vec<Field>,
    hash: ContentHash,
    id: Option<DocumentId>,
}

#[derive(Clone)]
pub struct RulesAccess {
    slot: IndexSlot,
    locations: Vec<Location>,
    kind_language: KindLanguage,
}

impl std::fmt::Debug for RulesAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RulesAccess").finish_non_exhaustive()
    }
}

impl RulesAccess {
    pub fn new(slot: IndexSlot, locations: Vec<Location>) -> Self {
        Self {
            slot,
            locations,
            kind_language: KindLanguage::default(),
        }
    }

    pub fn with_kind_language(self, kind_language: KindLanguage) -> Self {
        Self {
            kind_language,
            ..self
        }
    }

    /// Reads only: nothing is written to the index or the disk. The documents
    /// the rule would match, as they stand and as they would stand with it.
    pub fn try_rule(&self, candidate: &RuleTry) -> Result<TryResult, String> {
        self.try_rule_with(candidate, &[])
    }

    /// `try_rule`, with files the index may not know yet: the ones it does
    /// not know are tried from what the caller read of them.
    pub fn try_rule_with(
        &self,
        candidate: &RuleTry,
        files: &[TryFile],
    ) -> Result<TryResult, String> {
        self.with_index(|index| {
            let rules_now = index.rules()?;
            let spec = RuleSpec {
                confirmed: true,
                enabled: true,
                ..candidate.spec.clone()
            };
            let (rules_after, candidate_id) = with_candidate(&rules_now, candidate.replaces, spec);
            let aliases = index.confirmed_sender_aliases()?;
            let in_review: HashSet<_> = index
                .open_reviews()?
                .into_iter()
                .map(|entry| entry.document_id)
                .collect();
            let mut tried = Vec::new();
            let mut archive_moves: Vec<ArchiveMove> = Vec::new();
            let kinds = Kinds::new(Arc::clone(index.kinds()), self.kind_language);
            let mut seen: HashSet<PathBuf> = HashSet::new();
            let mut subjects: Vec<Subject> = Vec::new();
            for location in &self.locations {
                for (id, absolute) in index.documents_under(&location.path)? {
                    seen.insert(absolute.clone());
                    let Some(text) = index.document_text(id)? else {
                        continue;
                    };
                    let Some(document) = index.document(id)? else {
                        continue;
                    };
                    subjects.push(Subject {
                        location,
                        absolute,
                        text,
                        fields: document.fields,
                        manual_fields: index.field_origins(id)?.manual_fields(),
                        hash: document.hash,
                        id: Some(id),
                    });
                }
            }
            let mut not_indexed = 0;
            for file in files {
                let Some(location) = self
                    .locations
                    .iter()
                    .find(|location| file.path.starts_with(&location.path))
                else {
                    continue;
                };
                if seen.contains(&file.path) {
                    continue;
                }
                not_indexed += 1;
                subjects.push(Subject {
                    location,
                    absolute: file.path.clone(),
                    text: file.text.clone(),
                    fields: file.fields.clone(),
                    manual_fields: Vec::new(),
                    hash: file.hash,
                    id: None,
                });
            }
            for subject in subjects {
                let location = subject.location;
                let absolute = subject.absolute;
                let path = absolute
                    .strip_prefix(&location.path)
                    .map_or_else(|_| absolute.clone(), std::path::Path::to_path_buf);
                let input = PlanInput {
                    text: &subject.text,
                    fields: &subject.fields,
                    path: Some(&path),
                    sender_aliases: &aliases,
                    manual_fields: &subject.manual_fields,
                    kinds: &kinds,
                };
                let after = plan(&rules_after, &input);
                let matching = matching_rules(&after);
                if !matching.contains(&candidate_id) {
                    continue;
                }
                let archived = location.mode == LocationMode::Archive;
                let now = if archived {
                    TryOutcome::Lies(path.clone())
                } else {
                    outcome(&plan(&rules_now, &input), &path)
                };
                let section = if archived {
                    TrySection::Archive
                } else if subject.id.is_some_and(|id| in_review.contains(&id)) {
                    TrySection::Review
                } else {
                    TrySection::Inbox
                };
                let after_outcome = outcome(&after, &path);
                if let (true, TryOutcome::Filed(target)) = (archived, &after_outcome) {
                    if *target != path {
                        archive_moves.push(ArchiveMove {
                            root: location.path.clone(),
                            from: path.clone(),
                            to: target.clone(),
                            hash: subject.hash,
                        });
                    }
                }
                tried.push(TriedDocument {
                    section,
                    path,
                    now,
                    after: after_outcome,
                    conflicts: matching
                        .iter()
                        .copied()
                        .filter(|other| *other != candidate_id)
                        .collect(),
                });
            }
            archive_moves
                .sort_by(|left, right| (&left.root, &left.from).cmp(&(&right.root, &right.from)));
            tried.sort_by(|left, right| {
                (left.section, &left.path).cmp(&(right.section, &right.path))
            });
            let in_archive = tried
                .iter()
                .filter(|document| document.is_in_archive())
                .count();
            let more = tried.len().saturating_sub(MAX_TRIED_DOCUMENTS);
            tried.truncate(MAX_TRIED_DOCUMENTS);
            Ok(TryResult {
                conflict_names: conflict_places(&rules_after, &tried),
                documents: tried,
                in_archive,
                archive_moves,
                not_indexed,
                more,
            })
        })
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
        self.with_index(|index| match change {
            RuleChange::SetEnabled { id, enabled } => {
                index.set_rule_enabled(id, enabled).map(|_| ())
            }
            RuleChange::Move { id, to_index } => index.move_rule(id, to_index),
            RuleChange::Delete { id } => index.delete_rule(id).map(|_| ()),
        })
    }

    /// Index refusals, such as a regex that does not compile, come back as
    /// the text of the error. The rule that was made or changed comes back.
    pub fn save(&self, save: RuleSave) -> Result<RuleId, String> {
        let saved = self.with_index(|index| match save {
            RuleSave::Create { spec, origin } => index.create_rule_from(&spec, origin).map(Some),
            RuleSave::Update(id, spec) => index.update_rule(id, &spec).map(|()| Some(id)),
            RuleSave::ConfirmSuggestion(id, spec) => index
                .confirm_suggested_rule(id, &spec)
                .map(|done| done.then_some(id)),
        })?;
        saved.ok_or_else(|| NO_SUCH_SUGGESTION.to_owned())
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
        spec,
    });
    (rules, id)
}

fn matching_rules(planning: &Planning) -> &[RuleId] {
    match planning {
        Planning::Plan(filing) => &filing.matching_rules,
        Planning::Review { matching_rules, .. } => matching_rules,
    }
}

/// A pattern names the file without its extension; filing adds the one of
/// the file, so the outcome shows it too.
fn outcome(planning: &Planning, source: &Path) -> TryOutcome {
    match planning {
        Planning::Plan(filing) => {
            TryOutcome::Filed(target_of(Path::new(""), &filing.relative_path, source))
        }
        Planning::Review { reason, .. } => TryOutcome::Review(*reason),
    }
}

/// Where the rules page lists each conflicting rule: it lists the confirmed
/// ones in the order of the index.
fn conflict_places(rules: &[Rule], tried: &[TriedDocument]) -> HashMap<RuleId, usize> {
    let listed: Vec<RuleId> = rules
        .iter()
        .filter(|rule| rule.spec.confirmed)
        .map(|rule| rule.id)
        .collect();
    tried
        .iter()
        .flat_map(|document| &document.conflicts)
        .filter_map(|id| {
            let place = listed.iter().position(|listed_id| listed_id == id)?;
            Some((*id, place.saturating_add(1)))
        })
        .collect()
}

#[cfg(test)]
mod tests;
