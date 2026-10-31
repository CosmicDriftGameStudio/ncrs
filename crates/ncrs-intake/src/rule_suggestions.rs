//! Rule candidates read out of how the user has already filed documents: a
//! sender whose documents are mostly in one folder, or a phrase that most
//! documents of a folder contain and hardly any document elsewhere does. Pure
//! and deterministic; nothing here stores anything.
//!
//! The text of a document is data. A phrase only ever becomes a literal word
//! condition, never a pattern.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use ncrs_index::{
    Condition, ConditionKind, DocumentFields, RuleConditions, RuleEvidence, RuleSpec,
    SubtreePattern,
};

use crate::archive_profile::{subtree_per_document, ArchiveDocument};
use crate::conditions::{normalise_words, words_hold};

/// A phrase must be in at least this share of its folder's documents, a
/// sender's documents must be in one folder with at least this share, and kind
/// and tags are taken over when this share of the documents agree.
pub const MIN_SHARE_IN_FOLDER_PERCENT: u32 = 80;
/// ... and in at most this share of the documents outside the folder.
pub const MAX_SHARE_ELSEWHERE_PERCENT: u32 = 5;
/// A folder with fewer documents proves nothing.
pub const MIN_FOLDER_DOCUMENTS: u32 = 3;
/// Shorter phrases are too common to say anything.
pub const MIN_PHRASE_CHARS: usize = 4;
pub const MAX_PHRASE_WORDS: usize = 4;
/// The longest, best-evidenced phrases of a folder are kept.
pub const MAX_CANDIDATES_PER_FOLDER: usize = 10;

/// Words that carry no meaning of their own, so a phrase of only these is no
/// candidate.
const FILLER_WORDS: [&str; 34] = [
    "the", "and", "for", "with", "from", "that", "this", "are", "was", "not", "you", "your", "our",
    "page", "of", "to", "is", "on", "at", "by", "or", "be", "as", "it", "we", "der", "die", "das",
    "und", "für", "mit", "von", "ist", "ein",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveText {
    pub document: ArchiveDocument,
    pub text: String,
    /// What the index knows about the document: kind and tags the
    /// suggestions take over.
    pub fields: DocumentFields,
    /// The canonical sender, stored or detected; decided by the caller.
    pub sender: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleCandidate {
    /// The folder the evidence is about.
    pub subtree: String,
    /// What the condition looks for: the lowercase phrase of a word
    /// condition, the canonical name of a sender condition.
    pub phrase: String,
    /// Unconfirmed, with one literal word or sender condition.
    pub spec: RuleSpec,
    pub evidence: RuleEvidence,
}

/// Candidates for every folder (subtree) that has a pattern and enough
/// documents, sorted by folder and phrase. A sender most of whose documents
/// are in one folder yields a sender candidate for that folder; the other
/// folders yield phrase candidates. The kind is what most documents of the
/// folder carry, else the last constant folder of the subtree; a document with
/// the condition's phrase in its text is matched the way a rule matches it
/// (case-insensitive, between word boundaries).
pub fn suggest_rules(archive: &[ArchiveText], profile: &[SubtreePattern]) -> Vec<RuleCandidate> {
    let documents: Vec<ArchiveDocument> =
        archive.iter().map(|entry| entry.document.clone()).collect();
    let subtrees = subtree_per_document(&documents);
    let lowered: Vec<String> = archive
        .iter()
        .map(|entry| entry.text.to_lowercase())
        .collect();
    let normalised: Vec<String> = archive
        .iter()
        .map(|entry| normalise_words(&entry.text))
        .collect();

    let mut members: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, assigned) in subtrees.iter().enumerate() {
        if let Some(subtree) = assigned {
            members.entry(subtree.as_str()).or_default().push(index);
        }
    }

    let mut candidates = sender_candidates(archive, &members, profile);
    let sender_folders: BTreeSet<String> = candidates
        .iter()
        .map(|candidate| candidate.subtree.clone())
        .collect();
    for entry in profile {
        let (Some(pattern), Some(folder)) = (
            entry.pattern.as_deref(),
            members.get(entry.subtree.as_str()),
        ) else {
            continue;
        };
        if sender_folders.contains(&entry.subtree) {
            continue;
        }
        let Some(kind) = last_folder_name(&entry.subtree) else {
            continue;
        };
        let in_folder = folder.len();
        if u32::try_from(in_folder).unwrap_or(u32::MAX) < MIN_FOLDER_DOCUMENTS {
            continue;
        }
        let elsewhere: Vec<usize> = (0..lowered.len())
            .filter(|index| !folder.contains(index))
            .collect();
        let shared = shared_fields(archive, folder);
        for found in folder_phrases(&lowered, &normalised, folder, &elsewhere) {
            candidates.push(RuleCandidate {
                subtree: entry.subtree.clone(),
                phrase: found.phrase.clone(),
                spec: spec_of(
                    Condition::new(ConditionKind::Word, found.phrase),
                    kind,
                    pattern,
                    &shared,
                ),
                evidence: RuleEvidence {
                    matching_in_folder: to_u32(found.matching_in_folder),
                    folder_documents: to_u32(in_folder),
                    matching_elsewhere: to_u32(found.matching_elsewhere),
                    subtree: Some(entry.subtree.clone()),
                },
            });
        }
    }
    candidates
        .sort_by(|left, right| (&left.subtree, &left.phrase).cmp(&(&right.subtree, &right.phrase)));
    candidates
}

fn last_folder_name(subtree: &str) -> Option<&str> {
    subtree.rsplit('/').next().filter(|name| !name.is_empty())
}

fn spec_of(
    condition: Condition,
    fallback_kind: &str,
    pattern: &str,
    shared: &SharedFields,
) -> RuleSpec {
    RuleSpec {
        name: None,
        conditions: RuleConditions::all_of(vec![condition]),
        extract_number: None,
        extract_date: None,
        extract_amount: None,
        kind: Some(
            shared
                .kind
                .clone()
                .unwrap_or_else(|| fallback_kind.to_owned()),
        ),
        tags: shared.tags.clone(),
        filing_pattern: pattern.to_owned(),
        confirmed: false,
        enabled: true,
    }
}

/// What at least `MIN_SHARE_IN_FOLDER_PERCENT` of some documents agree on.
struct SharedFields {
    kind: Option<String>,
    tags: BTreeSet<String>,
}

fn shared_fields(archive: &[ArchiveText], members: &[usize]) -> SharedFields {
    let total = members.len();
    let all_fields: Vec<&DocumentFields> = members
        .iter()
        .filter_map(|&index| archive.get(index))
        .map(|entry| &entry.fields)
        .collect();

    // Case-insensitive; the spelling shown is the smallest one of the winners.
    let mut kinds: BTreeMap<String, BTreeMap<&str, usize>> = BTreeMap::new();
    for kind in all_fields
        .iter()
        .filter_map(|fields| fields.kind.as_deref())
    {
        let spellings = kinds.entry(kind.to_lowercase()).or_default();
        *spellings.entry(kind).or_default() += 1;
    }
    let kind = kinds.values().find_map(|spellings| {
        let count: usize = spellings.values().sum();
        at_least_share(count, total, MIN_SHARE_IN_FOLDER_PERCENT)
            .then(|| {
                spellings
                    .keys()
                    .next()
                    .map(|spelling| (*spelling).to_owned())
            })
            .flatten()
    });

    let mut tag_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for tag in all_fields.iter().flat_map(|fields| &fields.tags) {
        *tag_counts.entry(tag.as_str()).or_default() += 1;
    }
    let tags = tag_counts
        .into_iter()
        .filter(|&(_tag, count)| at_least_share(count, total, MIN_SHARE_IN_FOLDER_PERCENT))
        .map(|(tag, _count)| tag.to_owned())
        .collect();

    SharedFields { kind, tags }
}

/// One candidate per sender whose documents lie mostly in one folder with a
/// pattern. The folder with the most of them wins, the smallest on a tie.
fn sender_candidates(
    archive: &[ArchiveText],
    members: &BTreeMap<&str, Vec<usize>>,
    profile: &[SubtreePattern],
) -> Vec<RuleCandidate> {
    let mut subtree_of: BTreeMap<usize, &str> = BTreeMap::new();
    for (&subtree, indexes) in members {
        for &index in indexes {
            subtree_of.insert(index, subtree);
        }
    }
    // Lowercase name -> the documents of that sender that lie in a subtree.
    let mut by_sender: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for &index in subtree_of.keys() {
        let Some(sender) = archive
            .get(index)
            .and_then(|entry| entry.sender.as_deref())
            .map(str::trim)
            .filter(|sender| !sender.is_empty())
        else {
            continue;
        };
        by_sender
            .entry(sender.to_lowercase())
            .or_default()
            .push(index);
    }

    let mut candidates = Vec::new();
    for sender_documents in by_sender.values() {
        let mut per_folder: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for &index in sender_documents {
            if let Some(&subtree) = subtree_of.get(&index) {
                per_folder.entry(subtree).or_default().push(index);
            }
        }
        let mut best: Option<(&str, &Vec<usize>)> = None;
        for (&subtree, indexes) in &per_folder {
            if best.is_none_or(|(_, kept)| indexes.len() > kept.len()) {
                best = Some((subtree, indexes));
            }
        }
        let Some((subtree, in_folder)) = best else {
            continue;
        };
        let total = sender_documents.len();
        if u32::try_from(in_folder.len()).unwrap_or(u32::MAX) < MIN_FOLDER_DOCUMENTS
            || !at_least_share(in_folder.len(), total, MIN_SHARE_IN_FOLDER_PERCENT)
        {
            continue;
        }
        let Some(pattern) = profile
            .iter()
            .find(|entry| entry.subtree == subtree)
            .and_then(|entry| entry.pattern.as_deref())
        else {
            continue;
        };
        let Some(kind) = last_folder_name(subtree) else {
            continue;
        };
        let Some(name) = sender_documents
            .iter()
            .filter_map(|&index| archive.get(index)?.sender.as_deref())
            .map(str::trim)
            .min()
        else {
            continue;
        };
        candidates.push(RuleCandidate {
            subtree: subtree.to_owned(),
            phrase: name.to_owned(),
            spec: spec_of(
                Condition::new(ConditionKind::Sender, name),
                kind,
                pattern,
                &shared_fields(archive, in_folder),
            ),
            evidence: RuleEvidence {
                matching_in_folder: to_u32(in_folder.len()),
                folder_documents: to_u32(members.get(subtree).map_or(0, Vec::len)),
                matching_elsewhere: to_u32(total.saturating_sub(in_folder.len())),
                subtree: Some(subtree.to_owned()),
            },
        });
    }
    candidates
}

fn to_u32(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

struct FoundPhrase {
    phrase: String,
    matching_in_folder: usize,
    matching_elsewhere: usize,
    /// Every document that has the phrase, to tell phrases apart that cover
    /// the same ones.
    covered: Vec<usize>,
}

/// Whether the evidence a suggestion was stored with meets the thresholds it
/// was made under. `documents` is the number of documents in the archive now,
/// to put `matching_elsewhere` in relation to the ones outside the folder; a
/// sender condition is measured against the documents of the sender instead.
pub fn evidence_meets_thresholds(
    conditions: &RuleConditions,
    evidence: &RuleEvidence,
    documents: usize,
) -> bool {
    let matching = evidence.matching_in_folder as usize;
    if conditions
        .single_condition()
        .is_some_and(|condition| condition.kind == ConditionKind::Sender)
    {
        return evidence.matching_in_folder >= MIN_FOLDER_DOCUMENTS
            && at_least_share(
                matching,
                matching.saturating_add(evidence.matching_elsewhere as usize),
                MIN_SHARE_IN_FOLDER_PERCENT,
            );
    }
    let folder = evidence.folder_documents as usize;
    evidence.folder_documents >= MIN_FOLDER_DOCUMENTS
        && at_least_share(matching, folder, MIN_SHARE_IN_FOLDER_PERCENT)
        && at_most_share(
            evidence.matching_elsewhere as usize,
            documents.saturating_sub(folder),
            MAX_SHARE_ELSEWHERE_PERCENT,
        )
}

fn at_least_share(count: usize, of: usize, percent: u32) -> bool {
    count.saturating_mul(100) >= of.saturating_mul(percent as usize)
}

fn at_most_share(count: usize, of: usize, percent: u32) -> bool {
    count.saturating_mul(100) <= of.saturating_mul(percent as usize)
}

fn folder_phrases(
    lowered: &[String],
    normalised: &[String],
    folder: &[usize],
    elsewhere: &[usize],
) -> Vec<FoundPhrase> {
    // Phrase -> the folder documents whose word sequences contain it.
    let mut seen: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for &index in folder {
        let Some(text) = lowered.get(index) else {
            continue;
        };
        for phrase in phrases_of(text) {
            seen.entry(phrase).or_default().push(index);
        }
    }
    seen.retain(|_phrase, indexes| {
        at_least_share(indexes.len(), folder.len(), MIN_SHARE_IN_FOLDER_PERCENT)
    });
    let shared = without_covered(seen.into_iter().collect(), |(phrase, indexes)| {
        (phrase.as_str(), indexes.as_slice())
    });

    let has = |phrase: &str, index: usize| {
        normalised
            .get(index)
            .is_some_and(|text| words_hold(text, phrase))
    };
    let mut found = Vec::new();
    for (phrase, _indexes) in shared {
        let matching_in_folder = folder.iter().filter(|&&index| has(&phrase, index)).count();
        if !at_least_share(
            matching_in_folder,
            folder.len(),
            MIN_SHARE_IN_FOLDER_PERCENT,
        ) {
            continue;
        }
        let mut matching_elsewhere = 0;
        let mut too_common = false;
        for &index in elsewhere {
            if has(&phrase, index) {
                matching_elsewhere += 1;
                if !at_most_share(
                    matching_elsewhere,
                    elsewhere.len(),
                    MAX_SHARE_ELSEWHERE_PERCENT,
                ) {
                    too_common = true;
                    break;
                }
            }
        }
        if too_common {
            continue;
        }
        let covered = folder
            .iter()
            .chain(elsewhere)
            .copied()
            .filter(|&index| has(&phrase, index))
            .collect();
        found.push(FoundPhrase {
            phrase,
            matching_in_folder,
            matching_elsewhere,
            covered,
        });
    }
    let mut kept = without_covered(found, |entry| {
        (entry.phrase.as_str(), entry.covered.as_slice())
    });
    kept.sort_by(|left, right| {
        (
            Reverse(left.phrase.chars().count()),
            Reverse(left.matching_in_folder),
            left.matching_elsewhere,
            &left.phrase,
        )
            .cmp(&(
                Reverse(right.phrase.chars().count()),
                Reverse(right.matching_in_folder),
                right.matching_elsewhere,
                &right.phrase,
            ))
    });
    kept.truncate(MAX_CANDIDATES_PER_FOLDER);
    kept
}

/// Drops a phrase that sits inside a longer one found in exactly the same
/// documents: the longer one says more about the same documents.
fn without_covered<T>(entries: Vec<T>, view: impl Fn(&T) -> (&str, &[usize])) -> Vec<T> {
    let mut by_documents: BTreeMap<Vec<usize>, Vec<String>> = BTreeMap::new();
    for entry in &entries {
        let (phrase, documents) = view(entry);
        by_documents
            .entry(documents.to_vec())
            .or_default()
            .push(phrase.to_owned());
    }
    entries
        .into_iter()
        .filter(|entry| {
            let (phrase, documents) = view(entry);
            !by_documents.get(documents).is_some_and(|same| {
                same.iter()
                    .any(|other| other.len() > phrase.len() && other.contains(phrase))
            })
        })
        .collect()
}

/// Word sequences of one to `MAX_PHRASE_WORDS` words, within a line, joined
/// by single spaces. Punctuation around a word is dropped; a piece of only
/// punctuation ends the sequence. Each phrase once per text.
fn phrases_of(lowered_text: &str) -> BTreeSet<String> {
    let mut phrases = BTreeSet::new();
    for line in lowered_text.lines() {
        let mut run: Vec<&str> = Vec::new();
        let mut runs: Vec<Vec<&str>> = Vec::new();
        for piece in line.split_whitespace() {
            let word = piece.trim_matches(|character: char| !character.is_alphanumeric());
            if word.is_empty() {
                runs.push(std::mem::take(&mut run));
            } else {
                run.push(word);
            }
        }
        runs.push(run);
        for words in runs {
            for start in 0..words.len() {
                for length in 1..=MAX_PHRASE_WORDS {
                    let Some(window) = words.get(start..start.saturating_add(length)) else {
                        break;
                    };
                    let phrase = window.join(" ");
                    if is_meaningful(window, &phrase) {
                        phrases.insert(phrase);
                    }
                }
            }
        }
    }
    phrases
}

fn is_meaningful(words: &[&str], phrase: &str) -> bool {
    phrase.chars().count() >= MIN_PHRASE_CHARS
        && !words.iter().all(|word| FILLER_WORDS.contains(word))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::archive_profile::learn_archive;

    fn folder(name: &str, count: usize, text: impl Fn(usize) -> String) -> Vec<ArchiveText> {
        (0..count)
            .map(|index| ArchiveText {
                document: ArchiveDocument {
                    relative_path: PathBuf::from(format!(
                        "{name}/2024-{:02}-{:02}_item{index}.pdf",
                        index.div_euclid(28) + 1,
                        index.rem_euclid(28) + 1
                    )),
                    date: None,
                },
                text: text(index),
                fields: DocumentFields::default(),
                sender: None,
            })
            .collect()
    }

    fn from_sender(
        mut archive: Vec<ArchiveText>,
        sender: impl Fn(usize) -> Option<&'static str>,
    ) -> Vec<ArchiveText> {
        for (index, entry) in archive.iter_mut().enumerate() {
            entry.sender = sender(index).map(str::to_owned);
        }
        archive
    }

    fn only_sender_candidate(candidates: &[RuleCandidate]) -> &RuleCandidate {
        let senders: Vec<_> = candidates
            .iter()
            .filter(|candidate| {
                candidate.spec.conditions.single_condition().map(|c| c.kind)
                    == Some(ConditionKind::Sender)
            })
            .collect();
        assert_eq!(senders.len(), 1, "{candidates:?}");
        senders[0]
    }

    fn suggest(archive: &[ArchiveText]) -> Vec<RuleCandidate> {
        let documents: Vec<ArchiveDocument> =
            archive.iter().map(|entry| entry.document.clone()).collect();
        suggest_rules(archive, &learn_archive(&documents))
    }

    fn phrases(candidates: &[RuleCandidate], subtree: &str) -> Vec<String> {
        candidates
            .iter()
            .filter(|candidate| candidate.subtree == subtree)
            .map(|candidate| candidate.phrase.clone())
            .collect()
    }

    fn insurance(index: usize) -> String {
        format!(
            "Company A Ltd\npolicy {}\nplease keep this letter",
            1000 + index
        )
    }

    fn taxes(index: usize) -> String {
        format!(
            "Agency B Tax Office\nnotice {}\nplease keep this letter",
            2000 + index
        )
    }

    #[test]
    fn a_folder_whose_documents_share_a_company_yields_that_rule_with_evidence() {
        let mut archive = folder("Insurance", 5, insurance);
        archive.extend(folder("Taxes", 5, taxes));

        let candidates = suggest(&archive);

        let company: Vec<_> = candidates
            .iter()
            .filter(|candidate| candidate.phrase == "company a ltd")
            .collect();
        assert_eq!(company.len(), 1);
        let candidate = company[0];
        assert_eq!(candidate.subtree, "Insurance");
        assert_eq!(candidate.spec.kind.as_deref(), Some("Insurance"));
        assert_eq!(candidate.spec.filing_pattern, "Insurance/{date}_{name}");
        assert_eq!(
            candidate.evidence,
            RuleEvidence {
                matching_in_folder: 5,
                folder_documents: 5,
                matching_elsewhere: 0,
                subtree: Some("Insurance".to_owned()),
            }
        );
        assert_eq!(
            phrases(&candidates, "Taxes"),
            ["agency b tax office", "notice"]
        );
    }

    #[test]
    fn candidates_are_unconfirmed_literal_conditions() {
        let mut archive = folder("Insurance", 5, insurance);
        archive.extend(folder("Taxes", 5, taxes));

        for candidate in suggest(&archive) {
            assert!(!candidate.spec.confirmed);
            assert!(candidate.spec.conditions.single_text().is_some());
        }
    }

    #[test]
    fn a_phrase_common_to_all_folders_yields_nothing() {
        let mut archive = folder("Insurance", 5, insurance);
        archive.extend(folder("Taxes", 5, taxes));

        let all: Vec<String> = suggest(&archive)
            .iter()
            .map(|candidate| candidate.phrase.clone())
            .collect();

        assert!(
            all.iter()
                .all(|phrase| !phrase.contains("keep this letter")),
            "{all:?}"
        );
    }

    #[test]
    fn a_phrase_just_below_the_share_in_the_folder_is_not_proposed() {
        let with_phrase = |count: usize| {
            let mut archive = folder("Insurance", 10, |index| {
                if index < count {
                    format!("Company A Ltd\nref {}", 1000 + index)
                } else {
                    format!("other text {}", 1000 + index)
                }
            });
            archive.extend(folder("Taxes", 10, taxes));
            phrases(&suggest(&archive), "Insurance")
        };

        assert!(with_phrase(7)
            .iter()
            .all(|phrase| phrase != "company a ltd"));
        assert!(with_phrase(8)
            .iter()
            .any(|phrase| phrase == "company a ltd"));
    }

    #[test]
    fn a_phrase_just_above_the_share_elsewhere_is_not_proposed() {
        let with_elsewhere = |count: usize| {
            let mut archive = folder("Insurance", 5, insurance);
            archive.extend(folder("Taxes", 40, |index| {
                if index < count {
                    format!("Company A Ltd\nnotice {}", 2000 + index)
                } else {
                    format!("Agency B notice {}", 2000 + index)
                }
            }));
            phrases(&suggest(&archive), "Insurance")
        };

        assert!(with_elsewhere(2)
            .iter()
            .any(|phrase| phrase == "company a ltd"));
        assert!(with_elsewhere(3)
            .iter()
            .all(|phrase| phrase != "company a ltd"));
    }

    #[test]
    fn a_folder_below_the_minimum_yields_nothing() {
        let mut archive = folder("Insurance", 2, insurance);
        archive.extend(folder("Taxes", 5, taxes));

        assert!(phrases(&suggest(&archive), "Insurance").is_empty());
    }

    #[test]
    fn a_folder_without_a_pattern_yields_nothing() {
        let archive: Vec<ArchiveText> = ["holiday", "notes", "recipe"]
            .iter()
            .map(|name| ArchiveText {
                document: ArchiveDocument {
                    relative_path: PathBuf::from(format!("Misc/{name}.pdf")),
                    date: None,
                },
                text: "Company A Ltd".to_owned(),
                fields: DocumentFields::default(),
                sender: None,
            })
            .collect();

        assert!(suggest(&archive).is_empty());
    }

    #[test]
    fn shorter_phrases_inside_a_longer_one_with_the_same_documents_are_dropped() {
        let archive = folder("Insurance", 4, |index| {
            format!("Company A Ltd\nref {}", 1000 + index)
        });

        assert_eq!(phrases(&suggest(&archive), "Insurance"), ["company a ltd"]);
    }

    #[test]
    fn the_candidates_are_the_same_on_every_run() {
        let mut archive = folder("Insurance", 5, insurance);
        archive.extend(folder("Taxes", 5, taxes));

        assert_eq!(suggest(&archive), suggest(&archive));
    }

    fn filed_by_company_a(archive: Vec<ArchiveText>) -> Vec<ArchiveText> {
        from_sender(archive, |_| Some("Company A"))
    }

    #[test]
    fn a_sender_whose_documents_are_in_one_folder_yields_a_sender_rule_for_that_folder() {
        let archive = filed_by_company_a(folder("Insurance", 5, insurance));

        let candidates = suggest(&archive);

        let candidate = only_sender_candidate(&candidates);
        assert_eq!(
            candidate.spec.conditions,
            RuleConditions::all_of(vec![Condition::new(ConditionKind::Sender, "Company A")])
        );
        assert_eq!(candidate.subtree, "Insurance");
        assert_eq!(candidate.spec.kind.as_deref(), Some("Insurance"));
        assert_eq!(candidate.spec.filing_pattern, "Insurance/{date}_{name}");
        assert!(!candidate.spec.confirmed);
        assert!(candidate.spec.enabled);
    }

    #[test]
    fn the_sender_is_compared_without_case() {
        let archive = from_sender(folder("Insurance", 4, insurance), |index| {
            Some(if index % 2 == 0 {
                "Company A"
            } else {
                "COMPANY A"
            })
        });

        let candidates = suggest(&archive);

        let candidate = only_sender_candidate(&candidates);
        assert_eq!(candidate.evidence.matching_in_folder, 4);
    }

    #[test]
    fn a_sender_just_below_the_share_in_its_folder_yields_no_sender_rule() {
        let with_in_folder = |in_folder: usize| {
            let mut archive = folder("Insurance", in_folder, insurance);
            archive.extend(folder("Taxes", 4 - in_folder, taxes));
            let filed = filed_by_company_a(archive);
            suggest(&filed)
                .iter()
                .any(|candidate| candidate.spec.conditions.required_sender().is_some())
        };

        assert!(!with_in_folder(3), "3 of 4 is 75 percent");
        assert!(with_in_folder(4));
    }

    #[test]
    fn a_sender_with_too_few_documents_in_the_folder_yields_no_sender_rule() {
        let archive = filed_by_company_a(folder("Insurance", 2, insurance));

        assert!(suggest(&archive).iter().all(|candidate| candidate
            .spec
            .conditions
            .required_sender()
            .is_none()));
    }

    #[test]
    fn the_evidence_of_a_sender_rule_counts_the_sender_and_the_folder() {
        let mut archive = folder("Insurance", 5, insurance);
        archive.extend(folder("Taxes", 3, taxes));
        let with_senders = from_sender(archive, |index| {
            (index < 4 || index == 5).then_some("Company A")
        });
        // 4 of the sender's 5 documents lie in Insurance, one in Taxes; the
        // fifth Insurance document has no sender.

        let candidates = suggest(&with_senders);

        let candidate = only_sender_candidate(&candidates);
        assert_eq!(
            candidate.evidence,
            RuleEvidence {
                matching_in_folder: 4,
                folder_documents: 5,
                matching_elsewhere: 1,
                subtree: Some("Insurance".to_owned()),
            }
        );
    }

    #[test]
    fn the_kind_is_taken_from_the_fields_when_enough_documents_agree() {
        let with_kind = |named: usize| {
            let mut archive = filed_by_company_a(folder("Insurance", 5, insurance));
            for (index, entry) in archive.iter_mut().enumerate() {
                entry.fields.kind = (index < named).then(|| "Policy".to_owned());
            }
            only_sender_candidate(&suggest(&archive))
                .spec
                .kind
                .clone()
                .unwrap_or_default()
        };

        assert_eq!(with_kind(4), "Policy");
        assert_eq!(with_kind(3), "Insurance");
    }

    #[test]
    fn only_the_tags_that_enough_documents_carry_are_taken() {
        let mut archive = filed_by_company_a(folder("Insurance", 5, insurance));
        for (index, entry) in archive.iter_mut().enumerate() {
            entry.fields.tags.insert("insurance".to_owned());
            if index < 3 {
                entry.fields.tags.insert("car".to_owned());
            }
        }

        let candidates = suggest(&archive);

        assert_eq!(
            only_sender_candidate(&candidates).spec.tags,
            BTreeSet::from(["insurance".to_owned()])
        );
    }

    #[test]
    fn a_folder_with_a_sender_rule_yields_no_phrase_rules() {
        let mut archive = filed_by_company_a(folder("Insurance", 5, insurance));
        archive.extend(folder("Taxes", 5, taxes));

        let candidates = suggest(&archive);

        assert!(phrases(&candidates, "Insurance")
            .iter()
            .all(|phrase| phrase == "Company A"));
        assert_eq!(phrases(&candidates, "Insurance").len(), 1);
        assert!(!phrases(&candidates, "Taxes").is_empty());
    }

    #[test]
    fn phrase_rules_take_kind_and_tags_from_the_fields_and_name_their_folder() {
        let mut archive = folder("Insurance", 5, insurance);
        for entry in &mut archive {
            entry.fields.tags.insert("insurance".to_owned());
        }

        let candidates = suggest(&archive);

        let candidate = candidates
            .iter()
            .find(|candidate| candidate.phrase == "company a ltd")
            .expect("the phrase rule");
        assert_eq!(
            candidate.spec.tags,
            BTreeSet::from(["insurance".to_owned()])
        );
        assert_eq!(candidate.evidence.subtree.as_deref(), Some("Insurance"));
    }

    #[test]
    fn the_thresholds_of_stored_evidence_depend_on_the_kind_of_condition() {
        let sender = RuleConditions::all_of(vec![Condition::new(ConditionKind::Sender, "A")]);
        let word = RuleConditions::all_of(vec![Condition::new(ConditionKind::Word, "a b")]);
        let evidence = |matching_elsewhere| RuleEvidence {
            matching_in_folder: 4,
            folder_documents: 10,
            matching_elsewhere,
            subtree: None,
        };

        assert!(evidence_meets_thresholds(&sender, &evidence(1), 100));
        assert!(!evidence_meets_thresholds(&sender, &evidence(2), 100));
        assert!(!evidence_meets_thresholds(&word, &evidence(0), 100));
    }
}
