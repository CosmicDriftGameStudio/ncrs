#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use ncrs_index::KindLanguage;
#[test]
fn every_offered_block_is_a_placeholder() {
    for name in FILING_PLACEHOLDERS {
        assert!(Placeholder::parse(name).is_some(), "{name}");
    }
}

#[test]
fn the_first_unknown_block_is_named() {
    assert_eq!(
        unknown_placeholder("{year}/{foo}/{bar}").as_deref(),
        Some("foo")
    );
    assert_eq!(unknown_placeholder("{year}/{sender}"), None);
    assert_eq!(unknown_placeholder("{year"), None);
}

use std::collections::BTreeSet;
use std::path::Path;

use chrono::NaiveDate;
use ncrs_index::{Amount, Currency, RuleConditions};

use super::*;

#[test]
fn a_pattern_is_valid_when_it_can_be_parsed() {
    assert!(pattern_is_valid("{year}/Invoices/{sender}"));
    assert!(!pattern_is_valid("{year}/{nonsense}"));
    assert!(!pattern_is_valid("{year"));
    assert!(!pattern_is_valid("/absolute"));
}

#[test]
fn a_component_that_is_only_a_windows_device_name_is_not_a_valid_pattern() {
    for pattern in ["CON", "{year}/nul.txt", "Com1/", "{year}/LPT9.pdf"] {
        assert!(!pattern_is_valid(pattern), "{pattern}");
    }
    assert!(pattern_is_valid("{year}/CON{number}.pdf"));
    assert!(pattern_is_valid("{year}/Console"));
}

#[test]
fn a_contains_condition_ignores_case_and_an_empty_one_never_holds() {
    assert!(contains_condition_holds(
        "invoice from company a ltd",
        "Company A"
    ));
    assert!(!contains_condition_holds(
        "invoice from agency b",
        "company a"
    ));
    assert!(!contains_condition_holds("anything", ""));
}

fn spec(pattern: &str) -> RuleSpec {
    RuleSpec {
        name: None,
        conditions: RuleConditions::all_of(vec![Condition::new(
            ConditionKind::Substring,
            "company a ltd",
        )]),
        extract_number: Some(r"Invoice\s+(\d+)".to_owned()),
        extract_date: Some(r"Date:\s*(\d{2}\.\d{2}\.\d{4})".to_owned()),
        extract_amount: Some(r"Total:\s*([\d.,]+\s*€)".to_owned()),
        kind: Some("invoice".to_owned()),
        tags: BTreeSet::from(["office".to_owned()]),
        filing_pattern: pattern.to_owned(),
        confirmed: true,
        enabled: true,
    }
}

/// Rule ids are only handed out by the index.
/// The conditions are put in after the index has stored a placeholder, so
/// a test can plan with a regex the index would refuse.
fn rules_from(specs: &[RuleSpec]) -> Vec<Rule> {
    let dir = tempfile::TempDir::new().unwrap();
    let mut index = ncrs_index::Index::create_plain(&dir.path().join("index.db")).unwrap();
    for spec in specs {
        index
            .create_rule(&RuleSpec {
                conditions: RuleConditions::all_of(vec![Condition::new(
                    ConditionKind::Word,
                    "placeholder",
                )]),
                ..spec.clone()
            })
            .unwrap();
    }
    let mut rules = index.rules().unwrap();
    for (rule, spec) in rules.iter_mut().zip(specs) {
        rule.spec = spec.clone();
    }
    rules
}

const TEXT: &str = "Company A Ltd\nInvoice 4711\nDate: 01.03.2026\nTotal: 1.234,56 €\nthank you";

fn plan_for(spec: RuleSpec, text: &str, fields: &DocumentFields) -> Planning {
    plan(
        &rules_from(&[spec]),
        &PlanInput {
            text,
            fields,
            path: None,
            location: None,
            sender_aliases: &SenderAliases::default(),
            manual_fields: &[],
            kinds: &Kinds::built_in(KindLanguage::English),
        },
    )
}

fn planned(planning: Planning) -> FilingPlan {
    match planning {
        Planning::Plan(plan) => plan,
        Planning::Review { reason, .. } => panic!("review: {reason:?}"),
    }
}

fn reason(planning: &Planning) -> ReviewReason {
    match planning {
        Planning::Review { reason, .. } => *reason,
        Planning::Plan(_) => panic!("expected a review"),
    }
}

fn plan_keeping(fields: &DocumentFields, manual_fields: &[Field]) -> FilingPlan {
    planned(plan(
        &rules_from(&[spec("{kind}/{date}")]),
        &PlanInput {
            text: TEXT,
            fields,
            path: None,
            location: None,
            sender_aliases: &SenderAliases::default(),
            manual_fields,
            kinds: &Kinds::built_in(KindLanguage::English),
        },
    ))
}

fn kind_tag_rule(tag: &str, negated: bool) -> RuleSpec {
    let condition = if negated {
        Condition::not(ConditionKind::Tag, tag)
    } else {
        Condition::new(ConditionKind::Tag, tag)
    };
    RuleSpec {
        conditions: RuleConditions::all_of(vec![condition]),
        extract_number: None,
        extract_date: None,
        extract_amount: None,
        kind: None,
        tags: BTreeSet::new(),
        ..spec("{kind}/{sender}")
    }
}

fn fields_of_kind(kind: &str) -> DocumentFields {
    DocumentFields {
        sender: Some("Company A".to_owned()),
        kind: Some(kind.to_owned()),
        ..DocumentFields::default()
    }
}

#[test]
fn a_kind_the_ai_wrote_in_english_is_filed_by_a_german_tag_rule() {
    let planning = plan_for(
        kind_tag_rule("rechnung", false),
        "",
        &fields_of_kind("invoice"),
    );

    assert_eq!(
        planned(planning).relative_path,
        Path::new("Invoice").join("Company A")
    );
}

#[test]
fn a_kind_from_a_german_heading_is_filed_by_an_english_tag_rule() {
    let planning = plan_for(
        kind_tag_rule("invoice", false),
        "",
        &fields_of_kind("Rechnung"),
    );

    assert!(matches!(planning, Planning::Plan(_)));
}

#[test]
fn a_rule_that_excludes_a_kind_excludes_its_other_spelling() {
    let planning = plan_for(
        kind_tag_rule("rechnung", true),
        "",
        &fields_of_kind("invoice"),
    );

    assert_eq!(reason(&planning), ReviewReason::NoMatchingRule);
}

#[test]
fn a_tag_rule_on_one_notice_does_not_match_every_notice() {
    let planning = plan_for(
        kind_tag_rule("steuerbescheid", false),
        "",
        &fields_of_kind("Rentenbescheid"),
    );

    assert_eq!(reason(&planning), ReviewReason::NoMatchingRule);
}

#[test]
fn the_kind_folder_is_written_in_the_language_of_the_plan() {
    let rule = rules_from(&[kind_tag_rule("invoice", false)]);
    let fields = fields_of_kind("invoice");
    let planned_in = |language| {
        let kinds = Kinds::built_in(language);
        planned(plan(
            &rule,
            &PlanInput {
                text: "",
                fields: &fields,
                path: None,
                location: None,
                sender_aliases: &SenderAliases::default(),
                manual_fields: &[],
                kinds: &kinds,
            },
        ))
        .relative_path
    };

    assert_eq!(
        planned_in(KindLanguage::German),
        Path::new("Rechnung").join("Company A")
    );
    assert_eq!(
        planned_in(KindLanguage::English),
        Path::new("Invoice").join("Company A")
    );
}

#[test]
fn a_manual_kind_and_date_survive_a_matching_rule() {
    let by_hand = DocumentFields {
        kind: Some("offer".to_owned()),
        date: NaiveDate::from_ymd_opt(2020, 5, 6),
        ..DocumentFields::default()
    };

    let kept = plan_keeping(&by_hand, &[Field::Kind, Field::Date]);
    assert_eq!(kept.relative_path, Path::new("offer").join("2020-05-06"));
    assert_eq!(kept.fields.kind.as_deref(), Some("offer"));

    let overwritten = plan_keeping(&by_hand, &[]);
    assert_eq!(
        overwritten.relative_path,
        Path::new("Invoice").join("2026-03-01")
    );
}

#[test]
fn only_the_listed_fields_are_kept() {
    let by_hand = DocumentFields {
        kind: Some("offer".to_owned()),
        date: NaiveDate::from_ymd_opt(2020, 5, 6),
        ..DocumentFields::default()
    };

    let kept = plan_keeping(&by_hand, &[Field::Kind]);

    assert_eq!(kept.relative_path, Path::new("offer").join("2026-03-01"));
}

#[test]
fn a_text_rule_matches_and_reads_number_date_and_amount() {
    let known = DocumentFields {
        sender: Some("Company A".to_owned()),
        ..DocumentFields::default()
    };

    let filed = planned(plan_for(
        spec("{year}/{date}_{sender}_{number}.pdf"),
        TEXT,
        &known,
    ));

    assert_eq!(
        filed.relative_path,
        Path::new("2026").join("2026-03-01_Company A_4711.pdf")
    );
    assert_eq!(filed.fields.number.as_deref(), Some("4711"));
    assert_eq!(filed.fields.date, NaiveDate::from_ymd_opt(2026, 3, 1));
    assert_eq!(
        filed.fields.amount,
        Some(Amount {
            minor_units: 123_456,
            currency: Currency::new("EUR").unwrap()
        })
    );
    assert_eq!(filed.fields.kind.as_deref(), Some("invoice"));
    assert!(filed.fields.tags.contains("office"));
}

#[test]
fn a_regex_rule_matches_and_sender_is_compared_without_case() {
    let mut regex_spec = spec("{year}/{number}");
    regex_spec.conditions = RuleConditions::all_of(vec![
        Condition::new(ConditionKind::Regex, r"(?i)company\s+a\s+ltd"),
        Condition::new(ConditionKind::Sender, "COMPANY A"),
    ]);
    let known = DocumentFields {
        sender: Some("company a".to_owned()),
        ..DocumentFields::default()
    };

    assert!(matches!(
        plan_for(regex_spec.clone(), TEXT, &known),
        Planning::Plan(_)
    ));
    assert_eq!(
        reason(&plan_for(regex_spec, TEXT, &DocumentFields::default())),
        ReviewReason::NoMatchingRule
    );
}

#[test]
fn a_kind_condition_holds_for_its_kind_in_any_spelling_and_for_no_other() {
    let mut by_kind = spec("{year}/{number}");
    by_kind.conditions =
        RuleConditions::all_of(vec![Condition::new(ConditionKind::Kind, "invoice")]);
    let dated = |kind: Option<&str>| DocumentFields {
        kind: kind.map(str::to_owned),
        date: NaiveDate::from_ymd_opt(2026, 3, 1),
        number: Some("4711".to_owned()),
        ..DocumentFields::default()
    };

    assert!(matches!(
        plan_for(by_kind.clone(), TEXT, &dated(Some("invoice"))),
        Planning::Plan(_)
    ));
    assert!(matches!(
        plan_for(by_kind.clone(), TEXT, &dated(Some("Invoice"))),
        Planning::Plan(_)
    ));
    for other in [Some("contract"), None] {
        assert_eq!(
            reason(&plan_for(by_kind.clone(), TEXT, &dated(other))),
            ReviewReason::NoMatchingRule
        );
    }
}

#[test]
fn an_invalid_or_too_large_regex_does_not_match_and_does_not_panic() {
    let known = DocumentFields::default();
    for source in ["(unclosed", "(a{1000}){1000}{1000}", r"\p{L}{1000}{1000}"] {
        let mut broken = spec("{number}");
        broken.conditions =
            RuleConditions::all_of(vec![Condition::new(ConditionKind::Regex, source)]);
        assert_eq!(
            reason(&plan_for(broken, TEXT, &known)),
            ReviewReason::NoMatchingRule,
            "{source}"
        );
    }
    let mut broken_extractor = spec("{number}");
    broken_extractor.extract_number = Some("(unclosed".to_owned());
    assert_eq!(
        reason(&plan_for(broken_extractor, TEXT, &known)),
        ReviewReason::MissingField(Field::Number)
    );
}

fn with_conditions(combinator: Combinator, list: Vec<Condition>) -> RuleSpec {
    RuleSpec {
        conditions: RuleConditions { combinator, list },
        ..spec("{number}")
    }
}

fn matched(spec: RuleSpec) -> bool {
    let known = DocumentFields {
        sender: Some("Company A".to_owned()),
        ..DocumentFields::default()
    };
    matches!(plan_for(spec, TEXT, &known), Planning::Plan(_))
}

fn substring(text: &str) -> Condition {
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
fn matched_at(condition: Condition, text: &str, path: Option<&Path>) -> bool {
    matched_in(condition, text, path, None)
}

fn matched_in(
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

fn plan_all(rules: &[Rule]) -> Planning {
    plan(
        rules,
        &PlanInput {
            text: TEXT,
            fields: &DocumentFields::default(),
            path: None,
            location: None,
            sender_aliases: &SenderAliases::default(),
            manual_fields: &[],
            kinds: &Kinds::built_in(KindLanguage::English),
        },
    )
}

#[test]
fn a_single_matching_rule_lists_itself() {
    let rules = rules_from(&[spec("{number}")]);

    let filing = planned(plan_all(&rules));

    assert_eq!(filing.rule, rules[0].id);
    assert_eq!(filing.matching_rules, [rules[0].id]);
}

#[test]
fn two_matching_rules_with_the_same_result_plan_with_both() {
    let rules = rules_from(&[spec("{number}"), spec("{number}")]);

    let filing = planned(plan_all(&rules));

    assert_eq!(filing.rule, rules[0].id);
    assert_eq!(filing.matching_rules, [rules[0].id, rules[1].id]);
    assert_eq!(filing.relative_path, Path::new("4711"));
}

#[test]
fn the_first_matching_rule_files_even_if_a_later_one_gives_another_result() {
    let rules = rules_from(&[spec("{number}"), spec("{year}/{number}")]);

    let filing = planned(plan_all(&rules));

    assert_eq!(filing.rule, rules[0].id);
    assert_eq!(filing.relative_path, Path::new("4711"));
}

#[test]
fn a_disabled_rule_is_skipped() {
    let mut disabled = spec("{year}/{number}");
    disabled.enabled = false;
    let rules = rules_from(&[spec("{number}"), disabled]);

    let filing = planned(plan_all(&rules));

    assert_eq!(filing.matching_rules, [rules[0].id]);
}

#[test]
fn an_unconfirmed_matching_rule_needs_review() {
    let mut unconfirmed = spec("{number}");
    unconfirmed.confirmed = false;

    let planning = plan_for(unconfirmed, TEXT, &DocumentFields::default());

    let Planning::Review {
        reason,
        rule,
        fields,
        ..
    } = planning
    else {
        panic!("expected a review");
    };
    assert_eq!(reason, ReviewReason::UnconfirmedRule);
    assert!(rule.is_some());
    assert_eq!(fields.number.as_deref(), Some("4711"));
}

#[test]
fn a_missing_field_is_named() {
    let planning = plan_for(spec("{sender}/{number}"), TEXT, &DocumentFields::default());
    assert_eq!(reason(&planning), ReviewReason::MissingField(Field::Sender));

    let no_date = plan_for(
        spec("{year}"),
        "Company A Ltd Invoice 4711 Total: 5 €",
        &DocumentFields::default(),
    );
    assert_eq!(reason(&no_date), ReviewReason::MissingField(Field::Date));
}

fn planned_path(pattern: &str, fields: &DocumentFields) -> Option<PathBuf> {
    let mut plain = spec(pattern);
    plain.extract_number = None;
    plain.extract_date = None;
    plain.extract_amount = None;
    match plan_for(plain, TEXT, fields) {
        Planning::Plan(filing) => Some(filing.relative_path),
        Planning::Review { .. } => None,
    }
}

fn known(sender: Option<&str>, number: Option<&str>, date: bool) -> DocumentFields {
    DocumentFields {
        sender: sender.map(str::to_owned),
        number: number.map(str::to_owned),
        date: date.then(|| NaiveDate::from_ymd_opt(2026, 3, 9).unwrap()),
        ..DocumentFields::default()
    }
}

#[test]
fn an_explicit_file_name_leaves_out_the_pieces_it_does_not_know() {
    let pattern = "{year}/{date}_{sender}_{number}";

    assert_eq!(
        planned_path(pattern, &known(Some("Telekom"), Some("42"), true)),
        Some(PathBuf::from("2026/2026-03-09_Telekom_42"))
    );
    assert_eq!(
        planned_path(pattern, &known(Some("Telekom"), None, true)),
        Some(PathBuf::from("2026/2026-03-09_Telekom"))
    );
    assert_eq!(
        planned_path("{sender}_{number}", &known(None, Some("42"), true)),
        Some(PathBuf::from("42"))
    );
    assert_eq!(
        planned_path("{date}_Miete_{number}", &known(None, None, true)),
        Some(PathBuf::from("2026-03-09_Miete"))
    );
}

#[test]
fn an_explicit_file_name_with_no_known_piece_is_a_missing_field() {
    let mut plain = spec("{sender}_{number}");
    plain.extract_number = None;
    let planning = plan_for(plain, TEXT, &DocumentFields::default());

    assert_eq!(reason(&planning), ReviewReason::MissingField(Field::Sender));
    assert_eq!(
        missing_field("{year}/{sender}_{number}", &known(None, None, true)),
        Some(Field::Sender)
    );
}

#[test]
fn a_folder_still_needs_its_field_and_a_slash_pattern_its_date() {
    assert_eq!(
        planned_path("{sender}/{number}", &known(None, Some("42"), true)),
        None
    );
    assert_eq!(planned_path("{year}/", &known(None, None, false)), None);
    assert_eq!(
        planned_path("{year}/", &known(None, None, true)),
        Some(PathBuf::from("2026/2026-03-09"))
    );
}

#[test]
fn an_amount_without_currency_counts_as_missing() {
    let mut bare = spec("{number}");
    bare.extract_amount = Some(r"Total:\s*([\d.,]+)".to_owned());

    let planning = plan_for(
        bare,
        "Company A Ltd Invoice 4711 Date: 01.03.2026 Total: 12,50",
        &DocumentFields::default(),
    );

    assert_eq!(reason(&planning), ReviewReason::MissingField(Field::Amount));
}

#[test]
fn a_pattern_that_cannot_be_rendered_needs_review() {
    for pattern in [
        "{unknown}",
        "{year",
        "year}",
        "/{number}",
        "../{number}",
        "{year}/../{number}",
        "{year}//{number}",
        "C:/{number}",
        "{year}/.",
        "...",
        "",
        "{year}/a\u{0}b",
    ] {
        let planning = plan_for(spec(pattern), TEXT, &DocumentFields::default());
        assert_eq!(
            reason(&planning),
            ReviewReason::UnconfirmedRule,
            "{pattern:?}"
        );
    }
}

#[test]
fn hostile_values_cannot_leave_the_target_folder() {
    let hostile = [
        "../../etc/passwd",
        "..\\..\\windows",
        "/absolute",
        "C:\\temp",
        "a/../b",
        "..",
        "...",
        " . . ",
        "\u{0}\u{7}\t\n",
        "x\u{202E}gpj.exe",
        "<>:\"|?*",
    ];
    for value in hostile {
        let fields = DocumentFields {
            sender: Some(value.to_owned()),
            recipient: Some(value.to_owned()),
            number: Some(value.to_owned()),
            ..DocumentFields::default()
        };
        let mut hostile_rule = spec("{recipient}/{sender}/{kind}/{number}.pdf");
        hostile_rule.kind = Some(value.to_owned());
        hostile_rule.extract_number = None;
        hostile_rule.extract_date = None;
        hostile_rule.extract_amount = None;
        let planning = plan_for(hostile_rule, "Company A Ltd", &fields);
        match planning {
            Planning::Plan(filed) => {
                assert!(stays_below(&filed.relative_path), "{value:?}");
                for component in filed.relative_path.components() {
                    let text = component.as_os_str().to_string_lossy();
                    assert!(
                        !text.chars().any(|c| matches!(
                            c,
                            '/' | '\\' | ':' | '<' | '>' | '|' | '?' | '*' | '"'
                        ) || c.is_control()),
                        "{value:?} -> {text:?}"
                    );
                    assert!(!text.starts_with(['.', ' ']) && !text.ends_with(['.', ' ']));
                }
            }
            Planning::Review { reason, .. } => assert!(
                matches!(reason, ReviewReason::MissingField(_)),
                "{value:?}: {reason:?}"
            ),
        }
    }
}

#[test]
fn a_number_read_from_text_is_sanitised_before_it_is_stored() {
    let mut dotty = spec("{number}.pdf");
    dotty.extract_number = Some(r"Ref:(\S+)".to_owned());
    dotty.extract_date = None;
    dotty.extract_amount = None;

    let filed = planned(plan_for(
        dotty,
        "Company A Ltd Ref:../../x/y",
        &DocumentFields::default(),
    ));

    assert_eq!(filed.fields.number.as_deref(), Some("xy"));
    assert_eq!(filed.relative_path, Path::new("xy.pdf"));
}

#[test]
fn a_number_of_only_dots_is_missing() {
    let mut dotty = spec("{number}");
    dotty.extract_number = Some(r"Ref:(\S+)".to_owned());
    dotty.extract_date = None;
    dotty.extract_amount = None;

    let planning = plan_for(dotty, "Company A Ltd Ref:....", &DocumentFields::default());

    assert_eq!(reason(&planning), ReviewReason::MissingField(Field::Number));
}

#[test]
fn stays_below_checks_components() {
    assert!(stays_below(Path::new("a/b..c/d.pdf")));
    assert!(!stays_below(Path::new("")));
    assert!(!stays_below(Path::new("../a")));
    assert!(!stays_below(Path::new("a/../../b")));
    assert!(!stays_below(Path::new("/a")));
    assert!(!stays_below(Path::new("./a")));
}

#[test]
fn the_parts_of_a_path_mark_the_fields_it_is_made_of() {
    let fields = DocumentFields {
        sender: Some("Company A".to_owned()),
        kind: Some("invoice".to_owned()),
        date: NaiveDate::from_ymd_opt(2024, 3, 5),
        ..DocumentFields::default()
    };
    let parts = render_pattern_parts(
        "{sender}/{date}_{kind}",
        &fields,
        &Kinds::built_in(KindLanguage::English),
    )
    .unwrap();
    let separator = std::path::MAIN_SEPARATOR_STR;
    let shown: Vec<(&str, Option<Field>)> = parts
        .iter()
        .map(|part| (part.text.as_str(), part.field))
        .collect();
    assert_eq!(
        shown,
        [
            ("Company A", Some(Field::Sender)),
            (separator, None),
            ("2024-03-05", Some(Field::Date)),
            ("_", None),
            ("Invoice", Some(Field::Kind)),
        ]
    );
    let joined: String = parts.iter().map(|part| part.text.as_str()).collect();
    assert_eq!(
        Some(PathBuf::from(joined)),
        render_pattern_in(
            "{sender}/{date}_{kind}",
            &fields,
            &Kinds::built_in(KindLanguage::English)
        )
    );
}

#[test]
fn parts_are_refused_where_the_path_is() {
    assert_eq!(
        render_pattern_parts(
            "{sender}",
            &DocumentFields::default(),
            &Kinds::built_in(KindLanguage::English)
        ),
        None
    );
    assert_eq!(
        render_pattern_parts(
            "{nonsense}",
            &DocumentFields::default(),
            &Kinds::built_in(KindLanguage::English)
        ),
        None
    );
}
