use super::app_simulator as simulator;
use std::collections::BTreeSet;

use ncrs_index::{Condition, ConditionKind, Rule, RuleConditions, RuleOrigin, RuleSpec};

use super::*;
use crate::condition_editor::ConditionEdit;
use crate::messages::{RuleFormMessage, RulesMessage, SettingsMessage};
use crate::rules_page::fixtures::rule_id;
use crate::rules_page::RulesPage;
use crate::rules_work::{ListedRule, RulesLoad};

fn app_shows(app: &App, label: &str) -> bool {
    simulator(app).find(label).is_ok()
}

fn listed(
    id: i64,
    origin: RuleOrigin,
    conditions: RuleConditions,
    spec: (&str, &[&str]),
    enabled: bool,
    hits: u64,
) -> ListedRule {
    let (kind, tags) = spec;
    ListedRule {
        rule: Rule {
            id: rule_id(usize::try_from(id).unwrap()),
            position: id,
            origin,
            spec: RuleSpec {
                name: None,
                conditions,
                extract_number: None,
                extract_date: None,
                extract_amount: None,
                kind: Some(kind.to_owned()),
                tags: tags
                    .iter()
                    .map(|tag| (*tag).to_owned())
                    .collect::<BTreeSet<_>>(),
                filing_pattern: "{year}/{sender}".to_owned(),
                confirmed: true,
                enabled,
            },
        },
        global_index: usize::try_from(id).unwrap() - 1,
        hits,
    }
}

fn three_rules() -> RulesPage {
    let named = |name: &str, mut rule: ListedRule| {
        rule.rule.spec.name = Some(name.to_owned());
        rule
    };
    let mut page = RulesPage::new();
    page.loaded(RulesLoad {
        rules: vec![
            named(
                "Telekom-Rechnungen",
                listed(
                    1,
                    RuleOrigin::Hand,
                    RuleConditions::all_of(vec![
                        Condition::new(ConditionKind::Sender, "Company A"),
                        Condition::new(ConditionKind::Word, "invoice"),
                    ]),
                    ("Invoice", &["tax"]),
                    true,
                    12,
                ),
            ),
            named(
                "Strom",
                listed(
                    2,
                    RuleOrigin::Suggestion,
                    RuleConditions::any_of(vec![
                        Condition::new(ConditionKind::Substring, "electricity"),
                        Condition::new(ConditionKind::Iban, "DE89 3704 0044 0532 0130 00"),
                    ]),
                    ("Contract", &[]),
                    true,
                    1,
                ),
            ),
            named(
                "Erinnerungen",
                listed(
                    3,
                    RuleOrigin::Filing,
                    RuleConditions::all_of(vec![
                        Condition::not(ConditionKind::Word, "reminder"),
                        Condition::new(ConditionKind::Folder, "Inbox/Post"),
                    ]),
                    ("Receipt", &[]),
                    false,
                    0,
                ),
            ),
        ],
    });
    page
}

fn empty() -> RulesPage {
    let mut page = RulesPage::new();
    page.loaded(RulesLoad::default());
    page
}

fn shows(page: RulesPage, label: &str) -> bool {
    simulator(&App::with_rules(page)).find(label).is_ok()
}

fn click_sends(page: RulesPage, label: &str, message: RulesMessage) {
    let app = App::with_rules(page);
    let mut sim = simulator(&app);
    sim.click(label).expect(label);
    assert_eq!(
        sim.into_messages()
            .filter(is_action)
            .map(without_focus)
            .collect::<Vec<_>>(),
        [Message::Rules(message)],
        "{label}"
    );
}

fn asking_to_delete() -> RulesPage {
    let mut page = three_rules();
    page.ask_delete(0);
    page
}

#[test]
fn the_button_of_the_remove_question_removes_the_rule() {
    let app = App::with_rules(asking_to_delete());
    let mut sim = simulator(&app);
    sim.click(button_repeating_the_title("Remove rule"))
        .expect("the confirm button");
    assert!(sim
        .into_messages()
        .any(|message| matches!(message, Message::Rules(RulesMessage::ConfirmDelete))));
}

#[test]
fn the_page_lists_the_rules_with_their_buttons_in_the_row() {
    for label in [
        "Telekom-Rechnungen",
        "Strom",
        "Erinnerungen",
        "12 hits",
        "↑",
        "↓",
        "Change",
        "Remove",
        "+ Rule",
    ] {
        assert!(shows(three_rules(), label), "{label}");
    }
}

#[test]
fn an_empty_page_says_where_rules_come_from() {
    assert!(shows(
        empty(),
        "No rules yet. Create one with “+ Rule”, or let rules come from sorting in the chat."
    ));
}

#[test]
fn the_buttons_send_what_they_say_for_their_row() {
    click_sends(three_rules(), "+ Rule", RulesMessage::New);
    click_sends(three_rules(), "Change", RulesMessage::Edit(0));
    click_sends(three_rules(), "Remove", RulesMessage::AskDelete(0));
    click_sends(
        three_rules(),
        "↓",
        RulesMessage::ReorderAt { index: 0, delta: 1 },
    );
}

#[test]
fn the_first_rule_cannot_move_up() {
    let app = App::with_rules(three_rules());
    let mut sim = simulator(&app);
    let _ = sim.click("↑");

    assert!(sim.into_messages().find(is_action).is_none());
}

#[test]
fn save_waits_for_a_change_and_cancel_closes_the_dialog() {
    let app = App::with_rules(with_edit_form());
    let mut sim = simulator(&app);
    let _ = sim.click("Save");
    assert!(
        sim.into_messages().find(is_action).is_none(),
        "Save without a change"
    );

    let mut changed = App::with_rules(with_edit_form());
    let _ = changed.update(Message::Rules(RulesMessage::Form(RuleFormMessage::Name(
        "Contract".to_owned(),
    ))));
    for (label, message) in [
        ("Save", RuleFormMessage::Save),
        ("Cancel", RuleFormMessage::Cancel),
    ] {
        let mut pressed = simulator(&changed);
        pressed.click(label).expect(label);
        assert_eq!(
            pressed
                .into_messages()
                .filter(is_action)
                .collect::<Vec<_>>(),
            [Message::Rules(RulesMessage::Form(message))],
            "{label}"
        );
    }
}

#[test]
fn cancelling_the_dialog_drops_what_was_typed() {
    let mut app = App::with_rules(with_edit_form());
    let _ = app.update(Message::Rules(RulesMessage::Form(RuleFormMessage::Name(
        "Contract".to_owned(),
    ))));

    let _ = app.update(Message::Rules(RulesMessage::Form(RuleFormMessage::Cancel)));

    let page = app.rules_for_test().unwrap();
    assert!(page.form().is_none());
    assert_eq!(page.rows().len(), 3);
    assert_eq!(
        page.rows()[0].rule.spec.name.as_deref(),
        Some("Telekom-Rechnungen")
    );
}

#[test]
fn the_list_stays_the_same_while_a_dialog_is_up() {
    for page in [with_edit_form(), with_new_form(), asking_to_delete()] {
        assert!(shows(page.clone(), "Strom"));
        assert!(shows(page, "Remove"));
    }
}

#[test]
fn the_delete_question_names_the_rule_and_cancel_keeps_it() {
    let mut app = App::with_rules(three_rules());
    let _ = app.update(Message::Rules(RulesMessage::AskDelete(0)));

    assert!(app.rules_for_test().unwrap().is_confirming_delete());
    assert!(app_shows(&app, "Remove rule"));
    assert!(app_shows(&app, "Cancel"));
    let _ = app.update(Message::Rules(RulesMessage::CancelDelete));
    let page = app.rules_for_test().unwrap();
    assert!(!page.is_confirming_delete());
    assert_eq!(page.rows().len(), 3);
}

fn matches_snapshot(page: RulesPage, name: &str) {
    let language = if name.ends_with("_de") {
        Language::German
    } else {
        Language::English
    };
    let app = App::with_rules(page).in_language(language);
    let tall = f32::from(name.starts_with("rule_form"));
    let size = iced::Size::new(1200.0, 760.0 + 340.0 * tall);
    let settings = iced::Settings {
        default_font: ncrs_ui_kit::APP_FONT,
        ..iced::Settings::default()
    };
    let mut simulator = Simulator::with_size(settings, size, app.view());
    let matches = simulator
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image(format!("tests/snapshots/{name}.png"))
        .expect("snapshot comparison");
    assert!(
        matches,
        "the view does not match tests/snapshots/{name}.png"
    );
}

#[test]
fn the_view_matches_its_snapshots() {
    matches_snapshot(three_rules(), "rules_en");
    matches_snapshot(three_rules(), "rules_de");
    matches_snapshot(empty(), "rules_empty_en");
    matches_snapshot(empty(), "rules_empty_de");
    matches_snapshot(asking_to_delete(), "rules_delete_en");
    matches_snapshot(asking_to_delete(), "rules_delete_de");
}

fn with_new_form() -> RulesPage {
    let mut page = three_rules();
    page.begin_new();
    page
}

fn with_edit_form() -> RulesPage {
    let mut page = three_rules();
    page.begin_edit(0);
    page
}

fn with_invalid_form() -> RulesPage {
    let mut page = with_new_form();
    let Some(form) = page.form_mut() else {
        panic!("no form");
    };
    form.update(RuleFormMessage::Condition(ConditionEdit::Add));
    form.update(RuleFormMessage::Condition(ConditionEdit::Kind(
        1,
        ConditionKind::Regex,
    )));
    form.update(RuleFormMessage::Condition(ConditionEdit::Value(
        1,
        "(".to_owned(),
    )));
    form.update(RuleFormMessage::Target(
        crate::filing_pieces::TargetEdit::Remove(0),
    ));
    form.update(RuleFormMessage::Target(
        crate::filing_pieces::TargetEdit::Remove(0),
    ));
    assert_eq!(page.begin_save(), None);
    page
}

fn tag_rules() -> RulesPage {
    let tagged = |id, origin, conditions, hits| {
        let mut rule = listed(id, origin, conditions, ("", &[]), true, hits);
        rule.rule.spec.kind = None;
        rule.rule.spec.filing_pattern = "{recipient}/{year}/{sender}/{date}_{sender}".to_owned();
        rule
    };
    let mut page = RulesPage::new();
    page.loaded(RulesLoad {
        rules: vec![
            tagged(
                1,
                RuleOrigin::Hand,
                RuleConditions::all_of(vec![
                    Condition::new(ConditionKind::Tag, "rechnung"),
                    Condition::new(ConditionKind::Tag, "firma-a"),
                ]),
                12,
            ),
            tagged(
                2,
                RuleOrigin::Chat,
                RuleConditions::all_of(vec![Condition::new(ConditionKind::Tag, "steuer")]),
                4,
            ),
            tagged(
                3,
                RuleOrigin::Suggestion,
                RuleConditions::all_of(vec![
                    Condition::new(ConditionKind::Tag, "vertrag"),
                    Condition::not(ConditionKind::Tag, "firma-a"),
                ]),
                3,
            ),
        ],
    });
    page
}

#[test]
fn the_rules_with_tags_match_their_snapshots() {
    matches_snapshot(tag_rules(), "rules_tags_en");
    matches_snapshot(tag_rules(), "rules_tags_de");
}

#[test]
fn the_rule_form_with_tags_matches_its_snapshots() {
    for language in ["en", "de"] {
        let mut page = tag_rules();
        page.begin_edit(0);
        matches_snapshot(page, &format!("rule_form_tags_{language}"));
    }
}

#[test]
fn the_rule_form_matches_its_snapshots() {
    for language in ["en", "de"] {
        matches_snapshot(with_new_form(), &format!("rule_form_new_{language}"));
        matches_snapshot(with_edit_form(), &format!("rule_form_edit_{language}"));
        matches_snapshot(
            with_invalid_form(),
            &format!("rule_form_invalid_{language}"),
        );
    }
}

fn with_tried_form() -> RulesPage {
    use crate::rules_work::{TriedDocument, TryOutcome, TryResult, TrySection};
    use ncrs_index::ReviewReason;
    use std::path::PathBuf;

    let mut page = with_edit_form();
    let Some(form) = page.form_mut() else {
        panic!("no form");
    };
    let filed = |path: &str| TryOutcome::Filed(PathBuf::from(path));
    form.live_try.status = crate::rule_form::TryStatus::Ready(TryResult {
        documents: vec![
            TriedDocument {
                section: TrySection::Inbox,
                path: PathBuf::from("rent-march.pdf"),
                now: TryOutcome::Review(ReviewReason::NoMatchingRule),
                after: filed("Invoice/2024/Telekom"),
                conflicts: Vec::new(),
            },
            TriedDocument {
                section: TrySection::Review,
                path: PathBuf::from("scan-0412.pdf"),
                now: TryOutcome::Review(ReviewReason::MissingField(ncrs_index::Field::Date)),
                after: filed("Invoice/2024/Telekom"),
                conflicts: Vec::new(),
            },
            TriedDocument {
                section: TrySection::Archive,
                path: PathBuf::from("2021/telekom-may.pdf"),
                now: TryOutcome::Lies(PathBuf::from("2021/telekom-may.pdf")),
                after: filed("Invoice/2021/Telekom"),
                conflicts: vec![rule_id(2), rule_id(3)],
            },
        ],
        more: 0,
        in_archive: 0,
        not_indexed: 0,
        archive_moves: Vec::new(),
        archive_blocked: 0,
        conflict_names: [(rule_id(2), 2), (rule_id(3), 3)].into_iter().collect(),
    });
    page
}

#[test]
fn the_try_of_the_rule_form_matches_its_snapshots() {
    for language in ["en", "de"] {
        matches_snapshot(with_tried_form(), &format!("rule_form_try_{language}"));
    }
}

#[test]
fn an_edit_of_the_form_starts_a_try_and_a_stale_timer_does_nothing() {
    use crate::rule_form::TryStatus;

    let mut app = App::with_rules(three_rules());
    let _ = app.update(Message::Rules(RulesMessage::Edit(0)));
    let waiting = |shown: &App| {
        shown
            .rules_for_test()
            .unwrap()
            .form()
            .unwrap()
            .live_try
            .clone()
    };
    let opened = waiting(&app);
    assert_eq!(opened.status, TryStatus::Waiting);

    let _ = app.update(Message::Rules(RulesMessage::Form(RuleFormMessage::Name(
        "Contract".to_owned(),
    ))));
    let edited = waiting(&app);
    assert_ne!(edited.generation, opened.generation);

    let _ = app.update(Message::Rules(RulesMessage::TryElapsed(opened.generation)));
    assert_eq!(waiting(&app), edited);
    let _ = app.update(Message::Rules(RulesMessage::Tried {
        generation: opened.generation,
        result: Ok(crate::rules_work::TryResult::default()),
    }));
    assert_eq!(waiting(&app), edited);
}

#[test]
fn the_new_rule_button_opens_the_form_and_the_edit_button_fills_it() {
    let mut app = App::with_rules(three_rules());
    let _ = app.update(Message::Rules(RulesMessage::New));
    let new_form = app.rules_for_test().unwrap().form().unwrap();
    assert_eq!(new_form.name, "");
    assert_eq!(new_form.filing.pattern(), "{year}/{sender}/");

    let _ = app.update(Message::Rules(RulesMessage::Form(RuleFormMessage::Cancel)));
    let _ = app.update(Message::Rules(RulesMessage::Edit(0)));
    let edit_form = app.rules_for_test().unwrap().form().unwrap();
    assert_eq!(edit_form.filing.pattern(), "{year}/{sender}");
    assert!(matches!(
        edit_form.target,
        crate::rule_form::RuleFormTarget::Edit(_)
    ));
}

#[test]
fn escape_closes_an_untouched_dialog_first_saves_nothing_and_then_closes_the_settings() {
    let mut app = App::with_rules(with_edit_form());

    let _ = app.update(Message::Settings(SettingsMessage::Close));

    let page = app.rules_for_test().unwrap();
    assert!(page.form().is_none());
    assert_eq!(
        page.rows().first().unwrap().rule.spec.kind.as_deref(),
        Some("Invoice")
    );
    assert!(app.open_hermes_page().is_some());
    let _ = app.update(Message::Settings(SettingsMessage::Close));
    assert!(app.open_hermes_page().is_none());
}

#[test]
fn escape_keeps_a_dialog_with_typed_input_open() {
    let mut app = App::with_rules(with_edit_form());
    let _ = app.update(Message::Rules(RulesMessage::Form(RuleFormMessage::Name(
        "Contract".to_owned(),
    ))));

    let _ = app.update(Message::Settings(SettingsMessage::Close));

    let page = app.rules_for_test().unwrap();
    assert_eq!(page.form().unwrap().name, "Contract");
    assert!(app.open_hermes_page().is_some());
}

#[test]
fn an_invalid_form_sends_nothing_and_shows_what_is_wrong() {
    let mut app = App::with_rules(with_invalid_form()).in_language(Language::English);
    let _ = app.update(Message::Rules(RulesMessage::Form(RuleFormMessage::Save)));

    let page = app.rules_for_test().unwrap();
    assert!(!page.form().unwrap().saving);
    for text in ["Enter a value.", "Add at least one folder."] {
        assert!(simulator(&app).find(text).is_ok(), "{text}");
    }
}
