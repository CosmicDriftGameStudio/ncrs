//! A reorganisation plan for an archive location: what looks wrong and where
//! it would go. A dry run: pure, nothing here touches a file or the index, and
//! nothing deletes. The plan only proposes; execution checks every item again
//! against the file on disk.
//!
//! The text of a document is data. It is only searched by rule conditions,
//! never acted on.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use chrono::Datelike as _;
use ncrs_index::{
    ConflictReason, ContentHash, DocumentFields, Finding, FindingKind, NoActionReason, PlanItem,
    PlanOutcome, Rule, RuleEvidence, SenderAliases, SubtreePattern,
};

use crate::archive_profile::{
    pattern_per_document, propose_target, year_of, ArchiveDocument, DocumentPattern,
};
use crate::filing_plan::{plan, PlanInput, Planning};
use crate::rule_suggestions::evidence_meets_thresholds;
use crate::sanitise::stays_below;

/// Where the copies of a duplicate go, at the root of the location, below
/// their original relative path so that copies with the same name differ.
pub const DUPLICATES_FOLDER: &str = "duplicates";

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
    pub rules: &'a [PlanRule],
    /// Everything that is on disk below the location, as relative paths. A
    /// target in here is a conflict.
    pub existing: &'a BTreeSet<PathBuf>,
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

/// One item per file with at least one finding, sorted by source. Files
/// already below `DUPLICATES_FOLDER` are left out: they are where a plan put
/// them.
pub fn plan_reorganisation(input: &ReorganisationInput<'_>) -> Vec<PlanItem> {
    let mut files: Vec<&PlanFile> = input
        .files
        .iter()
        .filter(|file| !is_in_duplicates_folder(&file.relative_path))
        .collect();
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
        eligible: eligible_rules(input.rules, files.len()),
        files: &files,
        patterns: &patterns,
    };
    let keepers = duplicate_keepers(&context);

    let mut resolved = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let mut candidates = Vec::new();
        candidates.extend(context.duplicate(file, keepers.get(&index)));
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

fn is_in_duplicates_folder(relative: &Path) -> bool {
    relative.components().next() == Some(Component::Normal(OsStr::new(DUPLICATES_FOLDER)))
        && relative.components().count() > 1
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
    eligible: Vec<Rule>,
    files: &'a [&'a PlanFile],
    patterns: &'a [Option<DocumentPattern>],
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

    fn duplicate(&self, file: &PlanFile, keeper_index: Option<&usize>) -> Option<Candidate> {
        let keeper = self.files.get(*keeper_index?)?;
        Some(Candidate {
            finding: Finding::Duplicate {
                keeper: keeper.relative_path.clone(),
            },
            proposal: Proposal::Move(Path::new(DUPLICATES_FOLDER).join(&file.relative_path)),
        })
    }

    fn misplaced(&self, file: &PlanFile) -> Option<Candidate> {
        let input = PlanInput {
            text: file.text.as_deref().unwrap_or_default(),
            fields: &file.fields,
            path: Some(&file.relative_path),
            sender_aliases: &SenderAliases::default(),
            tax_contexts: &[],
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
        let renamed = propose_target(learned, &file.fields, extension)
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

/// For every file that is a surplus copy, the index of the copy that stays.
/// The copy that fits its folder (its name follows the learned pattern and
/// its year folder is its date's year) stays; of several, or when none fits,
/// the one the index saw first, then the first path.
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
        let fitting: Vec<usize> = members.iter().copied().filter(fits).collect();
        let pool = if fitting.is_empty() {
            members.clone()
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
        for &member in members.iter().filter(|&&member| member != keeper) {
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
