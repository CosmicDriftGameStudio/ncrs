//! What the suggestions read: the documents the index knows below a target and
//! below its sources, the words that tell groups of them apart, and the test of
//! a candidate rule by deciding with it, exactly as a run would.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use ncrs_index::{
    Condition, ConditionKind, ContentHash, DocumentFields, FieldOrigin, FieldOrigins, Index, Kinds,
    Rule, RuleConditions, RuleId, RuleOrigin, RuleSpec, SenderAliases, TargetId,
};
use ncrs_intake::{
    decide, Decision, LocationTransfer, OnNoText, OnSeveralTargets, Place, SortSource, SortTarget,
    Subject, TargetRules, TextState, Words,
};

use super::access::{text, SortingAccess};
use super::plan_run::source_root;
use super::suggest::SuggestError;
use crate::reading_support::is_stopped;

/// A word must be this long to tell documents apart.
const MIN_WORD_CHARS: usize = 5;
/// The share of its group a condition has to take in.
pub(super) const MIN_SHARE_IN_GROUP_PERCENT: usize = 80;
/// A group with fewer documents proves nothing.
pub(super) const MIN_GROUP_DOCUMENTS: usize = 3;
/// How many words are tried per group.
const WORDS_TRIED: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Home {
    Target,
    Source,
}

pub(super) struct Doc {
    pub path: PathBuf,
    /// Below the root of the target or source it lies in.
    pub relative: PathBuf,
    pub hash: ContentHash,
    pub fields: DocumentFields,
    pub origins: FieldOrigins,
    pub text: String,
    pub home: Home,
}

pub(super) struct Material {
    pub docs: Vec<Doc>,
    /// The sources of the target that were read.
    pub sources: usize,
    pub kinds: Kinds,
    pub aliases: SenderAliases,
    pub target: SortTarget,
    /// The enabled, confirmed rules the target has.
    pub rules: Vec<Rule>,
}

/// The documents with text below the target and below each source that files
/// into it; a content known in both counts once, as the target's. `None` when
/// stopped. Reads the index only.
pub(super) fn load(
    access: &SortingAccess,
    target: &TargetId,
    stop: Option<&AtomicBool>,
) -> Result<Option<Material>, SuggestError> {
    let sort_target = access.target(target).ok_or(SuggestError::NoTarget)?;
    let source_roots: Vec<PathBuf> = access
        .sources
        .iter()
        .filter(|entry| entry.source.targets.contains(target))
        .filter_map(|entry| source_root(&entry.source).ok())
        .collect();
    let read = access
        .with_index(|index| {
            let mut docs = Vec::new();
            let mut seen = HashSet::new();
            let places = std::iter::once((sort_target.path.as_path(), Home::Target)).chain(
                source_roots
                    .iter()
                    .map(|root| (root.as_path(), Home::Source)),
            );
            for (root, home) in places {
                if !read_under(index, root, home, &mut seen, &mut docs, stop)? {
                    return Ok(None);
                }
            }
            let rules = index
                .rules_of(target)
                .map_err(text)?
                .into_iter()
                .filter(|rule| rule.spec.enabled && rule.spec.confirmed)
                .collect();
            let aliases = index.confirmed_sender_aliases().map_err(text)?;
            Ok(Some((docs, rules, aliases, access.kinds(index))))
        })
        .map_err(SuggestError::Index)?;
    Ok(read.map(|(docs, rules, aliases, kinds)| Material {
        docs,
        sources: source_roots.len(),
        kinds,
        aliases,
        target: sort_target,
        rules,
    }))
}

/// `false` when stopped.
fn read_under(
    index: &Index,
    root: &Path,
    home: Home,
    seen: &mut HashSet<ContentHash>,
    docs: &mut Vec<Doc>,
    stop: Option<&AtomicBool>,
) -> Result<bool, String> {
    for (id, path) in index.documents_under(root).map_err(text)? {
        if is_stopped(stop) {
            return Ok(false);
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        if index.is_excluded(&path).map_err(text)? {
            continue;
        }
        let Some(document) = index.document(id).map_err(text)? else {
            continue;
        };
        let Some(content) = index.document_text(id).map_err(text)? else {
            continue;
        };
        if content.trim().is_empty() || !seen.insert(document.hash) {
            continue;
        }
        docs.push(Doc {
            relative: relative.to_path_buf(),
            path,
            hash: document.hash,
            fields: document.fields,
            origins: index.field_origins(id).map_err(text)?,
            text: content,
            home,
        });
    }
    Ok(true)
}

impl Material {
    fn source(&self) -> SortSource {
        SortSource {
            path: self.target.path.clone(),
            include: String::new(),
            exclude: String::new(),
            targets: vec![self.target.id.clone()],
            watch: false,
            transfer: LocationTransfer::Move,
            no_text: OnNoText::Ask,
            several: OnSeveralTargets::Ask,
            own_volume: None,
        }
    }

    /// What the target would do with the document under these rules.
    pub fn decide(&self, doc: &Doc, rules: &[Rule]) -> Decision {
        let manual = doc.origins.manual_fields();
        let subject = Subject {
            relative: &doc.relative,
            extension: doc.path.extension().and_then(std::ffi::OsStr::to_str),
            text: TextState::Text(&doc.text),
            fields: &doc.fields,
            origins: &doc.origins,
            manual: &manual,
            known_at: &[],
        };
        let words = Words {
            kinds: &self.kinds,
            sender_aliases: &self.aliases,
        };
        let entry = [TargetRules {
            target: &self.target,
            rules,
        }];
        decide(&subject, &self.source(), &entry, &words)
    }

    /// Whether the saved rules of the target take the document in.
    pub fn is_covered(&self, doc: &Doc) -> bool {
        !matches!(
            self.decide(doc, &self.rules),
            Decision::NoRule | Decision::NoText | Decision::NotRead
        )
    }
}

/// A rule that only exists to be tried.
pub(super) fn candidate(
    target: &TargetId,
    name: &str,
    conditions: RuleConditions,
    pattern: &str,
) -> Rule {
    Rule {
        id: RuleId::unused_in(&[]),
        position: 1,
        origin: RuleOrigin::Suggestion,
        target: target.clone(),
        spec: RuleSpec {
            name: Some(name.to_owned()),
            conditions,
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: None,
            tags: BTreeSet::new(),
            filing_pattern: pattern.to_owned(),
            confirmed: true,
            enabled: true,
        },
    }
}

pub(super) fn word_condition(word: &str) -> RuleConditions {
    RuleConditions::all_of(vec![Condition::new(ConditionKind::Word, word)])
}

/// The words of the text that tell documents apart, in lower case.
fn words_of(content: &str) -> HashSet<String> {
    content
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| word.chars().count() >= MIN_WORD_CHARS)
        .filter(|word| word.chars().any(char::is_alphabetic))
        .map(str::to_lowercase)
        .collect()
}

/// Per document the words it has; computed once for all groups.
pub(super) struct WordIndex {
    per_doc: Vec<HashSet<String>>,
}

impl WordIndex {
    pub fn of(docs: &[Doc]) -> Self {
        Self {
            per_doc: docs.iter().map(|doc| words_of(&doc.text)).collect(),
        }
    }

    pub fn has(&self, doc: usize, word: &str) -> bool {
        self.per_doc
            .get(doc)
            .is_some_and(|words| words.contains(word))
    }

    /// The words that at least `MIN_SHARE_IN_GROUP_PERCENT` of `group` have,
    /// the ones the rest of the documents have least first.
    pub fn words_of_group(&self, group: &[usize], others: &[usize]) -> Vec<String> {
        let mut inside: HashMap<&str, usize> = HashMap::new();
        for &doc in group {
            for word in self.per_doc.get(doc).into_iter().flatten() {
                *inside.entry(word.as_str()).or_default() += 1;
            }
        }
        let mut found: Vec<(&str, usize, usize)> = inside
            .into_iter()
            .filter(|(_, count)| count * 100 >= group.len() * MIN_SHARE_IN_GROUP_PERCENT)
            .map(|(word, count)| {
                let elsewhere = others.iter().filter(|&&doc| self.has(doc, word)).count();
                (word, count, elsewhere)
            })
            .collect();
        found.sort_by(|left, right| {
            let share = |entry: &(&str, usize, usize)| {
                // Compared as fractions of the same denominator.
                entry.2.saturating_mul(1000) / others.len().max(1)
            };
            share(left)
                .cmp(&share(right))
                .then(right.1.cmp(&left.1))
                .then(right.0.chars().count().cmp(&left.0.chars().count()))
                .then(left.0.cmp(right.0))
        });
        found
            .into_iter()
            .take(WORDS_TRIED)
            .map(|(word, _, _)| word.to_owned())
            .collect()
    }
}

/// What a candidate rule does to all documents.
pub(super) struct Tried {
    /// Documents the rule files, by index into the material.
    pub filed: Vec<usize>,
    /// The first place a document of the group got.
    pub example: Option<Place>,
    /// A document of the group the rule takes but cannot name.
    pub group_unrenderable: bool,
}

/// Decides with the candidate over the documents that have `word`, as a run
/// would; `group` is the group the rule is for.
pub(super) fn try_word(
    material: &Material,
    words: &WordIndex,
    word: &str,
    candidate_rule: &Rule,
    group: &[usize],
) -> Tried {
    let rules = std::slice::from_ref(candidate_rule);
    let mut tried = Tried {
        filed: Vec::new(),
        example: None,
        group_unrenderable: false,
    };
    let in_group: HashSet<usize> = group.iter().copied().collect();
    for (at, doc) in material.docs.iter().enumerate() {
        if !words.has(at, word) {
            continue;
        }
        match material.decide(doc, rules) {
            Decision::File(place) => {
                if tried.example.is_none() && in_group.contains(&at) {
                    tried.example = Some(place);
                }
                tried.filed.push(at);
            }
            Decision::MissingValue { .. } | Decision::BadName { .. }
                if in_group.contains(&at) =>
            {
                tried.group_unrenderable = true;
            }
            _ => {}
        }
    }
    tried
}

/// The path as the user reads it, whatever the system.
pub(super) fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Whether a sender was not read by a model, so a condition may rest on it.
pub(super) fn sender_is_firm(doc: &Doc) -> bool {
    doc.fields.sender.is_some()
        && !matches!(
            doc.origins.of(ncrs_index::Field::Sender),
            FieldOrigin::Ai { .. }
        )
}
