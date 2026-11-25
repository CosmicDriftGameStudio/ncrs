use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use ncrs_index::{
    Condition, ConditionKind, FieldOrigin, FieldOrigins, KindLanguage, RuleConditions, RuleSpec,
};

use super::*;
use crate::sorting::{OnNoText, OnSeveralTargets, LocationTransfer};

const PATTERN: &str = "{year}/{sender}/{date}_{number}";

/// Rule ids are only handed out by the index; the conditions are put in after
/// the index has stored a placeholder, so any spec can be tried.
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

fn spec(condition: Condition, pattern: &str) -> RuleSpec {
    RuleSpec {
        name: None,
        conditions: RuleConditions::all_of(vec![condition]),
        extract_number: None,
        extract_date: None,
        extract_amount: None,
        kind: None,
        tags: BTreeSet::new(),
        filing_pattern: pattern.to_owned(),
        confirmed: true,
        enabled: true,
    }
}

fn mentions(word: &str, pattern: &str) -> RuleSpec {
    spec(Condition::new(ConditionKind::Substring, word), pattern)
}

fn target(id: &str) -> SortTarget {
    SortTarget {
        id: TargetId::new(id).unwrap(),
        name: id.to_owned(),
        path: PathBuf::from(format!("/archive/{id}")),
        own_volume: None,
    }
}

fn id(text: &str) -> TargetId {
    TargetId::new(text).unwrap()
}

fn source(targets: &[&str]) -> SortSource {
    SortSource {
        path: PathBuf::from("/inbox"),
        include: String::new(),
        exclude: String::new(),
        targets: targets.iter().map(|text| id(text)).collect(),
        watch: true,
        transfer: LocationTransfer::Move,
        no_text: OnNoText::Ask,
        several: OnSeveralTargets::Ask,
    }
}

fn letter() -> DocumentFields {
    DocumentFields {
        sender: Some("Acme".to_owned()),
        kind: Some("invoice".to_owned()),
        date: NaiveDate::from_ymd_opt(2026, 3, 1),
        number: Some("4711".to_owned()),
        ..DocumentFields::default()
    }
}

const TEXT: &str = "Acme Ltd, invoice 4711";

struct Case {
    text: TextState<'static>,
    fields: DocumentFields,
    origins: FieldOrigins,
    known_at: Vec<PathBuf>,
    extension: &'static str,
}

fn case() -> Case {
    Case {
        text: TextState::Text(TEXT),
        fields: letter(),
        origins: FieldOrigins::default(),
        known_at: Vec::new(),
        extension: "pdf",
    }
}

/// `per_target`: the target id and the specs of its rules, in order.
fn decide_case(case: &Case, source: &SortSource, per_target: &[(&str, Vec<RuleSpec>)]) -> Decision {
    let targets: Vec<SortTarget> = per_target.iter().map(|(name, _)| target(name)).collect();
    let rules: Vec<Vec<Rule>> = per_target
        .iter()
        .map(|(_, specs)| rules_from(specs))
        .collect();
    let entries: Vec<TargetRules<'_>> = targets
        .iter()
        .zip(&rules)
        .map(|(target, rules)| TargetRules { target, rules })
        .collect();
    let kinds = Kinds::built_in(KindLanguage::English);
    let words = Words {
        kinds: &kinds,
        sender_aliases: &SenderAliases::default(),
    };
    let subject = Subject {
        relative: Path::new("scan.pdf"),
        extension: Some(case.extension),
        text: case.text,
        fields: &case.fields,
        origins: &case.origins,
        manual: &[],
        known_at: &case.known_at,
    };
    decide(&subject, source, &entries, &words)
}

fn fits(pattern: &str) -> Vec<(&'static str, Vec<RuleSpec>)> {
    vec![("t1", vec![mentions("acme", pattern)])]
}

fn placed(decision: Decision) -> Place {
    match decision {
        Decision::File(place) => place,
        other => panic!("expected a file, got {other:?}"),
    }
}

#[test]
fn decide_gives_every_decision() {
    let one = source(&["t1"]);
    let two = source(&["t1", "t2"]);

    let filed = placed(decide_case(&case(), &one, &fits(PATTERN)));
    assert_eq!(filed.target, id("t1"));
    assert_eq!(filed.relative, Path::new("2026/Acme/2026-03-01_4711.pdf"));

    let both = vec![
        ("t1", vec![mentions("acme", "{year}/{sender}")]),
        ("t2", vec![mentions("invoice", "{kind}/{year}")]),
    ];
    match decide_case(&case(), &two, &both) {
        Decision::Several(places) => {
            let targets: Vec<_> = places.iter().map(|place| place.target.clone()).collect();
            assert_eq!(targets, [id("t1"), id("t2")]);
        }
        other => panic!("expected several, got {other:?}"),
    }

    assert_eq!(
        decide_case(&case(), &one, &[("t1", vec![mentions("zzz", PATTERN)])]),
        Decision::NoRule
    );

    let empty = Case {
        text: TextState::Empty,
        ..case()
    };
    assert_eq!(decide_case(&empty, &one, &fits(PATTERN)), Decision::NoText);

    let unread = Case {
        text: TextState::NotRead,
        ..case()
    };
    assert_eq!(
        decide_case(&unread, &one, &fits(PATTERN)),
        Decision::NotRead
    );

    let numberless = Case {
        fields: DocumentFields {
            number: None,
            ..letter()
        },
        ..case()
    };
    assert!(matches!(
        decide_case(&numberless, &one, &fits(PATTERN)),
        Decision::MissingValue { target, field: Field::Number, .. } if target == id("t1")
    ));

    assert!(matches!(
        decide_case(&case(), &one, &fits("CON.{year}")),
        Decision::BadName { target, .. } if target == id("t1")
    ));

    let there = Case {
        known_at: vec![PathBuf::from("/archive/t1/2026/old.pdf")],
        ..case()
    };
    assert_eq!(
        decide_case(&there, &one, &fits(PATTERN)),
        Decision::AlreadyThere {
            target: id("t1"),
            path: PathBuf::from("/archive/t1/2026/old.pdf")
        }
    );
}

#[test]
fn a_negated_condition_on_a_file_without_text_is_no_text_not_a_match() {
    let not_paid = Condition {
        negated: true,
        ..Condition::new(ConditionKind::Substring, "paid")
    };
    let empty = Case {
        text: TextState::Empty,
        ..case()
    };
    assert_eq!(
        decide_case(&empty, &source(&["t1"]), &[("t1", vec![spec(not_paid, PATTERN)])]),
        Decision::NoText
    );
}

#[test]
fn a_missing_value_for_one_target_is_not_hidden_by_the_other_target() {
    let numberless = Case {
        fields: DocumentFields {
            number: None,
            ..letter()
        },
        ..case()
    };
    let both = vec![
        ("t1", vec![mentions("acme", "{year}/{number}")]),
        ("t2", vec![mentions("acme", "{year}/{sender}")]),
    ];
    assert!(matches!(
        decide_case(&numberless, &source(&["t1", "t2"]), &both),
        Decision::MissingValue { target, rule: _, field: Field::Number } if target == id("t1")
    ));
}

#[test]
fn a_value_a_model_read_triggers_no_condition() {
    let by_sender = vec![(
        "t1",
        vec![spec(Condition::new(ConditionKind::Sender, "Acme"), PATTERN)],
    )];
    let one = source(&["t1"]);
    let by_model = Case {
        text: TextState::Text("a letter without the name"),
        origins: FieldOrigins::default().with(
            Field::Sender,
            FieldOrigin::Ai {
                confidence_percent: 99,
            },
        ),
        ..case()
    };
    assert_eq!(decide_case(&by_model, &one, &by_sender), Decision::NoRule);

    let detected = Case {
        origins: FieldOrigins::default().with(Field::Sender, FieldOrigin::Detected),
        ..by_model
    };
    placed(decide_case(&detected, &one, &by_sender));
}

#[test]
fn a_model_value_fills_the_file_name_but_never_a_folder() {
    let by_model = Case {
        origins: FieldOrigins::default().with(
            Field::Sender,
            FieldOrigin::Ai {
                confidence_percent: 90,
            },
        ),
        ..case()
    };
    let one = source(&["t1"]);
    assert!(matches!(
        decide_case(&by_model, &one, &fits("{sender}/{date}")),
        Decision::MissingValue { field: Field::Sender, .. }
    ));
    let in_name = placed(decide_case(&by_model, &one, &fits("{year}/{date}_{sender}")));
    assert_eq!(in_name.relative, Path::new("2026/2026-03-01_Acme.pdf"));

    let unsure = Case {
        origins: FieldOrigins::default().with(
            Field::Sender,
            FieldOrigin::Ai {
                confidence_percent: 79,
            },
        ),
        ..by_model
    };
    assert!(matches!(
        decide_case(&unsure, &one, &fits("{year}/{date}_{sender}")),
        Decision::MissingValue { field: Field::Sender, .. }
    ));
}

#[test]
fn already_there_counts_per_target() {
    let there = Case {
        known_at: vec![PathBuf::from("/archive/t1/x.pdf")],
        ..case()
    };
    let only_first = vec![
        ("t1", vec![mentions("acme", "{year}")]),
        ("t2", vec![mentions("zzz", "{year}")]),
    ];
    assert!(matches!(
        decide_case(&there, &source(&["t1", "t2"]), &only_first),
        Decision::AlreadyThere { target, .. } if target == id("t1")
    ));

    let both = vec![
        ("t1", vec![mentions("acme", "{year}")]),
        ("t2", vec![mentions("acme", "{year}")]),
    ];
    let new_place = placed(decide_case(&there, &source(&["t1", "t2"]), &both));
    assert_eq!(new_place.target, id("t2"));
}

#[test]
fn only_the_targets_of_the_source_count_and_no_target_means_paused() {
    let both = vec![
        ("t1", vec![mentions("acme", "{year}")]),
        ("t2", vec![mentions("acme", "{year}")]),
    ];
    assert_eq!(
        placed(decide_case(&case(), &source(&["t2"]), &both)).target,
        id("t2")
    );
    assert_eq!(
        decide_case(&case(), &source(&[]), &both),
        Decision::NoRule
    );
}

#[test]
fn the_first_matching_rule_of_a_target_wins_and_the_rest_do_not_decide() {
    let rules = vec![(
        "t1",
        vec![
            mentions("zzz", "Z/{year}"),
            mentions("acme", "First/{year}"),
            mentions("acme", "Second/{year}"),
        ],
    )];
    let place = placed(decide_case(&case(), &source(&["t1"]), &rules));
    assert_eq!(place.relative, Path::new("First/2026.pdf"));
}

#[test]
fn an_unconfirmed_or_switched_off_rule_decides_nothing() {
    let unconfirmed = RuleSpec {
        confirmed: false,
        ..mentions("acme", PATTERN)
    };
    let off = RuleSpec {
        enabled: false,
        ..mentions("acme", PATTERN)
    };
    let one = source(&["t1"]);
    for rule in [unconfirmed, off] {
        assert_eq!(
            decide_case(&case(), &one, &[("t1", vec![rule])]),
            Decision::NoRule
        );
    }
}

#[test]
fn the_file_keeps_its_own_extension() {
    let one = source(&["t1"]);
    let cases = [
        ("pdf", "{year}/{number}.pdf", "2026/4711.pdf"),
        ("PDF", "{year}/{number}.pdf", "2026/4711.PDF"),
        ("PDF", "{year}/{number}", "2026/4711.PDF"),
        ("jpg", "{year}/{number}.pdf", "2026/4711.jpg"),
        ("jpg", "{year}/{number}", "2026/4711.jpg"),
        ("pdf", "{year}/Nr. {number}", "2026/Nr. 4711.pdf"),
        ("pdf", "{year}/v1.2_{number}", "2026/v1.2_4711.pdf"),
        ("pdf", "{year}/", "2026/2026-03-01_Acme_4711.pdf"),
    ];
    for (extension, pattern, expected) in cases {
        let with = Case {
            extension,
            ..case()
        };
        let place = placed(decide_case(&with, &one, &fits(pattern)));
        assert_eq!(place.relative, Path::new(expected), "{extension} {pattern}");
    }
}

#[test]
fn a_pattern_is_filled_as_typed_and_never_leaves_a_piece_out() {
    let one = source(&["t1"]);
    let long = "Bank/{sender}/{year}/{year}{month}{day}_{kind}_{number}.pdf";
    let place = placed(decide_case(&case(), &one, &fits(long)));
    assert_eq!(place.relative, Path::new("Bank/Acme/2026/20260301_Invoice_4711.pdf"));

    let numberless = Case {
        fields: DocumentFields {
            number: None,
            ..letter()
        },
        ..case()
    };
    assert!(matches!(
        decide_case(&numberless, &one, &fits(long)),
        Decision::MissingValue { field: Field::Number, .. }
    ));
    let dateless = Case {
        fields: DocumentFields {
            date: None,
            ..letter()
        },
        ..case()
    };
    assert!(matches!(
        decide_case(&dateless, &one, &fits("{sender}/")),
        Decision::MissingValue { field: Field::Date, .. }
    ));
}
