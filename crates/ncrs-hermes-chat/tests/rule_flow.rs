//! Opening a folder of the archive and making a filing rule for it: the
//! levels, what goes into the folder, the trial, and the click that stores the
//! rule. Nothing is saved before that click.

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

    use crate::common::{count, letter, say, Talk};
    use ncrs_ai::Levels;
    use ncrs_hermes_chat::{ChipReply, Entry, Message, Part};
    use ncrs_i18n::Msg;
    use ncrs_index::{Combinator, Condition, ConditionKind, RuleConditions, RuleSpec};

    const DEFAULT_PATTERN: &str = "{year}/{date}_{sender}_{kind}";
    const CHANGE_STRUCTURE: &str =
        r#"{"intent":"change_structure","area":"Telefon","order":"by_month"}"#;

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

    fn levels(pattern: &str) -> Levels {
        Levels::from_pattern(pattern).unwrap()
    }

    /// `Alfa` holds four invoices filed by year; `Telefon` is empty; the
    /// invoices are in the index as well, so a rule finds them.
    fn archive_talk() -> Talk {
        let talk = Talk::in_dir(tempfile::tempdir().unwrap(), &["archive"], &["inbox"]);
        for (month, day) in [(1, 10), (3, 11), (6, 12), (9, 13)] {
            talk.index_document(
                &format!("archive/Alfa/2020/2020-{month:02}-{day}.pdf"),
                &letter(
                    "Alfa AG",
                    &format!("{day}.{month:02}.2020"),
                    "Rechnung",
                    day,
                ),
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

    fn levels_card(talk: &Talk) -> Option<&ncrs_hermes_chat::LevelsCard> {
        talk.last_parts().iter().find_map(|part| match part {
            Part::Levels(card) => Some(card),
            _ => None,
        })
    }

    fn the_only_rule(talk: &Talk) -> ncrs_index::Rule {
        let rules = talk.rules();
        assert_eq!(rules.len(), 1, "{rules:?}");
        rules.into_iter().next().unwrap()
    }

    /// The empty folder `Telefon` up to the proposal of the rule for invoices.
    fn propose_for_telefon(talk: &mut Talk) {
        open_area(talk, "Telefon");
        talk.press(&ChipReply::LevelsKeep);
        talk.model.push_rule(Ok(spec("Rechnung", "Telefon/{year}")));
        talk.type_text("Rechnungen kommen hierhin");
    }

    #[test]
    fn an_empty_folder_gets_the_default_levels_and_the_rule_only_after_the_click() {
        let mut talk = archive_talk();

        open_area(&mut talk, "Telefon");
        assert_eq!(
            talk.last_texts()[0],
            say(Msg::HermesChatFolderEmpty, &[("folder", "Telefon")])
        );
        let card = levels_card(&talk).expect("a card of levels");
        assert_eq!(card.rows.len(), 2, "year and the file name");
        talk.press(&ChipReply::LevelsKeep);
        assert_eq!(talk.rule_count(), 0);

        talk.model.push_rule(Ok(spec("Rechnung", "Telefon/{year}")));
        talk.type_text("Rechnungen kommen hierhin");
        assert_eq!(
            talk.model.described(),
            vec!["Rechnungen kommen hierhin".to_owned()]
        );
        assert_eq!(talk.rule_count(), 0, "a proposal saves nothing");
        assert!(has_chip(&talk, &ChipReply::RuleCreate));

        talk.press(&ChipReply::RuleCreate);
        let rule = the_only_rule(&talk);
        assert_eq!(
            rule.spec.filing_pattern,
            format!("Telefon/{DEFAULT_PATTERN}")
        );
        assert_eq!(rule.spec.conditions.combinator, Combinator::AllOf);
        assert_eq!(
            rule.spec.conditions.list,
            vec![Condition::new(ConditionKind::Word, "Rechnung")]
        );
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatRuleStored, &[])]);
        assert!(has_chip(&talk, &ChipReply::RuleNow));
    }

    #[test]
    fn a_full_folder_that_fits_its_recognized_levels_keeps_them() {
        let mut talk = archive_talk();
        talk.model.push_levels(Ok(levels("{year}/{date}")));

        open_area(&mut talk, "Alfa");
        assert_eq!(talk.model.recognized().len(), 1);
        let card = levels_card(&talk).expect("a card of levels");
        assert_eq!(card.footer, None, "every file fits");
        assert!(has_chip(&talk, &ChipReply::LevelsKeep));
        assert!(!has_chip(&talk, &ChipReply::LevelsDefault));

        talk.press(&ChipReply::LevelsKeep);
        let suggestion = talk
            .state
            .chips()
            .iter()
            .find(|chip| matches!(chip.reply, ChipReply::RuleWhat(_)))
            .expect("a suggestion from the files of the folder")
            .reply
            .clone();
        talk.model.push_rule(Ok(spec("Rechnung", "Alfa/{year}")));
        talk.press(&suggestion);
        talk.press(&ChipReply::RuleCreate);

        assert_eq!(
            the_only_rule(&talk).spec.filing_pattern,
            "Alfa/{year}/{date}"
        );
    }

    #[test]
    fn files_that_deviate_are_shown_and_nothing_is_saved_until_a_click() {
        let mut talk = archive_talk();
        talk.index_document(
            "archive/Alfa/2019/2020-11-14.pdf",
            &letter("Alfa AG", "14.11.2020", "Rechnung", 14),
        );
        talk.model.push_levels(Ok(levels("{year}/{date}")));

        open_area(&mut talk, "Alfa");

        let card = levels_card(&talk).expect("a card of levels");
        assert_eq!(
            card.footer,
            Some(say(
                Msg::HermesChatLevelsFit,
                &[("fitting", &count(4)), ("total", &count(5))]
            ))
        );
        let texts = talk.last_texts();
        assert!(
            texts.iter().any(|line| line.contains("2020-11-14")),
            "{texts:?}"
        );
        for reply in [
            ChipReply::LevelsKeep,
            ChipReply::LevelsChange,
            ChipReply::LevelsDefault,
        ] {
            assert!(has_chip(&talk, &reply), "{reply:?}");
        }
        assert_eq!(talk.rule_count(), 0);

        talk.press(&ChipReply::LevelsDefault);
        let default_card = levels_card(&talk).expect("the default levels");
        assert_eq!(default_card.rows.len(), 2);
        assert_eq!(talk.rule_count(), 0);
    }

    #[test]
    fn cancel_saves_nothing() {
        let mut talk = archive_talk();
        propose_for_telefon(&mut talk);
        assert!(has_chip(&talk, &ChipReply::RuleCreate));

        talk.press(&ChipReply::Cancel);

        assert_eq!(talk.rule_count(), 0);
        assert!(talk.state.session.rule.is_none());
    }

    #[test]
    fn a_rule_the_form_would_refuse_asks_again_and_saves_nothing() {
        let mut talk = archive_talk();
        open_area(&mut talk, "Telefon");
        talk.press(&ChipReply::LevelsKeep);
        let mut refused = spec("Rechnung", "Telefon/{year}");
        refused.conditions = RuleConditions::all_of(Vec::new());
        talk.model.push_rule(Ok(refused));

        talk.type_text("irgendwas");

        assert!(!has_chip(&talk, &ChipReply::RuleCreate));
        assert!(has_chip(&talk, &ChipReply::Cancel));
        assert_eq!(talk.rule_count(), 0);
        let Some(Entry::Hermes { parts, .. }) = talk.state.thread().last() else {
            panic!("{:?}", talk.state.thread().last());
        };
        assert!(
            matches!(parts.first(), Some(Part::Text(line)) if line.contains(&say(Msg::HermesChatRuleWhat, &[("folder", "Telefon")]))),
            "{parts:?}"
        );
    }

    #[test]
    fn a_rule_that_matches_nothing_says_so_and_asks_again() {
        let mut talk = archive_talk();
        open_area(&mut talk, "Telefon");
        talk.press(&ChipReply::LevelsKeep);
        talk.model.push_rule(Ok(spec("Zahnarzt", "Telefon/{year}")));

        talk.type_text("Zahnarzt");

        assert!(!has_chip(&talk, &ChipReply::RuleCreate));
        assert!(talk.last_texts()[0].starts_with(&say(Msg::HermesChatRuleNoneMatch, &[])));
        assert_eq!(talk.rule_count(), 0);
    }

    #[test]
    fn leaving_a_level_out_changes_the_saved_pattern() {
        let mut talk = archive_talk();
        open_area(&mut talk, "Telefon");
        talk.press(&ChipReply::LevelsChange);
        talk.press(&ChipReply::LevelDrop(0));
        talk.press(&ChipReply::LevelsKeep);
        talk.model.push_rule(Ok(spec("Rechnung", "Telefon/{year}")));
        talk.type_text("Rechnungen");

        talk.press(&ChipReply::RuleCreate);

        assert_eq!(
            the_only_rule(&talk).spec.filing_pattern,
            "Telefon/{date}_{sender}_{kind}"
        );
    }

    #[test]
    fn changing_a_proposal_goes_back_to_the_question() {
        let mut talk = archive_talk();
        propose_for_telefon(&mut talk);

        talk.press(&ChipReply::RuleEdit);

        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatRuleWhat, &[("folder", "Telefon")])]
        );
        assert_eq!(talk.rule_count(), 0);
    }

    #[test]
    fn a_description_without_a_folder_takes_folder_and_levels_from_the_rule() {
        let mut talk = archive_talk();
        talk.model
            .push_rule(Ok(spec("Rechnung", "Telefon/{year}/{month}/")));
        talk.send(Message::Open);

        talk.type_text_understood("Rechnungen nach Telefon, Jahr und Monat", CHANGE_STRUCTURE);

        assert_eq!(talk.rule_count(), 0);
        let card = levels_card(&talk).expect("the levels of the rule");
        assert_eq!(card.rows.len(), 3, "year, month and the automatic file");
        assert!(has_chip(&talk, &ChipReply::RuleCreate));
        assert!(has_chip(&talk, &ChipReply::LevelsChange));

        talk.press(&ChipReply::RuleCreate);
        assert_eq!(
            the_only_rule(&talk).spec.filing_pattern,
            "Telefon/{year}/{month}/"
        );
    }

    #[test]
    fn a_typed_open_with_a_slash_names_the_area_below_the_target() {
        let mut talk = archive_talk();
        talk.model.push_levels(Ok(levels("{year}/{date}")));
        talk.send(Message::Open);

        talk.type_text_understood(
            "öffne mal /Alfa",
            r#"{"intent":"open_folder","folder":"/Alfa"}"#,
        );

        assert_eq!(talk.model.recognized().len(), 1, "the folder was read");
        assert!(levels_card(&talk).is_some(), "{:?}", talk.last_texts());
        assert_eq!(talk.rule_count(), 0);
    }

    #[test]
    fn a_nested_folder_is_found_by_its_name_alone() {
        let mut talk = archive_talk();
        talk.model.push_levels(Ok(levels("{date}")));
        talk.send(Message::Open);

        talk.type_text_understood(
            "öffne mal 2020",
            r#"{"intent":"open_folder","folder":"2020"}"#,
        );

        assert_eq!(talk.model.recognized().len(), 1, "the folder was read");
        assert!(levels_card(&talk).is_some(), "{:?}", talk.last_texts());
    }

    #[test]
    fn several_nested_folders_of_a_name_are_asked_about() {
        let mut talk = archive_talk();
        talk.index_document(
            "archive/Beta/2020/2020-05-01.pdf",
            &letter("Beta AG", "01.05.2020", "Rechnung", 5),
        );
        talk.model.push_levels(Ok(levels("{date}")));
        talk.send(Message::Open);

        talk.type_text_understood(
            "öffne mal 2020",
            r#"{"intent":"open_folder","folder":"2020"}"#,
        );

        let choices: Vec<_> = talk
            .state
            .chips()
            .iter()
            .filter_map(|chip| match &chip.reply {
                ChipReply::NamedFolder(path) => Some(path.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            choices,
            vec![
                talk.path("archive/Alfa/2020"),
                talk.path("archive/Beta/2020")
            ]
        );
        assert!(talk.model.recognized().is_empty(), "nothing is read yet");

        talk.press(&ChipReply::NamedFolder(talk.path("archive/Beta/2020")));
        assert_eq!(talk.model.recognized().len(), 1);
        assert!(levels_card(&talk).is_some());
    }

    #[test]
    fn opening_the_chat_again_drops_an_old_rule_session() {
        let mut talk = archive_talk();
        propose_for_telefon(&mut talk);
        assert!(talk.state.session.rule.is_some());

        talk.send(Message::Open);

        assert!(talk.state.session.rule.is_none());
        talk.press(&ChipReply::RuleCreate);
        assert_eq!(talk.rule_count(), 0, "the old proposal is gone");
        talk.type_text_understood("Rechnung", r#"{"intent":"search","text":"Rechnung"}"#);
        assert_eq!(talk.rule_count(), 0);
        assert!(
            talk.state.session.rule.is_none(),
            "the sentence went to the router"
        );
    }

    #[test]
    fn a_folder_outside_the_targets_keeps_the_old_way() {
        let mut talk = archive_talk();
        talk.write(
            "inbox/a.pdf",
            &letter("Alfa AG", "01.01.2020", "Rechnung", 1),
        );
        talk.send(Message::Open);

        talk.press(&ChipReply::Folder(talk.path("inbox")));

        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatSourceSummary,
                &[("folder", "inbox"), ("count", "1")]
            )]
        );
        assert!(talk.state.session.rule.is_none());
        assert!(talk.model.recognized().is_empty());
        assert_eq!(talk.rule_count(), 0);
    }
}
