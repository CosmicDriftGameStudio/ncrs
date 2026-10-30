//! Rule candidates read out of how the user has already filed documents: a
//! phrase that most documents of a folder contain and hardly any document
//! elsewhere does. Pure and deterministic; nothing here stores anything.
//!
//! The text of a document is data. A phrase only ever becomes a literal word
//! condition, never a pattern.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use ncrs_index::{
    Condition, ConditionKind, RuleConditions, RuleEvidence, RuleSpec, SubtreePattern, TaxContext,
};

use crate::archive_profile::{subtree_per_document, ArchiveDocument};
use crate::conditions::{normalise_words, words_hold};

/// A phrase must be in at least this share of its folder's documents.
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleCandidate {
    /// The folder the evidence is about.
    pub subtree: String,
    /// The text the condition looks for, lowercase.
    pub phrase: String,
    /// Unconfirmed, with a literal word condition.
    pub spec: RuleSpec,
    pub evidence: RuleEvidence,
}

/// Candidates for every folder (subtree) that has a pattern and enough
/// documents, sorted by folder and phrase. The kind is the last constant
/// folder of the subtree; a document with the condition's phrase in its text
/// is matched the way a rule matches it (case-insensitive, between word boundaries).
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

    let mut candidates = Vec::new();
    for entry in profile {
        let (Some(pattern), Some(folder)) = (
            entry.pattern.as_deref(),
            members.get(entry.subtree.as_str()),
        ) else {
            continue;
        };
        let Some(kind) = entry
            .subtree
            .rsplit('/')
            .next()
            .filter(|kind| !kind.is_empty())
        else {
            continue;
        };
        let in_folder = folder.len();
        if u32::try_from(in_folder).unwrap_or(u32::MAX) < MIN_FOLDER_DOCUMENTS {
            continue;
        }
        let elsewhere: Vec<usize> = (0..lowered.len())
            .filter(|index| !folder.contains(index))
            .collect();
        for found in folder_phrases(&lowered, &normalised, folder, &elsewhere) {
            candidates.push(RuleCandidate {
                subtree: entry.subtree.clone(),
                phrase: found.phrase.clone(),
                spec: RuleSpec {
                    conditions: RuleConditions::all_of(vec![Condition::new(
                        ConditionKind::Word,
                        found.phrase,
                    )]),
                    extract_number: None,
                    extract_date: None,
                    extract_amount: None,
                    kind: kind.to_owned(),
                    tax_context: TaxContext::None,
                    tags: BTreeSet::new(),
                    filing_pattern: pattern.to_owned(),
                    confirmed: false,
                },
                evidence: RuleEvidence {
                    matching_in_folder: to_u32(found.matching_in_folder),
                    folder_documents: to_u32(in_folder),
                    matching_elsewhere: to_u32(found.matching_elsewhere),
                },
            });
        }
    }
    candidates
        .sort_by(|left, right| (&left.subtree, &left.phrase).cmp(&(&right.subtree, &right.phrase)));
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
/// to put `matching_elsewhere` in relation to the ones outside the folder.
pub fn evidence_meets_thresholds(evidence: &RuleEvidence, documents: usize) -> bool {
    let folder = evidence.folder_documents as usize;
    evidence.folder_documents >= MIN_FOLDER_DOCUMENTS
        && at_least_share(
            evidence.matching_in_folder as usize,
            folder,
            MIN_SHARE_IN_FOLDER_PERCENT,
        )
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
            })
            .collect()
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
        assert_eq!(candidate.spec.kind, "Insurance");
        assert_eq!(candidate.spec.filing_pattern, "Insurance/{date}_{name}");
        assert_eq!(
            candidate.evidence,
            RuleEvidence {
                matching_in_folder: 5,
                folder_documents: 5,
                matching_elsewhere: 0,
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
}
