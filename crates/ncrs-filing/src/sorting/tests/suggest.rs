use chrono::NaiveDate;
use ncrs_ai::{AiError, Levels, LocalModelStatus, ScriptedChatModel};
use ncrs_config::config::SourceSetting;
use ncrs_index::{ConditionKind, DocumentFields, FieldOrigin};

use super::*;

fn fields(kind: &str, day: u32) -> DocumentFields {
    DocumentFields {
        kind: Some(kind.to_owned()),
        date: NaiveDate::from_ymd_opt(2026, 7, day),
        ..DocumentFields::default()
    }
}

/// Three documents each in the folders Schule and Kita of the target T1,
/// known to the index with their kind and date.
fn filed_target(setup: &Setup) {
    for (folder, word, kind) in [
        ("Schule", "elternbrief", "Zeugnis"),
        ("Kita", "kitavertrag", "Vertrag"),
    ] {
        for day in 1..=3 {
            setup.write(
                &format!("T1/{folder}/2026/2026070{day}_{kind}.txt"),
                &format!("{word} gemeinsame Nachricht Nummer{day}"),
            );
        }
    }
    let source = setup.source("T1", &["t1"]);
    setup.plan(&source);
    for (folder, kind) in [("Schule", "Zeugnis"), ("Kita", "Vertrag")] {
        for day in 1..=3 {
            let path = setup.path(&format!("T1/{folder}/2026/2026070{day}_{kind}.txt"));
            setup.set_fields(&path, &fields(kind, day), FieldOrigin::Manual);
        }
    }
}

fn names(suggestions: &Suggestions) -> Vec<&str> {
    suggestions
        .items
        .iter()
        .map(|item| item.name.as_str())
        .collect()
}

fn scheme(setup: &Setup, model: Option<&dyn ncrs_ai::ChatModel>, again: usize) -> Suggestions {
    suggest_scheme(&setup.access, model, &id("t1"), None, again, None).unwrap()
}

fn patterns_wishing(setup: &Setup, wish: &str) -> Vec<String> {
    suggest_scheme(&setup.access, None, &id("t1"), Some(wish), 0, None)
        .unwrap()
        .items
        .into_iter()
        .map(|item| item.spec.filing_pattern)
        .collect()
}

#[test]
fn a_wished_pattern_comes_first_where_it_names_every_document() {
    let setup = Setup::new();
    filed_target(&setup);

    assert_eq!(
        patterns_wishing(&setup, "{year}/{kind}/"),
        ["{year}/{kind}/", "{year}/{kind}/"]
    );
}

#[test]
fn a_wished_pattern_that_lacks_a_value_is_left_out() {
    let setup = Setup::new();
    filed_target(&setup);

    let patterns = patterns_wishing(&setup, "{sender}/{year}/");

    assert!(patterns[0].starts_with("Kita/"), "{patterns:?}");
    assert!(patterns[1].starts_with("Schule/"), "{patterns:?}");
}

#[test]
fn a_wished_pattern_with_the_folder_of_a_group_stays_with_it() {
    let setup = Setup::new();
    filed_target(&setup);

    let patterns = patterns_wishing(&setup, "Schule/{kind}/");

    assert!(patterns[0].starts_with("Kita/"), "{patterns:?}");
    assert_eq!(patterns[1], "Schule/{kind}/");
}

#[test]
fn every_folder_becomes_a_rule_with_a_word_only_its_documents_have() {
    let setup = Setup::new();
    filed_target(&setup);

    let suggestions = scheme(&setup, None, 0);

    assert_eq!(names(&suggestions), ["Kita", "Schule"], "{suggestions:?}");
    assert_eq!(suggestions.files_read, 6);
    assert_eq!(suggestions.fit_suggested, 6);
    assert_eq!(suggestions.fit_none, 0);
    assert_eq!(suggestions.model, ModelNote::NotAvailable);
    let schule = &suggestions.items[1];
    let condition = schule.spec.conditions.single_condition().unwrap();
    assert_eq!(
        (condition.kind, condition.value.as_str()),
        (ConditionKind::Word, "elternbrief")
    );
    assert!(schule.spec.filing_pattern.starts_with("Schule/"));
    assert_eq!(schule.files, 3);
    assert!(!schule.spec.confirmed);
    assert!(schule
        .example
        .as_deref()
        .unwrap()
        .starts_with("Schule/2026/"));
}

#[test]
fn a_folder_without_a_word_its_documents_share_gets_no_rule() {
    let setup = Setup::new();
    filed_target(&setup);
    for (day, text) in [(1, "alphabet"), (2, "bravissimo"), (3, "charmant")] {
        setup.write(&format!("T1/Misc/2026/2026070{day}_Brief.txt"), text);
    }
    let source = setup.source("T1", &["t1"]);
    setup.plan(&source);
    for day in 1..=3 {
        let path = setup.path(&format!("T1/Misc/2026/2026070{day}_Brief.txt"));
        setup.set_fields(&path, &fields("Brief", day), FieldOrigin::Manual);
    }

    let suggestions = scheme(&setup, None, 0);

    assert_eq!(names(&suggestions), ["Kita", "Schule"]);
    assert_eq!(suggestions.fit_none, 3);
}

#[test]
fn the_model_is_asked_once_per_group_and_not_again_after_it_failed() {
    let setup = Setup::new();
    filed_target(&setup);
    let model = ScriptedChatModel::new(LocalModelStatus::NotLoadedYet, []);
    model.push_levels(Ok(
        Levels::from_pattern("Kita/{year}/{date}_{kind}").unwrap()
    ));
    model.push_levels(Ok(
        Levels::from_pattern("Schule/{year}/{date}_{kind}").unwrap()
    ));

    let suggestions = scheme(&setup, Some(&model), 0);

    assert_eq!(model.recognized().len(), 2);
    assert_eq!(suggestions.model, ModelNote::Used);
    assert_eq!(names(&suggestions), ["Kita", "Schule"]);

    let failing = ScriptedChatModel::new(LocalModelStatus::NotLoadedYet, []);
    failing.push_levels(Err(AiError::Model("down")));
    let without_model = scheme(&setup, Some(&failing), 0);
    assert_eq!(failing.recognized().len(), 1);
    assert!(matches!(without_model.model, ModelNote::Failed(_)));
    assert_eq!(names(&without_model), ["Kita", "Schule"]);
}

#[test]
fn a_dismissed_suggestion_does_not_come_back() {
    let setup = Setup::new();
    filed_target(&setup);
    let first = scheme(&setup, None, 0);

    dismiss(&setup.access, &id("t1"), &first.items[..1]).unwrap();

    assert_eq!(names(&scheme(&setup, None, 0)), ["Schule"]);
    assert_eq!(setup.index(|index| index.rules().unwrap().len()), 0);
}

#[test]
fn adopting_saves_exactly_the_ticked_rules_last_and_confirmed() {
    let setup = Setup::new();
    filed_target(&setup);
    setup.rule("t1", "existing", "Old/{number}");
    let suggestions = scheme(&setup, None, 0);

    let made = adopt(&setup.access, &id("t1"), &suggestions.items[1..]).unwrap();

    assert_eq!(made.len(), 1);
    let rules = setup.index(|index| index.rules_of(&id("t1")).unwrap());
    assert_eq!(rules.len(), 2);
    let adopted = rules.last().unwrap();
    assert_eq!(adopted.id, made[0]);
    assert!(adopted.spec.confirmed && adopted.spec.enabled);
    assert_eq!(adopted.spec.name.as_deref(), Some("Schule"));
    assert_eq!(adopted.origin, RuleOrigin::Suggestion);
    assert_eq!(
        adopted.spec.filing_pattern,
        suggestions.items[1].spec.filing_pattern
    );
}

#[test]
fn a_target_with_rules_is_offered_only_what_they_leave_out() {
    let setup = Setup::new();
    filed_target(&setup);
    setup.rule("t1", "elternbrief", "Schule/{number}");

    let suggestions = scheme(&setup, None, 0);

    assert_eq!(suggestions.rules_existing, 1);
    assert_eq!(names(&suggestions), ["Kita"]);
}

#[test]
fn nothing_is_stored_by_looking_for_suggestions() {
    let setup = Setup::new();
    filed_target(&setup);

    scheme(&setup, None, 0);
    scheme(&setup, None, 1);

    assert_eq!(setup.index(|index| index.rules().unwrap().len()), 0);
}

#[test]
fn a_file_without_a_rule_gets_one_that_also_takes_its_like() {
    let mut setup = Setup::new();
    for (name, text) in [
        ("scan-006", "stadtwerke abrechnung energie a1"),
        ("scan-014", "stadtwerke abrechnung energie b2"),
        ("scan-025", "stadtwerke abrechnung energie c3"),
    ] {
        setup.write(&format!("Q1/{name}.txt"), text);
    }
    let source = setup.source("Q1", &["t1"]);
    setup.access.sources.push(SourceSetting {
        source: source.clone(),
        paused: None,
    });
    setup.plan(&source);
    for (day, name) in [(14, "scan-006"), (15, "scan-014"), (16, "scan-025")] {
        let path = setup.path(&format!("Q1/{name}.txt"));
        setup.set_fields(&path, &fields("Abrechnung", day), FieldOrigin::Manual);
    }

    let found = suggest_rule_for(
        &setup.access,
        None,
        &source,
        &setup.path("Q1/scan-006.txt"),
        None,
    )
    .unwrap();

    assert_eq!(found.target, id("t1"));
    assert_eq!(found.rule.files, 3);
    assert_eq!(
        found.also,
        [setup.path("Q1/scan-014.txt"), setup.path("Q1/scan-025.txt")]
    );
    assert!(found
        .rule
        .example
        .as_deref()
        .unwrap()
        .starts_with("Abrechnung/2026/"));
}

#[test]
fn a_file_the_index_does_not_know_is_not_read() {
    let setup = Setup::new();
    let source = setup.source("Q1", &["t1"]);
    let path = setup.write("Q1/unknown.txt", "stadtwerke abrechnung energie");

    let outcome = suggest_rule_for(&setup.access, None, &source, &path, None);

    assert_eq!(outcome.unwrap_err(), SuggestError::NotKnown);
}
