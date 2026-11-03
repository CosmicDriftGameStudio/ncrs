//! The files that stay in the trial, grouped by sender or kind, so that
//! Hermes can ask where a whole group goes instead of listing every file.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use ncrs_fs::flat_names::target_name;
use ncrs_index::{Condition, ConditionKind, Kinds, RuleConditions, RuleSpec, SenderAliases};
use ncrs_intake::{learn_archive, pattern_is_valid, ArchiveDocument, RuleProblem};

use super::reading::FolderReading;
use super::trial::{StayReason, TrialEntry, TrialOutcome, TrialPlan};

/// Most groups Hermes asks about in one tidy-up.
pub const MAX_GROUP_QUESTIONS: usize = 6;
const EXAMPLES: usize = 3;
const MAX_AREAS: usize = 3;

/// What a group has in common. The text is what the sender condition of a
/// rule compares (lower case, through the sender aliases) or the key of the
/// kind.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GroupKey {
    Sender(String),
    Kind(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StayGroup {
    pub key: GroupKey,
    /// The sender as the rule spells it, or the word of the kind.
    pub label: String,
    pub entries: Vec<TrialEntry>,
    pub examples: Vec<String>,
    /// First-level folders of the target Hermes offers, best first.
    pub areas: Vec<String>,
}

impl StayGroup {
    pub fn count(&self) -> usize {
        self.entries.len()
    }
}

/// A file whose name is taken in the folder it belongs in, and a free name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameTaken {
    /// Position in the entries of the plan.
    pub entry: usize,
    pub name: String,
    pub suggested: PathBuf,
}

/// What the user already decided about groups in this tidy-up.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnsweredGroups {
    /// Groups asked about so far, answered with a rule or left alone.
    pub questions: usize,
    pub left: Vec<GroupKey>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StayGroups {
    /// The groups to ask about, the largest first, at most
    /// `MAX_GROUP_QUESTIONS` together with the ones already answered.
    pub asked: Vec<StayGroup>,
    /// The other staying files: smaller groups and single pieces. Files of
    /// groups the user left alone are not in it.
    pub rest: Vec<TrialEntry>,
    pub name_taken: Vec<NameTaken>,
    /// Groups of two files or more, whether asked about or not.
    pub groups: usize,
    /// Files that belong to no group: no sender and no kind, or alone.
    pub single_pieces: usize,
}

impl StayGroups {
    pub fn has_questions(&self) -> bool {
        !self.asked.is_empty() || !self.rest.is_empty() || !self.name_taken.is_empty()
    }

    /// Files the first `questions` groups cover.
    pub fn files_in_first(&self, questions: usize) -> usize {
        self.asked
            .iter()
            .take(questions)
            .map(StayGroup::count)
            .sum()
    }
}

struct Forming {
    label: String,
    entries: Vec<TrialEntry>,
}

/// Where the files of the target lie, by first-level folder.
struct Areas {
    total: BTreeMap<String, usize>,
    by_sender: HashMap<(String, String), usize>,
    by_kind: HashMap<(String, String), usize>,
}

fn area_of(path: &Path) -> Option<String> {
    let mut parts = path.components();
    let first = parts.next()?;
    parts.next()?;
    match first {
        Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
        _ => None,
    }
}

fn first_folder(path: &Path) -> Option<String> {
    match path.components().next()? {
        Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
        _ => None,
    }
}

impl Areas {
    fn of(target: &FolderReading, kinds: &Kinds, aliases: &SenderAliases) -> Self {
        let mut areas = Self {
            total: BTreeMap::new(),
            by_sender: HashMap::new(),
            by_kind: HashMap::new(),
        };
        for file in &target.files {
            if target.structure.is_unordered(&file.relative_path) {
                continue;
            }
            let Some(area) = area_of(&file.relative_path) else {
                continue;
            };
            if let Some(sender) = file.fields.sender.as_deref() {
                let key = aliases.canonical_sender(sender).to_lowercase();
                *areas.by_sender.entry((area.clone(), key)).or_default() += 1;
            }
            if let Some(kind) = file.fields.kind.as_deref() {
                let key = kinds.catalog.kind_key(kind);
                *areas.by_kind.entry((area.clone(), key)).or_default() += 1;
            }
            *areas.total.entry(area).or_default() += 1;
        }
        areas
    }

    /// The neighbour folder of the files without a pattern first, then the
    /// areas whose files most often share the sender or the kind, then the
    /// largest areas.
    fn offered(&self, key: &GroupKey, entries: &[TrialEntry]) -> Vec<String> {
        let mut known_folder: BTreeMap<String, usize> = BTreeMap::new();
        for entry in entries {
            let neighbour_area = entry.neighbour.as_deref().and_then(first_folder);
            if let Some(known) = neighbour_area.filter(|name| self.total.contains_key(name)) {
                *known_folder.entry(known).or_default() += 1;
            }
        }
        let mut from_neighbours: Vec<(String, usize)> = known_folder.into_iter().collect();
        from_neighbours.sort_by_key(|(area, count)| (Reverse(*count), area.clone()));

        let shared = |area: &String| match key {
            GroupKey::Sender(sender) => self.by_sender.get(&(area.clone(), sender.clone())),
            GroupKey::Kind(kind) => self.by_kind.get(&(area.clone(), kind.clone())),
        };
        let mut sharing: Vec<(String, usize)> = self
            .total
            .keys()
            .filter_map(|area| shared(area).map(|count| (area.clone(), *count)))
            .collect();
        sharing.sort_by_key(|(area, count)| (Reverse(*count), area.clone()));

        let mut largest: Vec<(&String, &usize)> = self.total.iter().collect();
        largest.sort_by_key(|(area, count)| (Reverse(**count), (*area).clone()));

        let mut offered: Vec<String> = Vec::new();
        let candidates = from_neighbours
            .into_iter()
            .map(|(area, _)| area)
            .chain(sharing.into_iter().map(|(area, _)| area))
            .chain(largest.into_iter().map(|(area, _)| area.clone()));
        for area in candidates {
            if offered.len() == MAX_AREAS {
                break;
            }
            if !offered.contains(&area) {
                offered.push(area);
            }
        }
        offered
    }
}

fn grouped_by(
    entry: &TrialEntry,
    kinds: &Kinds,
    aliases: &SenderAliases,
) -> Option<(GroupKey, String)> {
    let written_sender = entry
        .fields
        .sender
        .as_deref()
        .map(str::trim)
        .filter(|written| !written.is_empty());
    if let Some(sender) = written_sender {
        let label = aliases.canonical_sender(sender).to_owned();
        return Some((GroupKey::Sender(label.to_lowercase()), label));
    }
    let kind = entry
        .fields
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|kind| !kind.is_empty())?;
    Some((
        GroupKey::Kind(kinds.catalog.kind_key(kind)),
        kinds.kind_word(kind).into_owned(),
    ))
}

fn stays_for_a_group(entry: &TrialEntry) -> bool {
    matches!(
        entry.outcome,
        TrialOutcome::Stays(
            StayReason::NoNeighbours | StayReason::NoPattern | StayReason::MissingField(_)
        )
    )
}

/// The first free `stem (n).ext` next to `wanted`, compared without case and
/// against everything in `taken`. The name is claimed in `taken`.
pub fn numbered_name(wanted: &Path, taken: &mut BTreeSet<PathBuf>) -> PathBuf {
    let folder = wanted.parent().unwrap_or_else(|| Path::new(""));
    let mut names: HashSet<String> = taken
        .iter()
        .filter(|path| path.parent().unwrap_or_else(|| Path::new("")) == folder)
        .filter_map(|path| path.file_name())
        .map(|name| name.to_string_lossy().to_lowercase())
        .collect();
    let renamed = match target_name(wanted, &mut names) {
        Some(name) => folder.join(name),
        None => wanted.to_path_buf(),
    };
    taken.insert(renamed.clone());
    renamed
}

pub fn stay_groups(
    plan: &TrialPlan,
    kinds: &Kinds,
    aliases: &SenderAliases,
    target: &FolderReading,
    answered: &AnsweredGroups,
) -> StayGroups {
    let mut forming: BTreeMap<GroupKey, Forming> = BTreeMap::new();
    let mut singles: Vec<&TrialEntry> = Vec::new();
    let mut name_taken = Vec::new();
    let mut taken = plan.taken.clone();
    for (position, entry) in plan.entries.iter().enumerate() {
        if entry.outcome == TrialOutcome::Stays(StayReason::NameTaken) {
            if let Some(wanted) = entry.wanted.as_deref() {
                name_taken.push(NameTaken {
                    entry: position,
                    name: entry.name.clone(),
                    suggested: numbered_name(wanted, &mut taken),
                });
            }
        } else if stays_for_a_group(entry) {
            match grouped_by(entry, kinds, aliases) {
                Some((key, label)) => forming
                    .entry(key)
                    .or_insert_with(|| Forming {
                        label,
                        entries: Vec::new(),
                    })
                    .entries
                    .push(entry.clone()),
                None => singles.push(entry),
            }
        }
    }

    let (mut grouped, alone): (Vec<_>, Vec<_>) = forming
        .into_iter()
        .partition(|(_, group)| group.entries.len() >= 2);
    let mut single_pieces: Vec<&TrialEntry> = singles;
    single_pieces.extend(alone.iter().flat_map(|(_, group)| group.entries.iter()));
    grouped.sort_by_key(|(_, group)| (Reverse(group.entries.len()), group.label.clone()));
    let group_count = grouped.len();

    let budget = MAX_GROUP_QUESTIONS.saturating_sub(answered.questions);
    let areas = Areas::of(target, kinds, aliases);
    let mut asked = Vec::new();
    let mut not_asked: Vec<TrialEntry> = Vec::new();
    for (key, group) in grouped {
        if answered.left.contains(&key) {
            continue;
        }
        if asked.len() >= budget {
            not_asked.extend(group.entries);
            continue;
        }
        asked.push(StayGroup {
            examples: group
                .entries
                .iter()
                .take(EXAMPLES)
                .map(|entry| entry.name.clone())
                .collect(),
            areas: areas.offered(&key, &group.entries),
            key,
            label: group.label,
            entries: group.entries,
        });
    }
    let single_count = single_pieces.len();
    let mut rest: Vec<TrialEntry> = single_pieces.into_iter().cloned().collect();
    rest.extend(not_asked);
    StayGroups {
        asked,
        rest,
        name_taken,
        groups: group_count,
        single_pieces: single_count,
    }
}

/// The pattern the target follows in a flat `area`. An area with subfolders
/// has a pattern per subfolder, none for the area; the files then go into the
/// area itself and get the name the filing makes from date, sender and number.
pub fn area_pattern(target: &FolderReading, area: &str) -> String {
    let documents: Vec<ArchiveDocument> = target
        .files
        .iter()
        .filter(|file| !target.structure.is_unordered(&file.relative_path))
        .map(|file| ArchiveDocument {
            relative_path: file.relative_path.clone(),
            date: file.fields.date,
        })
        .collect();
    learn_archive(&documents)
        .into_iter()
        .find(|learned| learned.subtree == area)
        .and_then(|learned| learned.pattern)
        .filter(|pattern| pattern_is_valid(pattern))
        .unwrap_or_else(|| format!("{area}/"))
}

/// Why a rule of the chat was not stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleSaveFailure {
    /// The check the rule form runs refused it; nothing was written.
    Refused(RuleProblem),
    Failed(String),
}

/// The rule that files a whole group below `area`.
pub fn group_rule(key: &GroupKey, label: &str, area: &str, target: &FolderReading) -> RuleSpec {
    let condition = match key {
        GroupKey::Sender(_) => Condition::new(ConditionKind::Sender, label),
        GroupKey::Kind(kind) => Condition::new(ConditionKind::Kind, kind.as_str()),
    };
    let pattern = area_pattern(target, area);
    RuleSpec {
        name: Some(label.to_owned()),
        conditions: RuleConditions::all_of(vec![condition]),
        extract_number: None,
        extract_date: None,
        extract_amount: None,
        kind: None,
        tags: BTreeSet::new(),
        filing_pattern: pattern,
        confirmed: true,
        enabled: true,
    }
}
