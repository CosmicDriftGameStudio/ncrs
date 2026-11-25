//! One rule for one file no rule takes: the condition rests on what the file
//! shows and the pattern on what the target does with documents like it.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use ncrs_ai::{ChatModel, ContextDocument};
use ncrs_index::{Condition, ConditionKind, RuleConditions, TargetId};
use ncrs_intake::{canonical_path, Decision, SortSource};

use super::access::SortingAccess;
use super::suggest::{ModelNote, RuleForFile, SuggestError, Suggested};
use super::suggest_material::{
    candidate, load, sender_is_firm, slashed, word_condition, Doc, Material, WordIndex,
};
use super::suggest_scheme::first_new;
use crate::reading_support::is_stopped;

/// The documents of the target the model sees.
const CONTEXT_DOCUMENTS: usize = 12;

/// A rule for `path`, a file of `source`; the rule files into the first
/// target of the source that exists. The file has to be known to the index:
/// nothing is read here. `also` lists the other documents without a rule that
/// the rule would file.
pub fn suggest_rule_for(
    access: &SortingAccess,
    model: Option<&dyn ChatModel>,
    source: &SortSource,
    path: &Path,
    stop: Option<&AtomicBool>,
) -> Result<RuleForFile, SuggestError> {
    let target = source
        .targets
        .iter()
        .find(|id| access.target(id).is_some())
        .cloned()
        .ok_or(SuggestError::NoSource)?;
    let Some(material) = load(access, &target, stop)? else {
        return Err(SuggestError::Stopped);
    };
    let spelled = spelled_path(path);
    let at = material
        .docs
        .iter()
        .position(|doc| doc.path == spelled || doc.path == path)
        .ok_or(SuggestError::NotKnown)?;
    let doc = material.docs.get(at).ok_or(SuggestError::NotKnown)?;
    if doc.text.trim().is_empty() {
        return Err(SuggestError::NoText);
    }
    let words = WordIndex::of(&material.docs);
    let conditions = conditions_for(&material, &words, at, doc);

    let mut note = ModelNote::NotAvailable;
    let mut patterns = Vec::new();
    if let Some(asked) = model {
        match model_pattern(asked, &material, doc) {
            Ok(pattern) => {
                note = ModelNote::Used;
                patterns.extend(pattern);
            }
            Err(error) => note = ModelNote::Failed(error),
        }
    }
    patterns.extend(fallback_patterns(&material, doc));

    for pattern in &patterns {
        for condition in &conditions {
            if is_stopped(stop) {
                return Err(SuggestError::Stopped);
            }
            if let Some(found) = try_rule(access, &material, &target, doc, condition, pattern)? {
                return Ok(RuleForFile {
                    target,
                    rule: found.0,
                    fields: doc.fields.clone(),
                    also: found.1,
                    model: note,
                });
            }
        }
    }
    Err(SuggestError::NoPattern)
}

/// The path the way the index spells it: the folder resolved, the name kept.
fn spelled_path(path: &Path) -> std::path::PathBuf {
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => canonical_path(parent)
            .map(|folder| folder.join(name))
            .unwrap_or_else(|_| path.to_path_buf()),
        _ => path.to_path_buf(),
    }
}

/// The conditions to try, best first: the sender when no model read it, then
/// the words of the file the other documents have least.
fn conditions_for(
    material: &Material,
    words: &WordIndex,
    at: usize,
    doc: &Doc,
) -> Vec<RuleConditions> {
    let mut list = Vec::new();
    if let (true, Some(sender)) = (sender_is_firm(doc), doc.fields.sender.as_deref()) {
        list.push(RuleConditions::all_of(vec![Condition::new(
            ConditionKind::Sender,
            sender,
        )]));
    }
    let others: Vec<usize> = (0..material.docs.len())
        .filter(|&other| other != at)
        .collect();
    list.extend(
        words
            .words_of_group(&[at], &others)
            .iter()
            .map(|word| word_condition(word)),
    );
    list
}

/// The patterns that are tried when no model wrote one: the pattern of the
/// folder of the target that holds documents of the same sender or kind, then
/// what the fields give.
fn fallback_patterns(material: &Material, doc: &Doc) -> Vec<String> {
    let mut patterns = Vec::new();
    let same_sender =
        |other: &Doc| doc.fields.sender.is_some() && other.fields.sender == doc.fields.sender;
    let same_kind = |other: &Doc| doc.fields.kind.is_some() && other.fields.kind == doc.fields.kind;
    let sibling = material
        .docs
        .iter()
        .filter(|other| other.home == super::suggest_material::Home::Target)
        .find(|other| same_sender(other) || same_kind(other));
    if let Some(folder) = sibling.and_then(|other| other.relative.components().next()) {
        if let Some(name) = folder.as_os_str().to_str() {
            if doc.fields.date.is_some() {
                patterns.push(format!("{name}/{{year}}/"));
            }
            patterns.push(format!("{name}/"));
        }
    }
    if doc.fields.kind.is_some() {
        patterns.push("{kind}/{year}/".to_owned());
    }
    if doc.fields.sender.is_some() {
        patterns.push("{sender}/{year}/".to_owned());
    }
    patterns.push("{year}/".to_owned());
    patterns
}

/// One model call: the rule the file describes, of which only the pattern
/// is taken; the condition is always one the text proves.
fn model_pattern(
    model: &dyn ChatModel,
    material: &Material,
    doc: &Doc,
) -> Result<Option<String>, String> {
    let description = description_of(doc);
    let context: Vec<ContextDocument> = material
        .docs
        .iter()
        .filter(|other| other.home == super::suggest_material::Home::Target)
        .take(CONTEXT_DOCUMENTS)
        .map(|other| ContextDocument {
            fields: other.fields.clone(),
            folder: other
                .relative
                .parent()
                .map(slashed)
                .filter(|folder| !folder.is_empty()),
        })
        .collect();
    let spec = model
        .generate_rule(&description, &context, &material.kinds)
        .map_err(|error| error.to_string())?;
    Ok(Some(spec.filing_pattern).filter(|pattern| !pattern.trim().is_empty()))
}

fn description_of(doc: &Doc) -> String {
    let mut parts = vec!["File this document like the ones already filed.".to_owned()];
    if let Some(sender) = &doc.fields.sender {
        parts.push(format!("Sender: {sender}."));
    }
    if let Some(kind) = &doc.fields.kind {
        parts.push(format!("Kind: {kind}."));
    }
    parts.join(" ")
}

/// Tests the rule on every document it could take; it counts when it files
/// the document, names it completely and is not one the user turned down.
fn try_rule(
    access: &SortingAccess,
    material: &Material,
    target: &TargetId,
    doc: &Doc,
    conditions: &RuleConditions,
    pattern: &str,
) -> Result<Option<(Suggested, Vec<std::path::PathBuf>)>, SuggestError> {
    let name = doc
        .fields
        .sender
        .clone()
        .or_else(|| doc.fields.kind.clone())
        .or_else(|| {
            doc.path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    let rule = candidate(target, &name, conditions.clone(), pattern);
    let rules = std::slice::from_ref(&rule);
    let Decision::File(place) = material.decide(doc, rules) else {
        return Ok(None);
    };
    let mut also = Vec::new();
    let mut filed: usize = 1;
    for other in &material.docs {
        if other.hash == doc.hash {
            continue;
        }
        if matches!(material.decide(other, rules), Decision::File(_)) {
            filed += 1;
            if !material.is_covered(other) {
                also.push(other.path.clone());
            }
        }
    }
    let suggested = Suggested {
        name,
        spec: ncrs_index::RuleSpec {
            confirmed: false,
            ..rule.spec
        },
        example: Some(slashed(&place.relative)),
        files: filed,
        group_documents: 1,
        elsewhere: u32::try_from(filed - 1 - also.len()).unwrap_or(0),
    };
    Ok(first_new(access, target, vec![(suggested, Vec::new())], 0)
        .map_err(SuggestError::Index)?
        .map(|(not_dismissed, _)| (not_dismissed, also)))
}
