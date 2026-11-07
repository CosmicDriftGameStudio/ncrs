//! A reorganisation plan for an archive location: what looks wrong and where
//! it would go. A dry run: pure, nothing here touches a file or the index, and
//! nothing deletes. The plan only proposes; execution checks every item again
//! against the file on disk.
//!
//! The text of a document is data. It is only searched by rule conditions,
//! never acted on.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::Datelike as _;
use ncrs_index::{
    ConflictReason, ContentHash, DocumentFields, Finding, FindingKind, Kinds, NoActionReason,
    PlanItem, PlanOutcome, Rule, RuleEvidence, SenderAliases, SubtreePattern,
};

use crate::archive_profile::{
    pattern_per_document, propose_target, year_of, ArchiveDocument, DocumentPattern,
};
use crate::filing_plan::{plan, PlanInput, Planning};
use crate::rule_suggestions::evidence_meets_thresholds;
use crate::sanitise::stays_below;
use crate::structure::LocationStructure;

/// When several findings apply to a file, the first of these decides the
/// action. All of them are stored with the item.
pub const FINDING_PRIORITY: [FindingKind; 4] = [
    FindingKind::Duplicate,
    FindingKind::Misplaced,
    FindingKind::YearMismatch,
    FindingKind::NameOutlier,
];

/// A file of the location, as the index and the disk know it. A copy of a
/// document is a file of its own with the document's hash, fields and text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanFile {
    /// Below the root of the location.
    pub relative_path: PathBuf,
    pub hash: ContentHash,
    pub fields: DocumentFields,
    pub text: Option<String>,
    /// Seconds since the epoch when the index first saw this path; of copies
    /// with the same hash, the oldest one stays when none fits its folder.
    pub recorded_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRule {
    pub rule: Rule,
    /// What a suggested rule was stored with; `None` for rules made by hand.
    pub evidence: Option<RuleEvidence>,
}

#[derive(Debug, Clone, Copy)]
pub struct ReorganisationInput<'a> {
    pub files: &'a [PlanFile],
    /// As `learn_archive` returns it for these files.
    pub profile: &'a [SubtreePattern],
    /// As `learn_structure` returns it for these files: a copy lying in an
    /// unordered folder is the one that can go.
    pub unordered: &'a LocationStructure,
    pub rules: &'a [PlanRule],
    /// The confirmed names a sender condition treats as one sender, as
    /// intake and filing plan with them.
    pub sender_aliases: &'a SenderAliases,
    /// Everything that is on disk below the location, as relative paths. A
    /// target in here is a conflict.
    pub existing: &'a BTreeSet<PathBuf>,
    /// The language a `{kind}` folder of a proposed target is written in.
    pub kinds: &'a Kinds,
}

/// What one finding would do to the file.
enum Proposal {
    Move(PathBuf),
    Rename(PathBuf),
    NoAction(NoActionReason),
}

struct Candidate {
    finding: Finding,
    proposal: Proposal,
}

struct Resolved {
    index: usize,
    findings: Vec<Finding>,
    proposal: Proposal,
}

/// One item per file with at least one finding, sorted by source.
pub fn plan_reorganisation(input: &ReorganisationInput<'_>) -> Vec<PlanItem> {
    let mut files: Vec<&PlanFile> = input.files.iter().collect();
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

    let documents: Vec<ArchiveDocument> = files
        .iter()
        .map(|file| ArchiveDocument {
            relative_path: file.relative_path.clone(),
            date: file.fields.date,
        })
        .collect();
    let patterns = pattern_per_document(&documents);
    let context = Context {
        profile: input.profile,
        unordered: input.unordered,
        eligible: eligible_rules(input.rules, files.len()),
        sender_aliases: input.sender_aliases,
        files: &files,
        patterns: &patterns,
        kinds: input.kinds,
    };
    let keepers = duplicate_keepers(&context);

    let mut resolved = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let mut candidates = Vec::new();
        candidates.extend(context.duplicate(keepers.get(&index)));
        candidates.extend(context.misplaced(file));
        candidates.extend(context.year_mismatch(file));
        candidates.extend(context.name_outlier(file, index));
        if let Some(decided) = decide(candidates) {
            resolved.push(Resolved {
                index,
                findings: decided.0,
                proposal: decided.1,
            });
        }
    }
    outcomes(&resolved, &files, input.existing)
}

/// All findings, and the proposal of the one with the highest priority.
fn decide(mut candidates: Vec<Candidate>) -> Option<(Vec<Finding>, Proposal)> {
    candidates.sort_by_key(|candidate| priority_of(candidate.finding.kind()));
    let mut ordered = candidates.into_iter();
    let first = ordered.next()?;
    let findings = std::iter::once(first.finding)
        .chain(ordered.map(|candidate| candidate.finding))
        .collect();
    Some((findings, first.proposal))
}

fn priority_of(kind: FindingKind) -> usize {
    FINDING_PRIORITY
        .iter()
        .position(|&ranked| ranked == kind)
        .unwrap_or(FINDING_PRIORITY.len())
}

struct Context<'a> {
    profile: &'a [SubtreePattern],
    unordered: &'a LocationStructure,
    eligible: Vec<Rule>,
    sender_aliases: &'a SenderAliases,
    files: &'a [&'a PlanFile],
    patterns: &'a [Option<DocumentPattern>],
    kinds: &'a Kinds,
}

/// Confirmed rules, then suggestions whose stored evidence meets the B2
/// thresholds. Suggestions the user discarded are gone from the index and
/// never get here.
fn eligible_rules(rules: &[PlanRule], documents: usize) -> Vec<Rule> {
    let confirmed = rules.iter().filter(|entry| entry.rule.spec.confirmed);
    let qualified = rules.iter().filter(|entry| {
        !entry.rule.spec.confirmed
            && entry.evidence.as_ref().is_some_and(|evidence| {
                evidence_meets_thresholds(&entry.rule.spec.conditions, evidence, documents)
            })
    });
    confirmed
        .map(|entry| entry.rule.clone())
        .chain(qualified.map(|entry| {
            let mut rule = entry.rule.clone();
            rule.spec.confirmed = true;
            rule
        }))
        .collect()
}

impl Context<'_> {
    fn own(&self, index: usize) -> Option<&DocumentPattern> {
        self.patterns.get(index).and_then(Option::as_ref)
    }

    fn learned(&self, subtree: &str) -> Option<&SubtreePattern> {
        self.profile
            .iter()
            .find(|entry| entry.subtree == subtree && entry.pattern.is_some())
    }

    /// The file name pattern the folder follows when the file's own differs.
    fn name_deviation(&self, index: usize) -> Option<(&SubtreePattern, String)> {
        let own = self.own(index)?;
        let learned = self.learned(&own.subtree)?;
        let expected = file_name_pattern(learned.pattern.as_deref()?);
        (file_name_pattern(&own.pattern) != expected).then(|| (learned, expected.to_owned()))
    }

    fn duplicate(&self, keeper_index: Option<&usize>) -> Option<Candidate> {
        let keeper = self.files.get(*keeper_index?)?;
        Some(Candidate {
            finding: Finding::Duplicate {
                keeper: keeper.relative_path.clone(),
            },
            proposal: Proposal::NoAction(NoActionReason::DuplicateCopy),
        })
    }

    fn misplaced(&self, file: &PlanFile) -> Option<Candidate> {
        let input = PlanInput {
            text: file.text.as_deref().unwrap_or_default(),
            fields: &file.fields,
            path: Some(&file.relative_path),
            location: None,
            sender_aliases: self.sender_aliases,
            manual_fields: &[],
            kinds: self.kinds,
        };
        let Planning::Plan(filing) = plan(&self.eligible, &input) else {
            return None;
        };
        if filing.relative_path.parent() == file.relative_path.parent() {
            return None;
        }
        Some(Candidate {
            finding: Finding::Misplaced { rule: filing.rule },
            proposal: Proposal::Move(filing.relative_path),
        })
    }

    fn year_mismatch(&self, file: &PlanFile) -> Option<Candidate> {
        let (folder_year, date_year, fixed) = year_fix(file)?;
        Some(Candidate {
            finding: Finding::YearMismatch {
                folder_year,
                date_year,
            },
            proposal: Proposal::Move(fixed),
        })
    }

    fn name_outlier(&self, file: &PlanFile, index: usize) -> Option<Candidate> {
        let (learned, expected) = self.name_deviation(index)?;
        let extension = file
            .relative_path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default();
        let renamed = propose_target(learned, &file.fields, self.kinds, extension)
            .and_then(|rendered| rendered.file_name().map(ToOwned::to_owned))
            .map(|name| file.relative_path.with_file_name(name));
        let proposal = match renamed {
            Some(target) if target == file.relative_path => return None,
            Some(target) => Proposal::Rename(target),
            None => Proposal::NoAction(NoActionReason::MissingFields),
        };
        Some(Candidate {
            finding: Finding::NameOutlier { pattern: expected },
            proposal,
        })
    }
}

fn file_name_pattern(pattern: &str) -> &str {
    pattern.rsplit('/').next().unwrap_or(pattern)
}

/// The first year folder that is not the year of the document's date, that
/// year, and the path with every year folder set to the date's year. Only the
/// year segments change.
fn year_fix(file: &PlanFile) -> Option<(i32, i32, PathBuf)> {
    let date_year = file.fields.date?.year();
    let folders = file.relative_path.parent()?;
    let folder_year = folders
        .components()
        .filter_map(|component| year_of(component.as_os_str().to_str()?))
        .find(|&year| year != date_year)?;
    if mentions_year(file, folder_year) {
        return None;
    }
    let mut fixed = PathBuf::new();
    for component in folders.components() {
        let is_year = component.as_os_str().to_str().and_then(year_of).is_some();
        if is_year {
            fixed.push(format!("{date_year:04}"));
        } else {
            fixed.push(component.as_os_str());
        }
    }
    fixed.push(file.relative_path.file_name()?);
    Some((folder_year, date_year, fixed))
}

/// A notice dated after its folder's year is normal when its name names that
/// year.
fn mentions_year(file: &PlanFile, year: i32) -> bool {
    let wanted = format!("{year:04}");
    file.relative_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| {
            stem.split(|character: char| !character.is_alphanumeric())
                .any(|token| token == wanted)
        })
}

/// For every surplus copy lying in an unordered folder, the index of the copy
/// that stays. Copies in ordered folders stay and have no entry. The keeper is
/// one of them; when all copies are unordered, one of those. Of several, the
/// one that fits its folder (its name follows the learned pattern and its year
/// folder is its date's year), then the one the index saw first, then the
/// first path.
fn duplicate_keepers(context: &Context<'_>) -> BTreeMap<usize, usize> {
    let mut groups: BTreeMap<[u8; 32], Vec<usize>> = BTreeMap::new();
    for (index, file) in context.files.iter().enumerate() {
        groups.entry(file.hash.0).or_default().push(index);
    }
    let mut keepers = BTreeMap::new();
    for members in groups.values().filter(|members| members.len() > 1) {
        let fits = |index: &usize| {
            context.name_deviation(*index).is_none()
                && context
                    .files
                    .get(*index)
                    .is_some_and(|file| year_fix(file).is_none())
        };
        let is_unordered = |index: &usize| {
            context
                .files
                .get(*index)
                .is_some_and(|file| context.unordered.is_unordered(&file.relative_path))
        };
        let ordered: Vec<usize> = members
            .iter()
            .copied()
            .filter(|index| !is_unordered(index))
            .collect();
        let candidates = if ordered.is_empty() {
            members.clone()
        } else {
            ordered
        };
        let fitting: Vec<usize> = candidates.iter().copied().filter(fits).collect();
        let pool = if fitting.is_empty() {
            candidates
        } else {
            fitting
        };
        let seniority = |index: &usize| {
            context
                .files
                .get(*index)
                .map(|file| (file.recorded_at, file.relative_path.clone()))
        };
        let Some(keeper) = pool.into_iter().min_by_key(seniority) else {
            continue;
        };
        let surplus = members
            .iter()
            .filter(|&&member| member != keeper && is_unordered(&member));
        for &member in surplus {
            keepers.insert(member, keeper);
        }
    }
    keepers
}

/// Turns the proposals into outcomes. A target is a conflict, not an action,
/// when it leaves the location, exists on disk, or is wanted by another item
/// of the plan (then every one of them is a conflict).
fn outcomes(
    resolved: &[Resolved],
    files: &[&PlanFile],
    existing: &BTreeSet<PathBuf>,
) -> Vec<PlanItem> {
    let existing_keys: BTreeSet<String> = existing.iter().map(|path| key_of(path)).collect();
    let mut claims: BTreeMap<String, usize> = BTreeMap::new();
    for entry in resolved {
        if let Some(target) = proposed_target(&entry.proposal).filter(|path| stays_below(path)) {
            *claims.entry(key_of(target)).or_default() += 1;
        }
    }
    resolved
        .iter()
        .filter_map(|entry| {
            let file = files.get(entry.index)?;
            let outcome = match &entry.proposal {
                Proposal::NoAction(reason) => PlanOutcome::NoAction(*reason),
                Proposal::Move(target) | Proposal::Rename(target) => {
                    let conflict = |reason, shown: Option<&PathBuf>| PlanOutcome::Conflict {
                        target: shown.cloned(),
                        reason,
                    };
                    let key = key_of(target);
                    if !stays_below(target) {
                        conflict(ConflictReason::TargetLeavesLocation, None)
                    } else if key != key_of(&file.relative_path) && existing_keys.contains(&key) {
                        conflict(ConflictReason::TargetExists, Some(target))
                    } else if claims.get(&key).copied().unwrap_or(0) > 1 {
                        conflict(ConflictReason::TargetTakenByOtherItem, Some(target))
                    } else if matches!(entry.proposal, Proposal::Move(_)) {
                        PlanOutcome::Move {
                            target: target.clone(),
                        }
                    } else {
                        PlanOutcome::Rename {
                            target: target.clone(),
                        }
                    }
                }
            };
            Some(PlanItem {
                source: file.relative_path.clone(),
                hash: file.hash,
                findings: entry.findings.clone(),
                outcome,
            })
        })
        .collect()
}

fn proposed_target(proposal: &Proposal) -> Option<&PathBuf> {
    match proposal {
        Proposal::Move(target) | Proposal::Rename(target) => Some(target),
        Proposal::NoAction(_) => None,
    }
}

/// Case-insensitive: a case-insensitive file system treats `A.pdf` and
/// `a.pdf` as one file.
fn key_of(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

#[cfg(test)]
mod archive_tests;
