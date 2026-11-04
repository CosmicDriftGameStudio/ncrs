//! The thresholds a stored rule suggestion has to meet to still count as
//! evidence.

use ncrs_index::{ConditionKind, RuleConditions, RuleEvidence};

/// A phrase must be in at least this share of its folder's documents, a
/// sender's documents must be in one folder with at least this share, and kind
/// and tags are taken over when this share of the documents agree.
pub const MIN_SHARE_IN_FOLDER_PERCENT: u32 = 80;
/// ... and in at most this share of the documents outside the folder.
pub const MAX_SHARE_ELSEWHERE_PERCENT: u32 = 5;
/// A folder with fewer documents proves nothing.
pub const MIN_FOLDER_DOCUMENTS: u32 = 3;

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

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use ncrs_index::Condition;

    use super::*;

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
