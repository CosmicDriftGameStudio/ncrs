use super::*;

pub(super) fn plan_all(rules: &[Rule]) -> Planning {
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

pub(super) fn planned_path(pattern: &str, fields: &DocumentFields) -> Option<PathBuf> {
    let mut plain = spec(pattern);
    plain.extract_number = None;
    plain.extract_date = None;
    plain.extract_amount = None;
    match plan_for(plain, TEXT, fields) {
        Planning::Plan(filing) => Some(filing.relative_path),
        Planning::Review { .. } => None,
    }
}

pub(super) fn known(sender: Option<&str>, number: Option<&str>, date: bool) -> DocumentFields {
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
