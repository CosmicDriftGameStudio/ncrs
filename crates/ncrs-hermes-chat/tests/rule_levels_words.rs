//! Levels of a folder of named files, changed in words, and the ways the chat
//! asks for a folder instead of a rule that has none; starting a new chat.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::common::{count, say, Talk};
    use ncrs_ai::{AiError, Levels, Revision};
    use ncrs_hermes_chat::{ChipReply, Entry, LevelsCard, Message, Part, Step};
    use ncrs_i18n::Msg;
    use ncrs_index::{Condition, ConditionKind, DocumentFields, RuleConditions, RuleSpec};

    const CHANGE_STRUCTURE: &str =
        r#"{"intent":"change_structure","area":"Telefon","order":"by_month"}"#;
    const DATE_FIRST: &str = "kannst du bei 3 das datum vorne hin machen?";

    fn levels(pattern: &str) -> Levels {
        Levels::from_pattern(pattern).unwrap()
    }

    fn spec(word: &str, pattern: &str) -> RuleSpec {
        RuleSpec {
            name: None,
            conditions: RuleConditions::all_of(vec![Condition::new(ConditionKind::Word, word)]),
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: None,
            tags: BTreeSet::new(),
            filing_pattern: pattern.to_owned(),
            confirmed: false,
            enabled: false,
        }
    }

    /// `Gehalt` holds pay slips by year, named `Gehaltsabrechnung_{date}`.
    fn pay_slips() -> Talk {
        let talk = Talk::in_dir(tempfile::tempdir().unwrap(), &["archive"], &["inbox"]);
        for (year, month, day) in [(2023, 5, 25), (2023, 6, 26), (2024, 1, 31), (2024, 2, 28)] {
            let id = talk.index_document(
                &format!("archive/Gehalt/{year}/Gehaltsabrechnung_{year}-{month:02}-{day}.pdf"),
                &crate::common::letter(
                    "Muster GmbH",
                    &format!("{day}.{month:02}.{year}"),
                    "Gehaltsabrechnung",
                    day,
                ),
            );
            talk.set_fields(
                id,
                &DocumentFields {
                    sender: Some("Muster GmbH".to_owned()),
                    date: chrono::NaiveDate::from_ymd_opt(year, month, day),
                    ..DocumentFields::default()
                },
            );
        }
        std::fs::create_dir_all(talk.path("archive/Telefon")).unwrap();
        talk.with_rules()
    }

    fn open_area(talk: &mut Talk, area: &str) {
        let folder = talk.path("archive");
        talk.send(Message::ChangeArea {
            folder,
            area: area.to_owned(),
        });
    }

    fn has_chip(talk: &Talk, reply: &ChipReply) -> bool {
        talk.state.chips().iter().any(|chip| &chip.reply == reply)
    }

    fn card(talk: &Talk) -> &LevelsCard {
        talk.last_parts()
            .iter()
            .find_map(|part| match part {
                Part::Levels(card) => Some(card),
                _ => None,
            })
            .expect("a card of levels")
    }

    #[test]
    fn an_automatic_file_name_beside_named_files_is_asked_again_and_the_named_level_wins() {
        let mut talk = pay_slips();
        talk.model.push_levels(Ok(levels("{year}/")));
        talk.model
            .push_levels(Ok(levels("{year}/Gehaltsabrechnung_{date}")));

        open_area(&mut talk, "Gehalt");

        assert_eq!(talk.model.recognized().len(), 2, "asked once more");
        let shown = card(&talk);
        assert_eq!(shown.rows.len(), 2);
        assert_eq!(
            shown.title,
            say(Msg::HermesChatLevelsFound, &[("count", &count(2))])
        );
        assert!(
            shown.rows[1].text.contains("Gehaltsabrechnung_2023-05-25"),
            "{:?}",
            shown.rows[1]
        );
    }

    #[test]
    fn a_file_name_level_in_the_first_answer_is_not_asked_again() {
        let mut talk = pay_slips();
        talk.model
            .push_levels(Ok(levels("{year}/Gehaltsabrechnung_{date}")));

        open_area(&mut talk, "Gehalt");

        assert_eq!(talk.model.recognized().len(), 1);
    }

    #[test]
    fn two_automatic_answers_keep_the_first_and_the_heading_counts_the_rows() {
        let mut talk = pay_slips();
        talk.model.push_levels(Ok(levels("{year}/")));
        talk.model.push_levels(Ok(levels("{year}/")));

        open_area(&mut talk, "Gehalt");

        assert_eq!(talk.model.recognized().len(), 2);
        let shown = card(&talk);
        assert_eq!(shown.rows.len(), 2, "the year and the file name");
        assert_eq!(
            shown.title,
            say(Msg::HermesChatLevelsFound, &[("count", &count(2))])
        );
    }

    fn revision(pattern: &str, done: bool) -> Revision {
        Revision {
            levels: levels(pattern),
            done,
        }
    }

    /// `Gehalt` opened, the levels found, "Change" pressed.
    fn changing(found: &str) -> Talk {
        let mut talk = pay_slips();
        talk.model.push_levels(Ok(levels(found)));
        open_area(&mut talk, "Gehalt");
        talk.press(&ChipReply::LevelsChange);
        talk
    }

    #[test]
    fn change_shows_only_the_hint_not_the_unchanged_card_again() {
        let talk = changing("{year}/Gehaltsabrechnung_{date}");

        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatLevelsChangeHint, &[])]
        );
        assert!(talk
            .last_parts()
            .iter()
            .all(|part| !matches!(part, Part::Levels(_))));
    }

    fn replies(talk: &Talk) -> Vec<ChipReply> {
        talk.state
            .chips()
            .iter()
            .map(|chip| chip.reply.clone())
            .collect()
    }

    fn the_only_rule_pattern(talk: &Talk) -> String {
        let rules = talk.rules();
        assert_eq!(rules.len(), 1, "{rules:?}");
        rules[0].spec.filing_pattern.clone()
    }

    #[test]
    fn the_levels_found_offer_looks_right_change_and_the_standard() {
        let mut talk = pay_slips();
        talk.model
            .push_levels(Ok(levels("{year}/Gehaltsabrechnung_{date}")));

        open_area(&mut talk, "Gehalt");

        assert_eq!(
            replies(&talk),
            vec![
                ChipReply::LevelsKeep,
                ChipReply::LevelsChange,
                ChipReply::LevelsDefault
            ]
        );
        assert!(card(&talk).rows.iter().all(|row| row.before.is_none()));
    }

    #[test]
    fn before_the_first_change_the_only_button_is_done() {
        let talk = changing("{year}/Gehaltsabrechnung_{date}");

        assert_eq!(replies(&talk), vec![ChipReply::LevelsDone]);
        assert!(talk
            .last_texts()
            .contains(&say(Msg::HermesChatLevelsChangeHint, &[])));
    }

    #[test]
    fn a_sentence_about_a_level_changes_it_through_the_model() {
        let mut talk = changing("{year}/Gehaltsabrechnung_{date}");
        let named = levels("{year}/Gehaltsabrechnung_{date}");
        talk.model
            .push_revision(Ok(revision("{year}/{date}_Gehaltsabrechnung", false)));

        talk.type_text(DATE_FIRST);

        assert_eq!(talk.model.revised(), vec![(named, DATE_FIRST.to_owned())]);
        assert!(talk.model.described().is_empty(), "no rule was asked for");
        let shown = card(&talk);
        assert!(
            shown.rows[1]
                .text
                .starts_with("2023-05-25_Gehaltsabrechnung"),
            "{:?}",
            shown.rows[1]
        );
        assert_eq!(shown.rows[0].before, None, "the year stays");
        assert!(
            shown.rows[1]
                .before
                .as_ref()
                .is_some_and(|before| before.contains("Gehaltsabrechnung_2023-05-25")),
            "{:?}",
            shown.rows[1]
        );
        assert_eq!(
            replies(&talk),
            vec![
                ChipReply::LevelsApply,
                ChipReply::LevelsMore,
                ChipReply::LevelsDiscard,
                ChipReply::LevelsDone
            ]
        );
        assert_eq!(talk.state.step, Step::RuleLevels);
        assert_eq!(talk.rule_count(), 0);
    }

    #[test]
    fn after_a_change_the_card_previews_what_applying_does_to_the_files() {
        let mut talk = changing("{year}/Gehaltsabrechnung_{date}");
        talk.model
            .push_revision(Ok(revision("{year}/{date}_Gehaltsabrechnung", false)));

        talk.type_text(DATE_FIRST);

        let texts = talk.last_texts();
        assert_eq!(texts.len(), 1, "{texts:?}");
        let lines: Vec<&str> = texts[0].lines().collect();
        assert_eq!(
            lines[0],
            say(Msg::HermesChatLevelsPreview, &[("count", &count(4))])
        );
        assert_eq!(
            lines[1],
            say(
                Msg::HermesChatRuleExample,
                &[
                    ("from", "Gehalt/2023/Gehaltsabrechnung_2023-05-25.pdf"),
                    ("to", "Gehalt/2023/2023-05-25_Gehaltsabrechnung.pdf"),
                ]
            )
        );
        assert_eq!(lines.len(), 5, "three examples and the rest: {lines:?}");
        assert_eq!(
            lines[4],
            say(Msg::HermesChatRuleExamplesMore, &[("count", &count(1))])
        );
        let fits = say(
            Msg::HermesChatLevelsFit,
            &[("fitting", &count(0)), ("total", &count(4))],
        );
        assert!(
            !texts[0].contains(&fits),
            "no error after an intended change"
        );
        assert!(card(&talk).footer.is_none());
    }

    #[test]
    fn a_change_that_moves_no_file_says_so() {
        let mut talk = changing("{year}/Gehaltsabrechnung_{date}");
        talk.model
            .push_revision(Ok(revision("{year}/Gehaltsabrechnung_{date}", false)));

        talk.type_text("lass alles");

        assert_eq!(replies(&talk), vec![ChipReply::LevelsDone]);
        assert!(talk.last_texts().is_empty());
    }

    #[test]
    fn apply_makes_the_levels_the_pattern_and_goes_to_the_trial() {
        let mut talk = changing("{year}/Gehaltsabrechnung_{date}");
        talk.model
            .push_revision(Ok(revision("{year}/{date}_Gehaltsabrechnung", false)));
        talk.type_text(DATE_FIRST);

        talk.press(&ChipReply::LevelsApply);

        assert!(
            has_chip(&talk, &ChipReply::RuleCreate),
            "{:?}",
            talk.labels()
        );
        talk.press(&ChipReply::RuleCreate);
        assert_eq!(
            the_only_rule_pattern(&talk),
            "Gehalt/{year}/{date}_Gehaltsabrechnung"
        );
    }

    #[test]
    fn done_after_a_change_goes_to_the_trial_like_apply() {
        let mut talk = changing("{year}/Gehaltsabrechnung_{date}");
        talk.model
            .push_revision(Ok(revision("{year}/{date}_Gehaltsabrechnung", false)));
        talk.type_text(DATE_FIRST);

        talk.press(&ChipReply::LevelsDone);

        assert!(has_chip(&talk, &ChipReply::RuleCreate));
        talk.press(&ChipReply::RuleCreate);
        assert_eq!(
            the_only_rule_pattern(&talk),
            "Gehalt/{year}/{date}_Gehaltsabrechnung"
        );
    }

    #[test]
    fn done_before_any_change_takes_the_levels_found() {
        let mut talk = changing("{year}/Gehaltsabrechnung_{date}");

        talk.press(&ChipReply::LevelsDone);

        assert!(has_chip(&talk, &ChipReply::RuleCreate));
        talk.press(&ChipReply::RuleCreate);
        assert_eq!(
            the_only_rule_pattern(&talk),
            "Gehalt/{year}/Gehaltsabrechnung_{date}"
        );
    }

    #[test]
    fn looks_right_goes_straight_to_the_trial() {
        let mut talk = pay_slips();
        talk.model
            .push_levels(Ok(levels("{year}/Gehaltsabrechnung_{date}")));
        open_area(&mut talk, "Gehalt");

        talk.press(&ChipReply::LevelsKeep);

        assert!(has_chip(&talk, &ChipReply::RuleCreate));
        assert_eq!(talk.state.step, Step::RuleWhat);
    }

    #[test]
    fn a_sentence_that_means_done_applies_the_levels_without_a_question() {
        let mut talk = changing("{year}/Gehaltsabrechnung_{date}");
        talk.model
            .push_revision(Ok(revision("{year}/Gehaltsabrechnung_{date}", true)));

        talk.type_text("den Rest so lassen");

        assert!(
            has_chip(&talk, &ChipReply::RuleCreate),
            "{:?}",
            talk.labels()
        );
        talk.press(&ChipReply::RuleCreate);
        assert_eq!(
            the_only_rule_pattern(&talk),
            "Gehalt/{year}/Gehaltsabrechnung_{date}"
        );
    }

    #[test]
    fn discard_shows_the_levels_found_again() {
        let mut talk = changing("{year}/Gehaltsabrechnung_{date}");
        talk.model
            .push_revision(Ok(revision("{year}/{date}_Gehaltsabrechnung", false)));
        talk.type_text(DATE_FIRST);

        talk.press(&ChipReply::LevelsDiscard);

        assert!(
            card(&talk).rows[1]
                .text
                .starts_with("Gehaltsabrechnung_2023-05-25"),
            "{:?}",
            card(&talk).rows
        );
        assert!(card(&talk).rows.iter().all(|row| row.before.is_none()));
        assert_eq!(
            replies(&talk),
            vec![
                ChipReply::LevelsKeep,
                ChipReply::LevelsChange,
                ChipReply::LevelsDefault
            ]
        );
    }

    #[test]
    fn the_card_of_a_change_is_titled_changed_levels_and_the_found_card_keeps_its_title() {
        let mut talk = changing("{year}/Gehaltsabrechnung_{date}");
        talk.model
            .push_revision(Ok(revision("{year}/{date}_Gehaltsabrechnung", false)));
        talk.type_text(DATE_FIRST);

        assert_eq!(
            card(&talk).title,
            say(Msg::HermesChatLevelsChanged, &[("count", &count(2))])
        );
    }

    #[test]
    fn change_more_asks_for_another_change_and_keeps_the_marks() {
        let mut talk = changing("{year}/Gehaltsabrechnung_{date}");
        talk.model
            .push_revision(Ok(revision("{year}/{date}_Gehaltsabrechnung", false)));
        talk.type_text(DATE_FIRST);

        talk.press(&ChipReply::LevelsMore);

        assert!(talk
            .last_texts()
            .contains(&say(Msg::HermesChatLevelsChangeHint, &[])));
        assert!(
            talk.last_parts()
                .iter()
                .all(|part| !matches!(part, Part::Levels(_))),
            "only the hint, the card above is still on screen"
        );
        assert!(has_chip(&talk, &ChipReply::LevelsDone));
    }

    #[test]
    fn a_failed_change_says_so_and_shows_the_levels_again() {
        let mut talk = changing("{year}/Gehaltsabrechnung_{date}");
        talk.model.push_revision(Err(AiError::Model("no answer")));

        talk.type_text(DATE_FIRST);

        let said_before = talk.state.thread().iter().rev().nth(1);
        assert!(
            matches!(said_before, Some(Entry::Hermes { parts, .. })
                if parts.iter().any(|part| matches!(part, Part::Text(line) if *line == say(Msg::HermesChatRuleNotLevels, &[])))),
            "{said_before:?}"
        );
        assert!(talk
            .last_texts()
            .contains(&say(Msg::HermesChatLevelsChangeHint, &[])));
        assert!(has_chip(&talk, &ChipReply::LevelsDone));
        assert!(talk.state.session.rule.is_some());
    }

    #[test]
    fn after_cancel_a_sentence_without_a_folder_asks_which_folder() {
        let mut talk = pay_slips();
        open_area(&mut talk, "Telefon");
        talk.press(&ChipReply::LevelsKeep);
        talk.press(&ChipReply::Cancel);
        assert!(talk.state.session.rule.is_none());
        talk.model.push_rule(Ok(spec("Zahnarzt", "{year}/{month}")));

        talk.type_text_understood(DATE_FIRST, CHANGE_STRUCTURE);

        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatAskReadFolder, &[])]
        );
        assert!(has_chip(&talk, &ChipReply::Folder(talk.path("archive"))));
        assert!(talk.state.session.rule.is_none());
        let said_nothing_empty = talk.state.thread().iter().all(|shown| match shown {
            Entry::Hermes { parts, .. } => parts.iter().all(|part| {
                !matches!(part, Part::Text(line) if *line == say(Msg::HermesChatRuleWhat, &[("folder", "")]))
            }),
            _ => true,
        });
        assert!(said_nothing_empty);
    }

    #[test]
    fn a_text_with_a_slash_inside_the_rule_flow_is_no_folder_bubble() {
        let mut talk = pay_slips();
        open_area(&mut talk, "Telefon");
        talk.press(&ChipReply::LevelsKeep);
        talk.model.push_rule(Ok(spec("Zahnarzt", "Telefon/{year}")));

        talk.type_text("/new");

        assert!(talk
            .state
            .thread()
            .iter()
            .any(|shown| matches!(shown, Entry::User(line) if line == "/new")));
        assert!(!talk
            .state
            .thread()
            .iter()
            .any(|shown| matches!(shown, Entry::UserFolder(_))));
    }

    #[test]
    fn a_lone_word_that_is_no_folder_says_so() {
        let mut talk = pay_slips();
        talk.send(Message::Open);

        talk.type_text_understood("new", r#"{"intent":"open_folder","folder":"new"}"#);

        assert_eq!(
            talk.last_texts()[0],
            say(Msg::HermesChatNotAFolder, &[("name", "new")])
        );
        assert!(talk.state.session.rule.is_none());
    }

    #[test]
    fn a_new_chat_starts_with_the_greeting_and_keeps_the_rules() {
        let mut talk = pay_slips();
        talk.send(Message::Open);
        let greeting = talk.last_texts();
        open_area(&mut talk, "Telefon");
        talk.press(&ChipReply::LevelsKeep);
        talk.model
            .push_rule(Ok(spec("Gehaltsabrechnung", "Telefon/{year}")));
        talk.type_text("Gehaltsabrechnungen kommen hierhin");
        talk.press(&ChipReply::RuleCreate);
        assert_eq!(talk.rule_count(), 1);
        assert!(talk.state.thread().len() > 3);

        talk.send(Message::NewChat);

        assert_eq!(talk.state.thread().len(), 1);
        assert_eq!(talk.last_texts(), greeting);
        assert!(talk.state.session.rule.is_none());
        assert_eq!(talk.state.step, Step::Greeting);
        assert_eq!(talk.rule_count(), 1, "the stored rule stays");
        assert!(talk.state.open);
    }
}
