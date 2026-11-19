//! The work behind the rules page: reading the confirmed rules with their hits,
//! switching, moving and deleting one. It blocks, so `App` runs it on a
//! blocking thread, never in `update`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use ncrs_index::{
    ContentHash, DocumentFields, DocumentId, Field, Index, IndexError, KindLanguage, Kinds,
    ReviewReason, Rule, RuleId, RuleOrigin, RuleSpec, SenderAliases,
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
    /// Archive matches that lie wrong but cannot be moved in one run: two want
    /// the same name, or they swap names.
    pub archive_blocked: usize,
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
    /// The root of the target archive: the only place a rule renames or moves
    /// documents in. Without one no archive document is moved.
    target: Option<PathBuf>,
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
            target: None,
            kind_language: KindLanguage::default(),
        }
    }

    pub fn with_target(self, target: Option<PathBuf>) -> Self {
        Self { target, ..self }
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
    ///
    /// The index lock is held for one document at a time, so a try on a large
    /// archive does not keep the intake and the search waiting; only the
    /// documents the rule matches are kept.
    pub fn try_rule_with(
        &self,
        candidate: &RuleTry,
        files: &[TryFile],
    ) -> Result<TryResult, String> {
        let (mut trial, listed) = self.with_index(|index| {
            let rules_now = index.rules()?;
            let spec = RuleSpec {
                confirmed: true,
                enabled: true,
                ..candidate.spec.clone()
            };
            let (rules_after, candidate_id) = with_candidate(&rules_now, candidate.replaces, spec);
            let in_review: HashSet<_> = index
                .open_reviews()?
                .into_iter()
                .map(|entry| entry.document_id)
                .collect();
            let mut listed = Vec::new();
            for (at, location) in self.locations.iter().enumerate() {
                for (id, absolute) in index.documents_under(&location.path)? {
                    listed.push((at, id, absolute));
                }
            }
            let trial = Trial {
                rules_now,
                rules_after,
                candidate_id,
                aliases: index.confirmed_sender_aliases()?,
                in_review,
                kinds: Kinds::new(Arc::clone(index.kinds()), self.kind_language),
                movable_root: self.target.as_deref(),
                tried: Vec::new(),
                moves: Vec::new(),
            };
            Ok((trial, listed))
        })?;
        let mut seen: HashSet<PathBuf> = HashSet::new();
        for (at, id, absolute) in listed {
            seen.insert(absolute.clone());
            let Some(location) = self.locations.get(at) else {
                continue;
            };
            let subject = self.with_index(|index| {
                let Some(text) = index.document_text(id)? else {
                    return Ok(None);
                };
                let Some(document) = index.document(id)? else {
                    return Ok(None);
                };
                Ok(Some(Subject {
                    location,
                    absolute,
                    text,
                    fields: document.fields,
                    manual_fields: index.field_origins(id)?.manual_fields(),
                    hash: document.hash,
                    id: Some(id),
                }))
            })?;
            if let Some(subject) = subject {
                trial.consider(subject);
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
            trial.consider(Subject {
                location,
                absolute: file.path.clone(),
                text: file.text.clone(),
                fields: file.fields.clone(),
                manual_fields: Vec::new(),
                hash: file.hash,
                id: None,
            });
        }
        let Trial {
            rules_after,
            mut tried,
            moves,
            ..
        } = trial;
        let (archive_moves, archive_blocked) = order_moves(moves);
        tried.sort_by(|left, right| (left.section, &left.path).cmp(&(right.section, &right.path)));
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
            archive_blocked,
            not_indexed,
            more,
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

/// What the rule is tried with, and what the documents tried so far came to.
struct Trial<'a> {
    rules_now: Vec<Rule>,
    rules_after: Vec<Rule>,
    candidate_id: RuleId,
    aliases: SenderAliases,
    in_review: HashSet<DocumentId>,
    kinds: Kinds,
    /// Only the documents of this archive are moved; the other archives are
    /// read-only and only teach.
    movable_root: Option<&'a Path>,
    tried: Vec<TriedDocument>,
    moves: Vec<ArchiveMove>,
}

impl Trial<'_> {
    fn consider(&mut self, subject: Subject<'_>) {
        let location = subject.location;
        let absolute = subject.absolute;
        let path = absolute
            .strip_prefix(&location.path)
            .map_or_else(|_| absolute.clone(), Path::to_path_buf);
        let input = PlanInput {
            text: &subject.text,
            fields: &subject.fields,
            path: Some(&path),
            location: Some(&location.path),
            sender_aliases: &self.aliases,
            manual_fields: &subject.manual_fields,
            kinds: &self.kinds,
        };
        let after = plan(&self.rules_after, &input);
        let matching = matching_rules(&after);
        if !matching.contains(&self.candidate_id) {
            return;
        }
        let archived = location.mode == LocationMode::Archive;
        let now = if archived {
            TryOutcome::Lies(path.clone())
        } else {
            outcome(&plan(&self.rules_now, &input), &path)
        };
        let section = if archived {
            TrySection::Archive
        } else if subject.id.is_some_and(|id| self.in_review.contains(&id)) {
            TrySection::Review
        } else {
            TrySection::Inbox
        };
        let after_outcome = outcome(&after, &path);
        let movable = self.movable_root == Some(location.path.as_path());
        if let (true, true, TryOutcome::Filed(target)) = (archived, movable, &after_outcome) {
            if *target != path {
                self.moves.push(ArchiveMove {
                    root: location.path.clone(),
                    from: path.clone(),
                    to: target.clone(),
                    hash: subject.hash,
                });
            }
        }
        self.tried.push(TriedDocument {
            section,
            path,
            now,
            after: after_outcome,
            conflicts: matching
                .iter()
                .copied()
                .filter(|other| *other != self.candidate_id)
                .collect(),
        });
    }
}

/// The moves in the order they can run, and how many cannot run at all: a
/// name that two documents want goes to the first, and a swap or a circle of
/// renames has no first step. A move whose target is the source of another
/// move waits for that one.
fn order_moves(mut moves: Vec<ArchiveMove>) -> (Vec<ArchiveMove>, usize) {
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
