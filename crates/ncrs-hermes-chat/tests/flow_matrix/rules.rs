//! The rule flow after the levels: the saved rule with its name and origin,
//! the question about the existing files, and the change of the condition.

use std::collections::BTreeSet;
use std::sync::Arc;

use ncrs_hermes_chat::Entry;

use super::*;

/// A word of the text of the archive documents and one of the inbox ones.
pub const SEARCH_WORDS: [&str; 2] = ["Entgeltabrechnung", "Gehaltsabrechnung"];

/// The paths the search of the app finds for a word of the documents.
pub fn searched(talk: &Talk) -> BTreeSet<String> {
    SEARCH_WORDS
        .iter()
        .flat_map(|word| talk.searched(word))
        .collect()
}

pub fn old_path(doc: &Doc) -> String {
    if doc.inbox {
        format!("inbox/{}", doc.in_area)
    } else {
        format!("archive/{AREA_PREFIX}{}", doc.in_area)
    }
}

/// The name of the rule: the opened folder, whatever the level words say.
pub fn expected_rule_name() -> String {
    AREA_PREFIX.trim_end_matches('/').to_owned()
}

/// Creates the rule from the trial: it holds `pattern`, a name and the
/// origin chat. The existing documents are asked about, in their own bubble.
pub fn create_rule(talk: &mut Talk, docs: &[Doc], pattern: &str) {
    talk.press(&ChipReply::RuleCreate);
    let rules = talk.rules();
    assert_eq!(rules.len(), 1, "{rules:?}");
    assert_eq!(
        rules[0].spec.filing_pattern,
        format!("{AREA_PREFIX}{pattern}")
    );
    let name = expected_rule_name();
    assert_eq!(rules[0].spec.name.as_deref(), Some(name.as_str()));
    assert_eq!(rules[0].origin, RuleOrigin::Chat);
    let moves = docs.iter().filter(|doc| doc.inbox).count() + archive_moves(docs, pattern).len();
    assert_eq!(
        talk.last_texts(),
        vec![say(
            Msg::HermesChatRuleMoveQuestionOther,
            &[("count", &count(moves))]
        )]
    );
    assert_eq!(
        talk.labels(),
        vec![
            say(Msg::HermesChatRuleRearrangeNow, &[("count", &count(moves))]),
            say(Msg::HermesChatRuleOnlyNew, &[]),
        ]
    );
}

/// Creates the rule and rearranges: every file lies on the disk and in the
/// search at its new path (the archive ones too), the old path is gone from
/// both, and undoing puts every file and every search hit back.
pub fn create_and_check(talk: &mut Talk, docs: &[Doc], pattern: &str) {
    let before = talk.files();
    let found_before = searched(talk);
    create_rule(talk, docs, pattern);
    talk.press(&ChipReply::RuleNow);
    let filed = talk.files();
    let found = searched(talk);
    for doc in docs {
        let old = old_path(doc);
        let new = format!("archive/{AREA_PREFIX}{}", render(pattern, doc.date));
        assert_eq!(filed.get(&new), before.get(&old), "{old} -> {new}");
        // A document the index does not know cannot be searched.
        if found_before.contains(&old) {
            assert!(found.contains(&new), "the search does not find {new}");
        }
        if new != old {
            assert!(!filed.contains_key(&old), "{old} is still there");
            assert!(!found.contains(&old), "the search still finds {old}");
        }
    }
    let old_folder = talk.path("archive/Arbeit/Gehalt");
    if !pattern.starts_with("Gehalt/") {
        assert!(
            !old_folder.exists(),
            "the emptied level 1 folder is removed"
        );
    }
    talk.press(&ChipReply::Undo);
    talk.press(&ChipReply::PutBack);
    assert_eq!(talk.files(), before, "undo puts every file back");
    assert_eq!(
        searched(talk),
        found_before,
        "undo puts the search hits back"
    );
    assert!(old_folder.is_dir(), "undo makes the folders again");
}

/// "No, only new ones": nothing moves, on the disk and in the index; the
/// rule is stored with its name and origin and the usual buttons come back.
pub fn create_and_leave(talk: &mut Talk, docs: &[Doc], pattern: &str) {
    let before = talk.files();
    let found_before = searched(talk);
    create_rule(talk, docs, pattern);
    talk.press(&ChipReply::RuleOnlyNew);
    assert_eq!(talk.files(), before);
    assert_eq!(searched(talk), found_before);
    assert_eq!(
        talk.last_texts(),
        vec![say(Msg::HermesChatRuleOnlyNewDone, &[])]
    );
    let replies: Vec<ChipReply> = talk
        .state
        .chips()
        .iter()
        .map(|chip| chip.reply.clone())
        .collect();
    assert!(!replies.contains(&ChipReply::RuleNow));
    assert!(!replies.contains(&ChipReply::RuleOnlyNew));
    assert!(!replies.is_empty(), "the usual buttons");
    let rule = &talk.rules()[0];
    assert_eq!(
        rule.spec.name.as_deref(),
        Some(expected_rule_name().as_str())
    );
    assert_eq!(rule.origin, RuleOrigin::Chat);
}

pub fn only_new(model: &Arc<CachedModel>, english: bool) {
    let (mut talk, docs) = entered(model, english);
    talk.press(&ChipReply::LevelsKeep);
    check_trial(&talk, &docs, &found_pattern());
    create_and_leave(&mut talk, &docs, &found_pattern());
}

pub const TEXT_NAMES_PAY_SLIP: (&str, &str) = (
    "nur wenn im Text Gehaltsabrechnung steht",
    "only if the text says Gehaltsabrechnung",
);

/// The proposal names why its condition is what it is; a sentence replaces
/// the condition and leaves the levels, the filing and the file name.
pub fn change_the_condition(model: &Arc<CachedModel>, english: bool) {
    let (mut talk, docs) = entered(model, english);
    talk.press(&ChipReply::LevelsKeep);
    check_trial(&talk, &docs, &found_pattern());
    let before = talk.proposal().cloned().expect("the proposal card");
    assert_eq!(
        before.facts[0].label,
        say(Msg::HermesChatProposalAppliesTo, &[])
    );
    assert!(
        before.facts[0].value.contains(SENDER)
            && before.facts[0].value.contains(&count(docs.len() - 5)),
        "the reason in words: {:?}",
        before.facts[0]
    );
    assert!(talk
        .state
        .chips()
        .iter()
        .any(|chip| chip.reply == ChipReply::RuleCondition));

    talk.press(&ChipReply::RuleCondition);
    talk.type_text(pick(english, TEXT_NAMES_PAY_SLIP));

    let card = talk.proposal().cloned().unwrap_or_else(|| {
        panic!(
            "no proposal card again: {:?} {:?}",
            talk.last_texts(),
            talk.labels()
        )
    });
    assert_ne!(
        card.facts[0].value, before.facts[0].value,
        "{:?}",
        card.facts
    );
    assert_eq!(card.facts[2], before.facts[2], "the filing stays");
    assert_eq!(card.facts[3], before.facts[3], "the file name stays");
    talk.press(&ChipReply::RuleCreate);
    let rules = talk.rules();
    assert_eq!(
        rules[0].spec.filing_pattern,
        format!("{AREA_PREFIX}{}", found_pattern()),
        "the levels did not change"
    );
}

/// Saves a rule for the folder, opens the folder again, changes another
/// level and saves again: one rule is left, holding the last pattern, and
/// the second opening says that the rule exists.
pub fn saved_twice_for_the_same_folder(model: &Arc<CachedModel>, english: bool) {
    let (mut talk, docs) = entered(model, english);
    talk.press(&ChipReply::LevelsChange);
    talk.type_text(pick(english, DATE_FIRST));
    talk.press(&ChipReply::LevelsApply);
    let first = "Gehalt/{year}/{date}_Gehaltsabrechnung";
    check_trial(&talk, &docs, first);
    create_and_leave(&mut talk, &docs, first);
    let first_rule = talk.rules().remove(0);

    talk.type_text(if english {
        "open /Arbeit"
    } else {
        "öffne /Arbeit"
    });
    assert!(has_levels_card(&talk), "{:?}", talk.last_texts());
    let rule_name = expected_rule_name();
    assert!(
        talk.last_texts().contains(&say(
            Msg::HermesChatLevelsExistingRule,
            &[
                ("folder", AREA_PREFIX.trim_end_matches('/')),
                ("name", &rule_name)
            ]
        )),
        "the second opening names the rule: {:?}",
        talk.last_texts()
    );
    talk.press(&ChipReply::LevelsChange);
    talk.type_text(pick(english, MONTH));
    talk.press(&ChipReply::LevelsApply);
    let last = "Gehalt/{year}-{month}/Gehaltsabrechnung_{date}";
    check_trial(&talk, &docs, last);
    talk.press(&ChipReply::RuleCreate);

    let rules = talk.rules();
    assert_eq!(rules.len(), 1, "no second rule: {rules:?}");
    assert_eq!(rules[0].id, first_rule.id);
    assert_eq!(rules[0].position, first_rule.position);
    assert_eq!(rules[0].spec.filing_pattern, format!("{AREA_PREFIX}{last}"));
    assert_eq!(rules[0].origin, RuleOrigin::Chat);
    assert!(talk.state.thread().iter().any(|entry| matches!(
        entry,
        Entry::Hermes { parts, .. }
            if parts.contains(&Part::Text(say(Msg::HermesChatRuleUpdated, &[("name", &rule_name)])))
    )));
}

/// Saves a rule for the folder, opens the folder again and renames level 1:
/// the rule keeps its name, id and position, holds the new pattern, and no
/// second rule appears.
pub fn saved_twice_with_a_new_level_word(model: &Arc<CachedModel>, english: bool) {
    let (mut talk, docs) = entered(model, english);
    talk.press(&ChipReply::LevelsChange);
    talk.type_text(pick(english, DATE_FIRST));
    talk.press(&ChipReply::LevelsApply);
    let first = "Gehalt/{year}/{date}_Gehaltsabrechnung";
    check_trial(&talk, &docs, first);
    create_and_leave(&mut talk, &docs, first);
    let first_rule = talk.rules().remove(0);

    talk.type_text(if english {
        "open /Arbeit"
    } else {
        "öffne /Arbeit"
    });
    assert!(has_levels_card(&talk), "{:?}", talk.last_texts());
    talk.press(&ChipReply::LevelsChange);
    talk.type_text(pick(english, RENAME));
    talk.press(&ChipReply::LevelsApply);
    talk.press(&ChipReply::RuleCreate);

    let rule_name = expected_rule_name();
    let rules = talk.rules();
    assert_eq!(rules.len(), 1, "no second rule: {rules:?}");
    assert_eq!(rules[0].id, first_rule.id);
    assert_eq!(rules[0].position, first_rule.position);
    assert!(
        rules[0]
            .spec
            .filing_pattern
            .starts_with(&format!("{AREA_PREFIX}Lohn/")),
        "{:?}",
        rules[0].spec.filing_pattern
    );
    assert_eq!(rules[0].spec.name.as_deref(), Some(rule_name.as_str()));
    assert_eq!(rules[0].origin, RuleOrigin::Chat);
    assert!(talk.state.thread().iter().any(|entry| matches!(
        entry,
        Entry::Hermes { parts, .. }
            if parts.contains(&Part::Text(say(Msg::HermesChatRuleUpdated, &[("name", &rule_name)])))
    )));
}
