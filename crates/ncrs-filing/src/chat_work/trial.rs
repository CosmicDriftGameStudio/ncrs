//! The trial: where each file of a folder would go, decided without touching
//! anything.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ncrs_ai::{AiError, ChatModel, FolderChoiceLane};
use ncrs_index::{ContentHash, DocumentFields, Field, Kinds, Rule, SenderAliases};
use ncrs_intake::{
    plan, render_complete_pattern, render_pattern_folder, render_pattern_in, Candidate,
    NeighbourIndex, OwnNames, PlanFile, PlanInput, Planning,
};

use super::folder_list::FolderList;
use super::groups::{stay_groups, AnsweredGroups, StayGroups};
use super::pattern::{layout_path, PatternLevel};
use super::{ChatAccess, FolderReading};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StayReason {
    NoNeighbours,
    MissingField(Field),
    NoPattern,
    NameTaken,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrialOutcome {
    /// Relative to the target.
    Filed {
        to: PathBuf,
    },
    /// Folders below the target, the one with most similar files first, and
    /// for each the name the file gets there: the same position in both.
    Unsure {
        choices: Vec<PathBuf>,
        file_names: Vec<String>,
        kind_label: Option<String>,
    },
    /// Relative to the target.
    AlreadyThere {
        at: PathBuf,
    },
    Stays(StayReason),
}

/// What gave a file its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacedBy {
    /// A confirmed rule.
    Rule,
    /// Files of the target that look alike.
    Neighbours,
    /// Only the order of a target without neighbours.
    Layout,
    /// The model chose the folder the neighbours offered first.
    ModelAndNeighbours,
    /// The model's folder alone: the neighbours offered none or another one.
    Model,
}

impl PlacedBy {
    /// Only a rule, neighbours or the model agreeing with them make a place
    /// sure; the model alone stays unsure, like the order of a bare target.
    pub fn is_sure(self) -> bool {
        !matches!(self, Self::Layout)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrialEntry {
    pub from: PathBuf,
    pub name: String,
    pub hash: ContentHash,
    pub fields: DocumentFields,
    /// The kind as a word of the user's language, as typed when unknown.
    pub kind_word: Option<String>,
    pub outcome: TrialOutcome,
    /// Relative to the target: the folder the neighbours pointed to, where
    /// there was one.
    pub neighbour: Option<PathBuf>,
    /// Relative to the target: where the file would go when its name is taken.
    pub wanted: Option<PathBuf>,
    /// What gave the place of a filed file; `None` for the others.
    pub placed_by: Option<PlacedBy>,
}

/// In the order the chat shows them: one filed file per area, the unsure
/// ones, the other filed files, the ones already there, the ones that stay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrialPlan {
    pub source_root: PathBuf,
    pub target_root: PathBuf,
    pub entries: Vec<TrialEntry>,
    pub filed: usize,
    pub unsure: usize,
    pub already_there: usize,
    pub stays: usize,
    /// Paths below the target that are in use: what lies there and what the
    /// filed entries claim.
    pub taken: BTreeSet<PathBuf>,
    /// The staying files, grouped for the questions.
    pub groups: StayGroups,
    /// The files that still need the model's folder: they are not decided yet
    /// and get no group question.
    pub model_left: BTreeSet<PathBuf>,
    /// Why the model could not answer for `model_left`, when it could not.
    /// Those files keep what the neighbours gave them and are not stored as
    /// "no folder fits".
    pub model_failure: Option<AiError>,
}

/// What ends a planning run early.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanFailure {
    /// The model could not answer; the plan so far is still good.
    Model(AiError),
    Other(String),
}

impl From<String> for PlanFailure {
    fn from(reason: String) -> Self {
        Self::Other(reason)
    }
}

impl std::fmt::Display for PlanFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Model(error) => write!(f, "{error}"),
            Self::Other(reason) => f.write_str(reason),
        }
    }
}

/// How many files the model looks at for the first trial page.
pub const MODEL_FIRST_PAGE: usize = 10;

/// Which of the files that need the model are asked now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelScope {
    /// The first files in source order, as many as the trial page shows.
    FirstPage,
    /// Only these files, by their source path.
    Only(BTreeSet<PathBuf>),
    /// The last files in source order, as many as a page, in the background
    /// lane; the files before them are left for later.
    LastPage,
}

impl TrialPlan {
    /// Claims `to` for a file; false when the name is in use.
    pub fn claim(&mut self, to: &Path) -> bool {
        self.taken.insert(to.to_path_buf())
    }
}

fn with_extension_of(rendered: PathBuf, name: &str) -> PathBuf {
    let Some(stem) = rendered.file_name().and_then(|stem| stem.to_str()) else {
        return rendered;
    };
    match Path::new(name).extension().and_then(|ext| ext.to_str()) {
        Some(extension) => rendered.with_file_name(format!("{stem}.{extension}")),
        None => rendered,
    }
}

pub fn trial(
    access: &ChatAccess,
    source: &FolderReading,
    target: &FolderReading,
    answered: &AnsweredGroups,
    layout: Option<&[PatternLevel]>,
    model: Option<&dyn ChatModel>,
    scope: &ModelScope,
) -> Result<TrialPlan, String> {
    let kinds = access.kinds()?;
    let (rules, aliases) = access.rules_and_aliases()?;
    let own = OwnNames {
        names: &access.own.names,
        ibans: &access.own.ibans,
        tax_ids: &access.own.tax_ids,
    };
    let neighbours = NeighbourIndex::new(&target.files, &own, &target.structure)
        .with_kinds(Arc::clone(&kinds.catalog));
    let mut taken: BTreeSet<PathBuf> = target
        .files
        .iter()
        .map(|file| file.relative_path.clone())
        .collect();

    let folder_list = FolderList::of(&target.files);
    // Where the first file of each content in the source goes: a second file
    // with the same content is a duplicate of it and is not filed again.
    let mut filed_from_source: HashMap<ContentHash, PathBuf> = HashMap::new();
    let mut candidate_lists = Vec::with_capacity(source.files.len());
    let mut entries = Vec::with_capacity(source.files.len());
    for file in &source.files {
        let name = file
            .relative_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut decided = Decided::outcome(TrialOutcome::Stays(StayReason::NoNeighbours));
        if let Some(at) = target
            .hashes
            .get(&file.hash)
            .or_else(|| filed_from_source.get(&file.hash))
        {
            decided.outcome = TrialOutcome::AlreadyThere { at: at.clone() };
        } else {
            let ctx = Deciding {
                rules: &rules,
                aliases: &aliases,
                neighbours: &neighbours,
                kinds: &kinds,
                location: &source.root,
                layout,
                folder_list: &folder_list,
            };
            decided = decide(&ctx, file, &name, &mut taken);
            if let TrialOutcome::Filed { to } = &decided.outcome {
                filed_from_source.insert(file.hash, to.clone());
            }
        }
        candidate_lists.push(decided.candidates);
        entries.push(TrialEntry {
            from: source.root.join(&file.relative_path),
            name,
            hash: file.hash,
            kind_word: file
                .fields
                .kind
                .as_deref()
                .map(|kind| kinds.kind_word(kind).into_owned()),
            fields: file.fields.clone(),
            outcome: decided.outcome,
            neighbour: decided.neighbour,
            wanted: decided.wanted,
            placed_by: decided.placed_by,
        });
    }

    let mut model_left = BTreeSet::new();
    let mut model_failure = None;
    if let Some(chat_model) = model {
        let group_keys: Vec<Option<String>> = source
            .files
            .iter()
            .map(|file| neighbours.group_key(&file.fields, file.text.as_deref()))
            .collect();
        let asked = ModelAsk {
            group_keys: &group_keys,
            access,
            model: chat_model,
            target_root: &target.root,
            folders: &folder_list,
            kinds: &kinds,
            files: &source.files,
            candidates: &candidate_lists,
        };
        (model_left, model_failure) = ask_model(&asked, &mut entries, &mut taken, scope);
    }

    let count = |matches_outcome: fn(&TrialOutcome) -> bool| {
        entries
            .iter()
            .filter(|entry| matches_outcome(&entry.outcome))
            .count()
    };
    let filed = count(|outcome| matches!(outcome, TrialOutcome::Filed { .. }));
    let unsure = count(|outcome| matches!(outcome, TrialOutcome::Unsure { .. }));
    let already_there = count(|outcome| matches!(outcome, TrialOutcome::AlreadyThere { .. }));
    // The group questions skip the files the model left, so they are not
    // counted as staying either.
    let stays = entries
        .iter()
        .filter(|entry| {
            matches!(entry.outcome, TrialOutcome::Stays(_)) && !model_left.contains(&entry.from)
        })
        .count();
    let mut plan = TrialPlan {
        source_root: source.root.clone(),
        target_root: target.root.clone(),
        entries: in_sample_order(entries),
        filed,
        unsure,
        already_there,
        stays,
        taken,
        groups: StayGroups::default(),
        model_left,
        model_failure,
    };
    plan.groups = stay_groups(&plan, &kinds, &aliases, target, answered);
    Ok(plan)
}

struct Deciding<'a> {
    rules: &'a [Rule],
    aliases: &'a SenderAliases,
    neighbours: &'a NeighbourIndex,
    kinds: &'a Kinds,
    /// The read folder the files lie in.
    location: &'a Path,
    /// How to file into a target no neighbour points the way in.
    layout: Option<&'a [PatternLevel]>,
    folder_list: &'a FolderList,
}

struct Decided {
    outcome: TrialOutcome,
    neighbour: Option<PathBuf>,
    wanted: Option<PathBuf>,
    placed_by: Option<PlacedBy>,
    /// The folders the neighbours offered for an unsure file.
    candidates: Vec<Candidate>,
}

impl Decided {
    fn outcome(outcome: TrialOutcome) -> Self {
        Self {
            outcome,
            neighbour: None,
            wanted: None,
            placed_by: None,
            candidates: Vec::new(),
        }
    }

    fn filed_or_taken(to: PathBuf, placed_by: PlacedBy, taken: &mut BTreeSet<PathBuf>) -> Self {
        if taken.insert(to.clone()) {
            Self {
                placed_by: Some(placed_by),
                ..Self::outcome(TrialOutcome::Filed { to })
            }
        } else {
            Self {
                wanted: Some(to),
                ..Self::outcome(TrialOutcome::Stays(StayReason::NameTaken))
            }
        }
    }
}

/// A stored rule that matches decides first; without one the neighbours do.
fn decide(
    ctx: &Deciding<'_>,
    file: &PlanFile,
    name: &str,
    taken: &mut BTreeSet<PathBuf>,
) -> Decided {
    let text = file.text.as_deref().unwrap_or_default();
    let planning = plan(
        ctx.rules,
        &PlanInput {
            text,
            fields: &file.fields,
            path: Some(&file.relative_path),
            location: Some(ctx.location),
            sender_aliases: ctx.aliases,
            manual_fields: &[],
            kinds: ctx.kinds,
        },
    );
    if let Planning::Plan(filing) = planning {
        let to = with_extension_of(filing.relative_path, name);
        return Decided::filed_or_taken(to, PlacedBy::Rule, taken);
    }
    let Some(placement) = ctx.neighbours.place(&file.fields, file.text.as_deref()) else {
        let Some(levels) = ctx.layout else {
            return Decided::outcome(TrialOutcome::Stays(StayReason::NoNeighbours));
        };
        return match layout_path(levels, &file.fields, ctx.kinds, name) {
            Some(to) => Decided::filed_or_taken(to, PlacedBy::Layout, taken),
            None => Decided::outcome(TrialOutcome::Stays(StayReason::NoPattern)),
        };
    };
    if !placement.is_sure() {
        let mut offers = unsure_offers(
            &placement.candidates,
            &file.fields,
            ctx.kinds,
            name,
            ctx.folder_list,
        );
        if offers.is_empty() {
            let mut folders = vec![placement.folder];
            folders.extend(placement.runner_up);
            offers = folders
                .into_iter()
                .map(|folder| (folder, name.to_owned()))
                .collect();
        }
        let (choices, file_names) = offers.into_iter().unzip();
        return Decided {
            candidates: placement.candidates,
            ..Decided::outcome(TrialOutcome::Unsure {
                choices,
                file_names,
                kind_label: known_kind_label(ctx.kinds, &file.fields)
                    .or_else(|| file.fields.sender.clone()),
            })
        };
    }
    if let Some(field) = placement.missing {
        return Decided {
            neighbour: Some(placement.folder),
            ..Decided::outcome(TrialOutcome::Stays(StayReason::MissingField(field)))
        };
    }
    let Some(rendered) = placement
        .pattern
        .as_deref()
        .and_then(|pattern| render_pattern_in(pattern, &file.fields, ctx.kinds))
    else {
        return Decided {
            neighbour: Some(placement.folder),
            ..Decided::outcome(TrialOutcome::Stays(StayReason::NoPattern))
        };
    };
    let to = with_extension_of(rendered, name);
    let mut decided = Decided::filed_or_taken(to, PlacedBy::Neighbours, taken);
    if decided.wanted.is_some() {
        decided.neighbour = Some(placement.folder);
    }
    decided
}

/// The kind as a word of the user's language, only when the catalog knows it.
fn known_kind_label(kinds: &Kinds, fields: &DocumentFields) -> Option<String> {
    let kind = fields.kind.as_deref()?;
    kinds.catalog.resolve(kind)?;
    Some(kinds.kind_word(kind).into_owned())
}

/// The most folders an unsure row offers.
const MAX_OFFERS: usize = 2;

/// Each candidate as the folder the document would go to, with the year of
/// the document, and the name it would get there; two candidates of one
/// folder are one.
fn unsure_offers(
    candidates: &[Candidate],
    fields: &DocumentFields,
    kinds: &Kinds,
    name: &str,
    folders: &FolderList,
) -> Vec<(PathBuf, String)> {
    let extension = Path::new(name).extension().and_then(|ext| ext.to_str());
    let mut offers: Vec<(PathBuf, String)> = Vec::new();
    for candidate in candidates {
        let Some((folder, file_name)) =
            offer_of(candidate, fields, kinds, name, extension, folders)
        else {
            continue;
        };
        if offers.iter().all(|(known, _)| *known != folder) {
            offers.push((folder, file_name));
        }
    }
    offers.truncate(MAX_OFFERS);
    offers
}

fn offer_of(
    candidate: &Candidate,
    fields: &DocumentFields,
    kinds: &Kinds,
    name: &str,
    extension: Option<&str>,
    folders: &FolderList,
) -> Option<(PathBuf, String)> {
    let kept = || {
        Some(candidate.folder.clone())
            .filter(|folder| !folder.as_os_str().is_empty())
            .map(|folder| (folders.with_year(folder, fields), name.to_owned()))
    };
    let Some(pattern) = candidate.pattern.as_deref() else {
        return kept();
    };
    let complete = extension
        .and_then(|ext| render_complete_pattern(pattern, fields, kinds, ext))
        .and_then(|path| {
            let file_name = path.file_name()?.to_string_lossy().into_owned();
            Some((path.parent()?.to_path_buf(), file_name))
        })
        .filter(|(folder, _)| !folder.as_os_str().is_empty());
    complete
        .or_else(|| {
            render_pattern_folder(pattern, fields, kinds).map(|folder| (folder, name.to_owned()))
        })
        .or_else(kept)
}

struct ModelAsk<'a> {
    access: &'a ChatAccess,
    model: &'a dyn ChatModel,
    folders: &'a FolderList,
    kinds: &'a Kinds,
    target_root: &'a Path,
    /// The files of the source, in the order of the entries.
    files: &'a [PlanFile],
    /// The neighbours' candidates of each file, in the same order.
    candidates: &'a [Vec<Candidate>],
    /// What alike files share, in the same order; files with the same key
    /// get one question.
    group_keys: &'a [Option<String>],
}

/// What the model reads of a file: the start of its text.
const MODEL_TEXT_CHARS: usize = 2000;

/// Bump when the folder-choice prompt changes: the stored picks of an older
/// version are asked again.
/// Version 2: version 1 also held "no folder fits" for files the model never
/// answered.
/// Version 3: a group of alike files gets one pick.
pub(in crate::chat_work) const FOLDER_PICK_VERSION: i64 = 3;

/// The files that did not get a sure place go to the model once, as a batch.
/// A file it picks a folder for gets the model's folder first, then the one
/// the neighbours offered: filed when the neighbours offered it first, else
/// unsure and placed by the model alone. A model that fails changes nothing and stores
/// nothing: the files it was asked about stay among the ones that still need
/// it, and the failure is returned. Picks stored by an earlier trial count as
/// the model's answer and are not asked again.
/// Returns the files that needed the model but were not answered this time.
fn ask_model(
    ask: &ModelAsk<'_>,
    entries: &mut [TrialEntry],
    taken: &mut BTreeSet<PathBuf>,
    scope: &ModelScope,
) -> (BTreeSet<PathBuf>, Option<AiError>) {
    let needing_model: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| needs_model(&entry.outcome))
        .map(|(position, _)| position)
        .collect();
    if needing_model.is_empty() || ask.folders.entries.is_empty() {
        return (BTreeSet::new(), None);
    }
    let wanting = apply_stored_picks(ask, entries, taken, model_groups(ask, &needing_model));
    let (asked, not_asked): (Vec<Vec<usize>>, Vec<Vec<usize>>) = match scope {
        ModelScope::FirstPage => {
            let mut asked = wanting;
            let rest = asked.split_off(MODEL_FIRST_PAGE.min(asked.len()));
            (asked, rest)
        }
        ModelScope::LastPage => {
            let mut rest = wanting;
            let asked = rest.split_off(rest.len().saturating_sub(MODEL_FIRST_PAGE));
            (asked, rest)
        }
        ModelScope::Only(files) => wanting.into_iter().partition(|group| {
            group.iter().any(|position| {
                entries
                    .get(*position)
                    .is_some_and(|entry| files.contains(&entry.from))
            })
        }),
    };
    // A named set is the rest itself: what it leaves out was settled before.
    let left: BTreeSet<PathBuf> = if matches!(scope, ModelScope::Only(_)) {
        BTreeSet::new()
    } else {
        not_asked
            .into_iter()
            .flatten()
            .filter_map(|position| entries.get(position).map(|entry| entry.from.clone()))
            .collect()
    };
    if asked.is_empty() {
        return (left, None);
    }
    let asked_files: Vec<PathBuf> = asked
        .iter()
        .flatten()
        .filter_map(|position| entries.get(*position).map(|entry| entry.from.clone()))
        .collect();
    let documents: Vec<String> = asked
        .iter()
        .filter_map(|group| group.first())
        .filter_map(|position| ask.files.get(*position))
        .map(document_text)
        .collect();
    let lane = match scope {
        ModelScope::Only(_) | ModelScope::LastPage => FolderChoiceLane::Background,
        ModelScope::FirstPage => FolderChoiceLane::Waiting,
    };
    let picks = match ask
        .model
        .choose_folders(&ask.folders.entries, &documents, lane)
    {
        Ok(picks) => picks,
        Err(failure) => {
            let mut left_over = left;
            left_over.extend(asked_files);
            return (left_over, Some(failure));
        }
    };
    let mut to_remember: Vec<(ContentHash, Option<String>)> = Vec::new();
    for (group, pick) in asked.into_iter().zip(picks) {
        let folder_name = pick
            .and_then(|number| ask.folders.entries.get(number))
            .map(|(folder, _)| folder.clone());
        for position in group {
            if let Some(entry) = entries.get(position) {
                to_remember.push((entry.hash, folder_name.clone()));
            }
            if let Some(folder) = &folder_name {
                offer_model_folder(ask, entries, taken, position, folder_path(folder));
            }
        }
    }
    // The picks are a cache: a trial without them is only slower next time.
    let _ = ask.access.with_index(|index| {
        index
            .store_folder_picks(ask.target_root, &to_remember, FOLDER_PICK_VERSION)
            .map_err(|err| err.to_string())
    });
    (left, None)
}

/// The files that need the model, alike ones together: the first of a group
/// stands for it. A file without a key is a group of its own.
fn model_groups(ask: &ModelAsk<'_>, needing_model: &[usize]) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut by_key: HashMap<&str, usize> = HashMap::new();
    for position in needing_model {
        let key = ask.group_keys.get(*position).and_then(Option::as_deref);
        let existing = key
            .and_then(|shared| by_key.get(shared))
            .and_then(|at| groups.get_mut(*at));
        if let Some(group) = existing {
            group.push(*position);
            continue;
        }
        if let Some(shared) = key {
            by_key.insert(shared, groups.len());
        }
        groups.push(vec![*position]);
    }
    groups
}

/// Offers the stored folder of a group's first file to the whole group, and
/// returns the groups that still need the model. A stored "none fits" settles
/// the group; a stored folder that is no longer in the list is asked again.
fn apply_stored_picks(
    ask: &ModelAsk<'_>,
    entries: &mut [TrialEntry],
    taken: &mut BTreeSet<PathBuf>,
    wanting: Vec<Vec<usize>>,
) -> Vec<Vec<usize>> {
    let hashes: Vec<ContentHash> = wanting
        .iter()
        .filter_map(|group| group.first())
        .filter_map(|position| entries.get(*position))
        .map(|entry| entry.hash)
        .collect();
    let stored = ask
        .access
        .with_index(|index| {
            index
                .folder_picks(ask.target_root, &hashes, FOLDER_PICK_VERSION)
                .map_err(|err| err.to_string())
        })
        .unwrap_or_default();
    let mut remaining = Vec::new();
    for group in wanting {
        let Some(entry) = group.first().and_then(|first| entries.get(*first)) else {
            continue;
        };
        match stored.get(&entry.hash) {
            Some(None) => {}
            Some(Some(folder)) if ask.folders.entries.iter().any(|(known, _)| known == folder) => {
                let stored_folder = folder.clone();
                for position in group {
                    offer_model_folder(ask, entries, taken, position, folder_path(&stored_folder));
                }
            }
            Some(Some(_)) | None => remaining.push(group),
        }
    }
    remaining
}

/// The folder lists spell folders with `/`; a plain root would otherwise get mixed
/// separators on Windows.
fn folder_path(folder: &str) -> PathBuf {
    folder.split('/').collect()
}

/// Makes the file unsure with the model's `folder` first, then what the
/// neighbours offered. When the model's folder is the one the neighbours
/// offered first, two independent signals agree: the file is filed there.
fn offer_model_folder(
    ask: &ModelAsk<'_>,
    entries: &mut [TrialEntry],
    taken: &mut BTreeSet<PathBuf>,
    position: usize,
    folder: PathBuf,
) {
    let (Some(entry), Some(candidates)) = (entries.get_mut(position), ask.candidates.get(position))
    else {
        return;
    };
    let extension = Path::new(&entry.name)
        .extension()
        .and_then(|ext| ext.to_str());
    let from_model = candidates
        .iter()
        .find(|candidate| candidate.folder == folder)
        .and_then(|candidate| {
            offer_of(
                candidate,
                &entry.fields,
                ask.kinds,
                &entry.name,
                extension,
                ask.folders,
            )
        })
        .unwrap_or_else(|| {
            (
                ask.folders.with_year(folder, &entry.fields),
                entry.name.clone(),
            )
        });
    // The neighbours were sure of the folder and only missed a name pattern:
    // the model may go deeper into it, never elsewhere.
    if let (TrialOutcome::Stays(StayReason::NoPattern), Some(sure_folder)) =
        (&entry.outcome, entry.neighbour.as_ref())
    {
        let (to, placed_by) = if from_model.0.starts_with(sure_folder) {
            (
                from_model.0.join(&from_model.1),
                PlacedBy::ModelAndNeighbours,
            )
        } else {
            (sure_folder.join(&entry.name), PlacedBy::Neighbours)
        };
        if taken.insert(to.clone()) {
            entry.outcome = TrialOutcome::Filed { to };
            entry.placed_by = Some(placed_by);
            return;
        }
    }
    let neighbours_first = match &entry.outcome {
        TrialOutcome::Unsure { choices, .. } => choices.first(),
        _ => None,
    };
    if neighbours_first == Some(&from_model.0) {
        let to = from_model.0.join(&from_model.1);
        if taken.insert(to.clone()) {
            entry.outcome = TrialOutcome::Filed { to };
            entry.placed_by = Some(PlacedBy::ModelAndNeighbours);
            return;
        }
    }
    let mut offers = vec![from_model];
    let mut kind_label = None;
    if let TrialOutcome::Unsure {
        choices,
        file_names,
        kind_label: known_label,
    } = &entry.outcome
    {
        offers.extend(choices.iter().cloned().zip(file_names.iter().cloned()));
        kind_label.clone_from(known_label);
    } else {
        kind_label =
            known_kind_label(ask.kinds, &entry.fields).or_else(|| entry.fields.sender.clone());
    }
    let mut distinct: Vec<(PathBuf, String)> = Vec::new();
    for offer in offers {
        if distinct.iter().all(|(known, _)| *known != offer.0) {
            distinct.push(offer);
        }
    }
    distinct.truncate(MAX_OFFERS);
    let (choices, file_names) = distinct.into_iter().unzip();
    entry.outcome = TrialOutcome::Unsure {
        choices,
        file_names,
        kind_label,
    };
    entry.placed_by = Some(PlacedBy::Model);
}

/// Sure places stay as they are; a taken name or a missing field is no
/// question of which folder.
fn needs_model(outcome: &TrialOutcome) -> bool {
    match outcome {
        TrialOutcome::Filed { .. } | TrialOutcome::AlreadyThere { .. } => false,
        TrialOutcome::Stays(reason) => {
            matches!(reason, StayReason::NoNeighbours | StayReason::NoPattern)
        }
        TrialOutcome::Unsure { .. } => true,
    }
}

fn document_text(file: &PlanFile) -> String {
    let fields = &file.fields;
    let text: String = file
        .text
        .as_deref()
        .unwrap_or_default()
        .chars()
        .take(MODEL_TEXT_CHARS)
        .collect();
    format!(
        "Absender: {}\nArt: {}\nDatum: {}\n\n{text}",
        fields.sender.as_deref().unwrap_or_default(),
        fields.kind.as_deref().unwrap_or_default(),
        fields.date.map(|date| date.to_string()).unwrap_or_default(),
    )
}

fn first_segment(to: &Path) -> String {
    to.iter()
        .next()
        .map(|segment| segment.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn in_sample_order(entries: Vec<TrialEntry>) -> Vec<TrialEntry> {
    let mut first_of_area: BTreeMap<String, usize> = BTreeMap::new();
    for (position, entry) in entries.iter().enumerate() {
        if let TrialOutcome::Filed { to } = &entry.outcome {
            first_of_area.entry(first_segment(to)).or_insert(position);
        }
    }
    let sampled: BTreeSet<usize> = first_of_area.into_values().collect();
    let rank = |position: usize, entry: &TrialEntry| match &entry.outcome {
        TrialOutcome::Filed { .. } if sampled.contains(&position) => 0,
        TrialOutcome::Unsure { .. } => 1,
        TrialOutcome::Filed { .. } => 2,
        TrialOutcome::AlreadyThere { .. } => 3,
        TrialOutcome::Stays(_) => 4,
    };
    // Sampled files go in the order of their area, the rest in path order.
    let mut ranked: Vec<(u8, usize, TrialEntry)> = entries
        .into_iter()
        .enumerate()
        .map(|(position, entry)| (rank(position, &entry), position, entry))
        .collect();
    let area_of = |entry: &TrialEntry| match &entry.outcome {
        TrialOutcome::Filed { to } => first_segment(to),
        _ => String::new(),
    };
    ranked.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| {
                if left.0 == 0 {
                    area_of(&left.2).cmp(&area_of(&right.2))
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .then(left.1.cmp(&right.1))
    });
    ranked.into_iter().map(|(_, _, entry)| entry).collect()
}

#[cfg(test)]
mod folder_path_tests {
    use super::*;

    #[test]
    fn a_folder_of_the_list_becomes_its_components() {
        let parts: Vec<_> = folder_path("A/B").iter().map(ToOwned::to_owned).collect();
        assert_eq!(parts, ["A", "B"]);
    }
}
