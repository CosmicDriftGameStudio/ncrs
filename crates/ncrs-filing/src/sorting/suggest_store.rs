//! Saving and forgetting suggestions. The index remembers what was discarded
//! for a target, so a suggestion the user turned down does not come back.

use ncrs_index::{Index, RuleEvidence, RuleId, RuleOrigin, RuleSpec, RuleSuggestion, TargetId};

use super::access::{text, SortingAccess};
use super::suggest::{SuggestError, Suggested};

fn evidence_of(suggested: &Suggested) -> RuleEvidence {
    RuleEvidence {
        matching_in_folder: u32::try_from(suggested.files).unwrap_or(u32::MAX),
        folder_documents: suggested.group_documents,
        matching_elsewhere: suggested.elsewhere,
        subtree: None,
    }
}

fn unconfirmed(suggested: &Suggested) -> RuleSpec {
    RuleSpec {
        confirmed: false,
        ..suggested.spec.clone()
    }
}

/// Whether the suggestion may still be shown: not discarded before and not a
/// rule already.
pub fn is_new(index: &Index, target: &TargetId, suggested: &Suggested) -> Result<bool, String> {
    index
        .is_new_suggestion(&unconfirmed(suggested), target)
        .map_err(text)
}

/// Saves the suggestions as confirmed rules, last in the order of the
/// target; the ids of the rules that were made. A suggestion that has a rule
/// already is skipped.
pub fn adopt(
    access: &SortingAccess,
    target: &TargetId,
    suggestions: &[Suggested],
) -> Result<Vec<RuleId>, SuggestError> {
    let _work = access.index_work.begin();
    access
        .with_index_mut(|index| {
            let mut made = Vec::new();
            for suggested in suggestions {
                let stored = index
                    .suggest_rule_in(&unconfirmed(suggested), evidence_of(suggested), target)
                    .map_err(text)?;
                match stored {
                    RuleSuggestion::Created(id) | RuleSuggestion::Refreshed(id) => {
                        let spec = RuleSpec {
                            confirmed: true,
                            ..suggested.spec.clone()
                        };
                        if index.confirm_suggested_rule(id, &spec).map_err(text)? {
                            made.push(id);
                        }
                    }
                    RuleSuggestion::AlreadyConfirmed(_) => {}
                    // The user asks for it now: a former "no" does not count.
                    RuleSuggestion::Discarded => {
                        let spec = RuleSpec {
                            confirmed: true,
                            ..suggested.spec.clone()
                        };
                        let id = index
                            .create_rule_in(&spec, RuleOrigin::Suggestion, target)
                            .map_err(text)?;
                        made.push(id);
                    }
                }
            }
            Ok(made)
        })
        .map_err(SuggestError::Index)
}

/// Remembers that the user does not want these for the target.
pub fn dismiss(
    access: &SortingAccess,
    target: &TargetId,
    suggestions: &[Suggested],
) -> Result<(), SuggestError> {
    let _work = access.index_work.begin();
    access
        .with_index_mut(|index| {
            for suggested in suggestions {
                let stored = index
                    .suggest_rule_in(&unconfirmed(suggested), evidence_of(suggested), target)
                    .map_err(text)?;
                if let RuleSuggestion::Created(id) | RuleSuggestion::Refreshed(id) = stored {
                    index.discard_rule_suggestion(id).map_err(text)?;
                }
            }
            Ok(())
        })
        .map_err(SuggestError::Index)
}
