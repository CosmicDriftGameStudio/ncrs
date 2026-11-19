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

mod conditions;
mod plans;
