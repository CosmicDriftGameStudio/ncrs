use super::*;

#[test]
fn real_ibans_pass_the_checksum_in_any_spelling() {
    assert!(iban_is_valid("DE89 3704 0044 0532 0130 00"));
    assert!(iban_is_valid("de89370400440532013000"));
    assert!(iban_is_valid("GB82 WEST 1234 5698 7654 32"));
}

#[test]
fn broken_ibans_fail() {
    assert!(!iban_is_valid("DE89 3704 0044 0532 0130 01"));
    assert!(!iban_is_valid("DE89 3704"));
    assert!(!iban_is_valid("DEXX 3704 0044 0532 0130 00"));
    assert!(!iban_is_valid(""));
}

#[test]
fn the_sender_is_required_only_by_a_plain_sender_condition_under_all_of() {
    let list = vec![
        Condition::not(ConditionKind::Sender, "Agency B"),
        Condition::new(ConditionKind::Substring, "Invoice"),
        Condition::new(ConditionKind::Sender, "Company A"),
    ];

    assert_eq!(
        RuleConditions::all_of(list.clone()).required_sender(),
        Some("Company A")
    );
    assert_eq!(RuleConditions::any_of(list).required_sender(), None);
}

#[test]
fn a_single_condition_is_one_plain_condition_of_any_kind_with_a_value() {
    let sender = Condition::new(ConditionKind::Sender, "Company A");
    assert_eq!(
        RuleConditions::all_of(vec![sender.clone()]).single_condition(),
        Some(&sender)
    );
    for list in [
        vec![Condition::not(ConditionKind::Sender, "Company A")],
        vec![Condition::new(ConditionKind::Sender, "  ")],
        vec![
            sender.clone(),
            Condition::new(ConditionKind::Word, "invoice"),
        ],
        Vec::new(),
    ] {
        assert_eq!(RuleConditions::all_of(list).single_condition(), None);
    }
}

#[test]
fn single_text_is_one_plain_substring_or_word_condition() {
    let substring = Condition::new(ConditionKind::Substring, "Invoice");
    let word = Condition::new(ConditionKind::Word, "Invoice");

    assert_eq!(
        RuleConditions::all_of(vec![substring.clone()]).single_text(),
        Some("Invoice")
    );
    assert_eq!(
        RuleConditions::all_of(vec![word.clone()]).single_text_condition(),
        Some(&word)
    );
    for list in [
        vec![Condition::not(ConditionKind::Substring, "Invoice")],
        vec![Condition::not(ConditionKind::Word, "Invoice")],
        vec![Condition::new(ConditionKind::Regex, "Invoice")],
        vec![Condition::new(ConditionKind::Iban, "Invoice")],
        vec![substring.clone(), substring],
    ] {
        assert_eq!(RuleConditions::all_of(list).single_text(), None);
    }
}

#[test]
fn every_condition_kind_round_trips_through_its_stored_name() {
    for kind in ConditionKind::ALL {
        assert_eq!(ConditionKind::parse(kind.as_str()).ok(), Some(kind));
    }
    assert_eq!(ConditionKind::parse("nonsense").ok(), None);
}
