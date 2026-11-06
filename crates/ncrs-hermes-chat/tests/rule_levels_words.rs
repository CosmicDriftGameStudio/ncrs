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
    use ncrs_ai::{AiError, Levels};
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
                "Gehaltsabrechnung",
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

    #[test]
    fn a_sentence_about_a_level_changes_it_through_the_model() {
        let mut talk = pay_slips();
        let named = levels("{year}/Gehaltsabrechnung_{date}");
        talk.model.push_levels(Ok(named.clone()));
        open_area(&mut talk, "Gehalt");
        talk.press(&ChipReply::LevelsChange);
        talk.model
            .push_levels(Ok(levels("{year}/{date}_Gehaltsabrechnung")));

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
        assert!(has_chip(&talk, &ChipReply::LevelsKeep));
        assert_eq!(talk.state.step, Step::RuleLevels);
        assert_eq!(talk.rule_count(), 0);
    }

    #[test]
    fn a_failed_change_says_so_and_shows_the_levels_again() {
        let mut talk = pay_slips();
        talk.model
            .push_levels(Ok(levels("{year}/Gehaltsabrechnung_{date}")));
        open_area(&mut talk, "Gehalt");
        talk.model.push_levels(Err(AiError::Model("no answer")));

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
        assert!(has_chip(&talk, &ChipReply::LevelDrop(0)));
        assert!(talk.state.session.rule.is_some());
    }

    #[test]
    fn after_cancel_a_sentence_without_a_folder_asks_which_folder() {
        let mut talk = pay_slips();
        talk.model.push_levels(Ok(levels("{year}/")));
        open_area(&mut talk, "Gehalt");
        talk.press(&ChipReply::LevelsChange);
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
