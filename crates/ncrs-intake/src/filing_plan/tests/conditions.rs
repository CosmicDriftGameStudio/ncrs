use super::*;

pub(super) fn with_conditions(combinator: Combinator, list: Vec<Condition>) -> RuleSpec {
    RuleSpec {
        conditions: RuleConditions { combinator, list },
        ..spec("{number}")
    }
}

pub(super) fn matched(spec: RuleSpec) -> bool {
    let known = DocumentFields {
        sender: Some("Company A".to_owned()),
        ..DocumentFields::default()
    };
    matches!(plan_for(spec, TEXT, &known), Planning::Plan(_))
}

pub(super) fn substring(text: &str) -> Condition {
    Condition::new(ConditionKind::Substring, text)
}

#[test]
fn all_of_needs_every_condition() {
    let both = vec![substring("invoice 4711"), substring("thank you")];
    let one_missing = vec![substring("invoice 4711"), substring("reminder")];

    assert!(matched(with_conditions(Combinator::AllOf, both)));
    assert!(!matched(with_conditions(Combinator::AllOf, one_missing)));
}

#[test]
fn any_of_needs_one_condition() {
    let one = vec![substring("reminder"), substring("invoice 4711")];
    let none = vec![substring("reminder"), substring("credit note")];

    assert!(matched(with_conditions(Combinator::AnyOf, one)));
    assert!(!matched(with_conditions(Combinator::AnyOf, none)));
}

#[test]
fn a_negated_condition_excludes() {
    let absent = vec![
        substring("invoice"),
        Condition::not(ConditionKind::Substring, "reminder"),
    ];
    let present = vec![
        substring("invoice"),
        Condition::not(ConditionKind::Substring, "thank you"),
    ];
    let wrong_sender = vec![
        substring("invoice"),
        Condition::not(ConditionKind::Sender, "company a"),
    ];

    assert!(matched(with_conditions(Combinator::AllOf, absent)));
    assert!(!matched(with_conditions(Combinator::AllOf, present)));
    assert!(!matched(with_conditions(Combinator::AllOf, wrong_sender)));
}

#[test]
fn a_condition_that_cannot_be_evaluated_blocks_the_rule_even_under_not_and_any_of() {
    let unusable = [
        Condition::not(ConditionKind::Regex, "(unclosed"),
        Condition::new(ConditionKind::Regex, "(unclosed"),
        Condition::not(ConditionKind::Substring, ""),
        Condition::new(ConditionKind::Substring, ""),
    ];
    for condition in unusable {
        let any = vec![substring("invoice"), condition.clone()];
        let all = vec![substring("invoice"), condition.clone()];
        assert!(
            !matched(with_conditions(Combinator::AnyOf, any)),
            "{condition:?}"
        );
        assert!(
            !matched(with_conditions(Combinator::AllOf, all)),
            "{condition:?}"
        );
    }
}

#[test]
fn a_negated_sender_condition_blocks_the_rule_while_no_sender_is_known() {
    let rule = |condition: Condition| with_conditions(Combinator::AllOf, vec![condition]);
    let no_sender = DocumentFields::default();
    let other_sender = DocumentFields {
        sender: Some("Company A".to_owned()),
        ..DocumentFields::default()
    };
    let negated = Condition::not(ConditionKind::Sender, "Agency B");
    let blocked = |spec: RuleSpec, fields: &DocumentFields| {
        matches!(
            plan_for(spec, TEXT, fields),
            Planning::Review {
                reason: ReviewReason::NoMatchingRule,
                ..
            }
        )
    };

    assert!(blocked(rule(negated.clone()), &no_sender));
    assert!(!blocked(rule(negated), &other_sender));
    let plain = Condition::new(ConditionKind::Sender, "Agency B");
    assert!(blocked(rule(plain), &no_sender));
}

#[test]
fn a_sender_condition_follows_a_confirmed_name_alias_only() {
    let rules = rules_from(&[with_conditions(
        Combinator::AllOf,
        vec![Condition::new(ConditionKind::Sender, "Telekom")],
    )]);
    let fields = DocumentFields {
        sender: Some("Telekom Deutschland GmbH".to_owned()),
        ..DocumentFields::default()
    };
    let planned_with = |aliases: &SenderAliases| {
        !matches!(
            plan(
                &rules,
                &PlanInput {
                    text: "",
                    fields: &fields,
                    path: None,
                    location: None,
                    sender_aliases: aliases,
                    manual_fields: &[],
                    kinds: &Kinds::built_in(KindLanguage::English),
                },
            ),
            Planning::Review {
                reason: ReviewReason::NoMatchingRule,
                ..
            }
        )
    };
    let confirmed = SenderAliases::from_confirmed([(
        ncrs_index::SenderAliasKind::Name,
        "Telekom Deutschland GmbH",
        "Telekom",
    )]);

    assert!(planned_with(&confirmed));
    assert!(!planned_with(&SenderAliases::default()));
}

/// The rule found the document; fields it cannot fill do not matter here.
pub(super) fn matched_at(condition: Condition, text: &str, path: Option<&Path>) -> bool {
    matched_in(condition, text, path, None)
}

pub(super) fn matched_in(
    condition: Condition,
    text: &str,
    path: Option<&Path>,
    location: Option<&Path>,
) -> bool {
    let rules = rules_from(&[with_conditions(Combinator::AllOf, vec![condition])]);
    let fields = DocumentFields::default();
    let planning = plan(
        &rules,
        &PlanInput {
            text,
            fields: &fields,
            path,
            location,
            sender_aliases: &SenderAliases::default(),
            manual_fields: &[],
            kinds: &Kinds::built_in(KindLanguage::English),
        },
    );
    !matches!(
        planning,
        Planning::Review {
            reason: ReviewReason::NoMatchingRule,
            ..
        }
    )
}

#[test]
fn a_word_condition_does_not_match_inside_another_word() {
    let word = |text| Condition::new(ConditionKind::Word, text);

    assert!(matched_at(word("invoice"), TEXT, None));
    assert!(!matched_at(word("invoic"), TEXT, None));
    assert!(!matched_at(word("bahn"), "Autobahn Bahnhof", None));
    assert!(matched_at(word("company a ltd"), "Company A\n  Ltd", None));
}

#[test]
fn the_new_condition_kinds_are_evaluated_from_the_text() {
    let text = "Nordlicht GmbH\n\nErika Mustermann\n20457 Hamburg\n\n\
        rechnung@billing.nordlicht.de IBAN DE00 0000 0000 0000 0000 00 USt DE 123 456 789";
    for (condition, expected) in [
        (
            Condition::new(ConditionKind::SenderAddress, "nordlicht.de"),
            true,
        ),
        (
            Condition::new(ConditionKind::SenderAddress, "other.de"),
            false,
        ),
        (
            Condition::new(ConditionKind::Iban, "DE00000000000000000000"),
            true,
        ),
        (
            Condition::new(ConditionKind::Iban, "DE00000000000000000001"),
            false,
        ),
        (Condition::new(ConditionKind::TaxId, "DE123456789"), true),
        (
            Condition::new(ConditionKind::Recipient, "Erika Mustermann"),
            true,
        ),
        (Condition::new(ConditionKind::Recipient, "Nordlicht"), false),
    ] {
        assert_eq!(
            matched_at(condition.clone(), text, None),
            expected,
            "{condition:?}"
        );
    }
}

#[test]
fn a_folder_condition_needs_a_path_even_when_negated() {
    let path = Path::new("/archive/Steuer/2025/bill.pdf");
    let folder = Condition::new(ConditionKind::Folder, "steuer/2025");
    let not_folder = Condition::not(ConditionKind::Folder, "steuer/2025");
    let elsewhere = Condition::not(ConditionKind::Folder, "privat");

    assert!(matched_at(folder.clone(), TEXT, Some(path)));
    assert!(!matched_at(not_folder.clone(), TEXT, Some(path)));
    assert!(matched_at(elsewhere.clone(), TEXT, Some(path)));
    assert!(!matched_at(folder, TEXT, None));
    assert!(!matched_at(not_folder, TEXT, None));
    assert!(!matched_at(elsewhere, TEXT, None));
}

#[test]
fn a_read_folder_condition_holds_for_the_location_the_document_lies_under() {
    let scans = Path::new("/a/Scans");
    let neighbour = Path::new("/a/Scans2");
    let read_folder = Condition::new(ConditionKind::ReadFolder, "/a/Scans");
    let not_read_folder = Condition::not(ConditionKind::ReadFolder, "/a/Scans");
    let path = Some(Path::new("2025/bill.pdf"));

    assert!(matched_in(read_folder.clone(), TEXT, path, Some(scans)));
    assert!(!matched_in(
        read_folder.clone(),
        TEXT,
        path,
        Some(neighbour)
    ));
    assert!(!matched_in(
        not_read_folder.clone(),
        TEXT,
        path,
        Some(scans)
    ));
    assert!(matched_in(
        not_read_folder.clone(),
        TEXT,
        path,
        Some(neighbour)
    ));
    assert!(!matched_in(read_folder, TEXT, path, None));
    assert!(!matched_in(not_read_folder, TEXT, path, None));
}

#[test]
fn the_sender_is_only_filled_from_a_plain_sender_condition_under_all_of() {
    let unconfirmed_sender = |combinator, condition| {
        let mut spec = with_conditions(combinator, vec![substring("invoice 4711"), condition]);
        spec.confirmed = false;
        spec
    };
    let sender_of = |spec: RuleSpec| match plan_for(spec, TEXT, &DocumentFields::default()) {
        Planning::Review { fields, .. } => fields.sender,
        Planning::Plan(_) => panic!("expected a review"),
    };

    let sender = Condition::new(ConditionKind::Sender, "Company A");
    let negated = Condition::not(ConditionKind::Sender, "Agency B");

    assert_eq!(
        sender_of(unconfirmed_sender(Combinator::AnyOf, sender)),
        None
    );
    assert_eq!(
        sender_of(unconfirmed_sender(Combinator::AllOf, negated)),
        None
    );
}

#[test]
fn a_rule_without_a_condition_matches_nothing() {
    // The index refuses such a rule; the planner does not rely on that.
    let mut rules = rules_from(&[spec("{number}")]);
    rules[0].spec.conditions = RuleConditions::default();
    let fields = DocumentFields::default();

    let planning = plan(
        &rules,
        &PlanInput {
            text: TEXT,
            fields: &fields,
            path: None,
            location: None,
            sender_aliases: &SenderAliases::default(),
            manual_fields: &[],
            kinds: &Kinds::built_in(KindLanguage::English),
        },
    );

    assert_eq!(reason(&planning), ReviewReason::NoMatchingRule);
}

#[test]
fn the_first_confirmed_rule_wins_over_an_earlier_unconfirmed_one() {
    let mut unconfirmed = spec("a");
    unconfirmed.confirmed = false;
    let rules = rules_from(&[unconfirmed, spec("{number}")]);
    let fields = DocumentFields::default();

    let planning = plan(
        &rules,
        &PlanInput {
            text: TEXT,
            fields: &fields,
            path: None,
            location: None,
            sender_aliases: &SenderAliases::default(),
            manual_fields: &[],
            kinds: &Kinds::built_in(KindLanguage::English),
        },
    );

    assert_eq!(planned(planning).rule, rules[1].id);
}
