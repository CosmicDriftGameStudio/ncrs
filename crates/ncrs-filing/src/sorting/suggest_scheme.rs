//! The scheme of a target: the folders of the documents it holds, and the
//! documents of its sources that no rule takes, become suggested rules.

use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::AtomicBool;

use ncrs_ai::{folder_depth, pattern_mismatches, AiError, ChatModel};
use ncrs_index::{DocumentFields, TargetId};
use ncrs_intake::{learn_archive, ArchiveDocument};

use super::access::SortingAccess;
use super::suggest::{ModelNote, SuggestError, Suggested, Suggestions};
use super::suggest_material::{
    candidate, load, slashed, try_word, word_condition, Doc, Home, Material, WordIndex,
    MIN_GROUP_DOCUMENTS, MIN_SHARE_IN_GROUP_PERCENT,
};
use super::suggest_store::is_new;
use crate::reading_support::is_stopped;

/// The files of a group the model sees.
const MODEL_SAMPLE: usize = 12;
/// A pattern the model wrote may miss this share of the group's paths.
const MODEL_MISS_PERCENT: usize = 20;
/// Documents without a folder of their own are grouped by kind with this pattern.
const KIND_PATTERN: &str = "{kind}/{year}/";

struct Group {
    name: String,
    docs: Vec<usize>,
    /// What the folders already say, without a model.
    structural: Option<String>,
    /// Whether the documents lie in the target, so the model can read their
    /// folders.
    in_target: bool,
}

/// Rules for the documents of `target` and its sources that no rule takes.
/// `again` asks for another proposal than the ones before: the n-th word that
/// tells the group apart is taken. The pattern `wish` of the user comes first
/// wherever every document of a group gets a place from it. Models run here,
/// never on the UI thread; `stop` ends the work between two groups.
pub fn suggest_scheme(
    access: &SortingAccess,
    model: Option<&dyn ChatModel>,
    target: &TargetId,
    wish: Option<&str>,
    again: usize,
    stop: Option<&AtomicBool>,
) -> Result<Suggestions, SuggestError> {
    let Some(material) = load(access, target, stop)? else {
        return Err(SuggestError::Stopped);
    };
    let covered: Vec<bool> = if material.rules.is_empty() {
        vec![false; material.docs.len()]
    } else {
        material
            .docs
            .iter()
            .map(|doc| material.is_covered(doc))
            .collect()
    };
    let words = WordIndex::of(&material.docs);
    let groups = groups_of(&material, &covered);
    let mut owner: Vec<Option<usize>> = vec![None; material.docs.len()];
    for (at, group) in groups.iter().enumerate() {
        for &doc in &group.docs {
            if let Some(slot) = owner.get_mut(doc) {
                *slot = Some(at);
            }
        }
    }

    let mut note = ModelNote::NotAvailable;
    let mut model_state = model;
    let mut items = Vec::new();
    let mut filed: HashSet<usize> = HashSet::new();
    for (at, group) in groups.iter().enumerate() {
        if is_stopped(stop) {
            return Err(SuggestError::Stopped);
        }
        let mut patterns: Vec<String> = wish
            .filter(|pattern| !leads_into_another_group(pattern, group, &groups))
            .map(str::to_owned)
            .into_iter()
            .collect();
        if let (Some(asked), true) = (model_state, group.in_target) {
            match model_pattern(asked, &material, group) {
                Ok(Some(pattern)) => {
                    note = ModelNote::Used;
                    patterns.push(pattern);
                }
                Ok(None) => note = ModelNote::Used,
                Err(error) => {
                    note = ModelNote::Failed(error.to_string());
                    model_state = None;
                }
            }
        }
        patterns.extend(group.structural.clone());
        let others: Vec<usize> = (0..material.docs.len())
            .filter(|doc| owner.get(*doc) != Some(&Some(at)))
            .collect();
        let ranked = words.words_of_group(&group.docs, &others);
        for pattern in &patterns {
            let accepted = accepted_for(&material, &words, &groups, &owner, at, &ranked, pattern);
            if let Some((suggested, hits)) =
                first_new(access, target, accepted, again).map_err(SuggestError::Index)?
            {
                filed.extend(hits);
                items.push(suggested);
                break;
            }
        }
    }

    let covered_before = covered.iter().filter(|&&is_covered| is_covered).count();
    Ok(Suggestions {
        files_read: material.docs.len(),
        sources_read: material.sources,
        rules_existing: material.rules.len(),
        fit_suggested: filed.len(),
        fit_none: material
            .docs
            .len()
            .saturating_sub(covered_before)
            .saturating_sub(filed.len()),
        items,
        model: note,
    })
}

/// A pattern that starts with the folder of another group would mix the two.
fn leads_into_another_group(pattern: &str, group: &Group, groups: &[Group]) -> bool {
    let first = pattern.trim_start_matches('/').split('/').next();
    groups
        .iter()
        .filter(|other| other.in_target && other.name != group.name)
        .any(|other| first == Some(other.name.as_str()))
}

/// The documents of the target by their first folder, and the documents of
/// the sources by their kind; only what no rule takes yet.
fn groups_of(material: &Material, covered: &[bool]) -> Vec<Group> {
    let mut folders: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut kinds: BTreeMap<String, (String, Vec<usize>)> = BTreeMap::new();
    for (at, doc) in material.docs.iter().enumerate() {
        if covered.get(at).copied().unwrap_or(false) {
            continue;
        }
        match doc.home {
            Home::Target => {
                let mut parts = doc.relative.components();
                let first = parts.next();
                if parts.next().is_none() {
                    continue;
                }
                if let Some(folder) = first.and_then(|part| part.as_os_str().to_str()) {
                    folders.entry(folder.to_owned()).or_default().push(at);
                }
            }
            Home::Source => {
                if let Some(kind) = doc.fields.kind.as_deref().filter(|kind| !kind.is_empty()) {
                    kinds
                        .entry(kind.to_lowercase())
                        .or_insert_with(|| (kind.to_owned(), Vec::new()))
                        .1
                        .push(at);
                }
            }
        }
    }
    let from_folders = folders.into_iter().map(|(name, docs)| Group {
        structural: dominant_pattern(&material.docs, &docs),
        name,
        docs,
        in_target: true,
    });
    let from_kinds = kinds.into_values().map(|(name, docs)| Group {
        name,
        docs,
        structural: Some(KIND_PATTERN.to_owned()),
        in_target: false,
    });
    from_folders
        .chain(from_kinds)
        .filter(|group| group.docs.len() >= MIN_GROUP_DOCUMENTS)
        .collect()
}

/// The pattern most of the documents of the folder follow.
fn dominant_pattern(docs: &[Doc], group: &[usize]) -> Option<String> {
    let archive: Vec<ArchiveDocument> = group
        .iter()
        .filter_map(|&at| docs.get(at))
        .map(|doc| ArchiveDocument {
            relative_path: doc.relative.clone(),
            date: doc.fields.date,
        })
        .collect();
    learn_archive(&archive)
        .into_iter()
        .filter(|learned| learned.pattern.is_some())
        .max_by_key(|learned| learned.documents)
        .and_then(|learned| learned.pattern)
}

/// One model call: the levels that wrote the paths of the folder. `None` when
/// the pattern misses too many of the folder's paths.
fn model_pattern(
    model: &dyn ChatModel,
    material: &Material,
    group: &Group,
) -> Result<Option<String>, AiError> {
    let all: Vec<(String, DocumentFields)> = group
        .docs
        .iter()
        .filter_map(|&at| material.docs.get(at))
        .map(|doc| (slashed(&doc.relative), doc.fields.clone()))
        .collect();
    let depth = folder_depth(all.iter().map(|(path, _)| path.as_str()));
    let shown: Vec<(String, DocumentFields)> = all.iter().take(MODEL_SAMPLE).cloned().collect();
    let pattern = model
        .recognize_levels(&shown, &material.kinds, depth)?
        .pattern();
    let misses = pattern_mismatches(&pattern, &all, &material.kinds).len();
    Ok((misses * 100 <= all.len() * MODEL_MISS_PERCENT).then_some(pattern))
}

/// The words of the group that make a rule which files the group completely
/// and takes no other group in for the most part, best first.
fn accepted_for(
    material: &Material,
    words: &WordIndex,
    groups: &[Group],
    owner: &[Option<usize>],
    at: usize,
    ranked: &[String],
    pattern: &str,
) -> Vec<(Suggested, Vec<usize>)> {
    let Some(group) = groups.get(at) else {
        return Vec::new();
    };
    let mut accepted = Vec::new();
    for word in ranked {
        let rule = candidate(
            &material.target.id,
            &group.name,
            word_condition(word),
            pattern,
        );
        let tried = try_word(material, words, word, &rule, &group.docs);
        let own = tried
            .filed
            .iter()
            .filter(|doc| group.docs.contains(doc))
            .count();
        let mut taken_of_others: BTreeMap<usize, usize> = BTreeMap::new();
        for doc in &tried.filed {
            if let Some(Some(other)) = owner.get(*doc) {
                if *other != at {
                    *taken_of_others.entry(*other).or_default() += 1;
                }
            }
        }
        let takes_most_of_another = taken_of_others.iter().any(|(other, count)| {
            groups
                .get(*other)
                .is_some_and(|group| count * 2 > group.docs.len())
        });
        let complete = own * 100 >= group.docs.len() * MIN_SHARE_IN_GROUP_PERCENT;
        let Some(example) = tried.example.as_ref() else {
            continue;
        };
        if tried.group_unrenderable || !complete || takes_most_of_another {
            continue;
        }
        let elsewhere = tried.filed.len() - own;
        accepted.push((
            Suggested {
                name: group.name.clone(),
                spec: ncrs_index::RuleSpec {
                    confirmed: false,
                    ..rule.spec
                },
                example: Some(slashed(&example.relative)),
                files: tried.filed.len(),
                group_documents: u32::try_from(group.docs.len()).unwrap_or(u32::MAX),
                elsewhere: u32::try_from(elsewhere).unwrap_or(u32::MAX),
            },
            tried.filed,
        ));
    }
    accepted
}

/// The `again`-th of the accepted ones, or the next after it, that the user
/// did not turn down before.
pub fn first_new(
    access: &SortingAccess,
    target: &TargetId,
    accepted: Vec<(Suggested, Vec<usize>)>,
    again: usize,
) -> Result<Option<(Suggested, Vec<usize>)>, String> {
    if accepted.is_empty() {
        return Ok(None);
    }
    let count = accepted.len();
    let mut slots: Vec<Option<(Suggested, Vec<usize>)>> = accepted.into_iter().map(Some).collect();
    for step in 0..count {
        let Some(slot) = slots.get_mut((again + step) % count) else {
            continue;
        };
        let Some((suggested, hits)) = slot.take() else {
            continue;
        };
        let fresh = access.with_index(|index| is_new(index, target, &suggested))?;
        if fresh {
            return Ok(Some((suggested, hits)));
        }
    }
    Ok(None)
}
