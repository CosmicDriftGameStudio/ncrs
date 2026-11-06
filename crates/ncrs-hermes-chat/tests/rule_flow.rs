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
    use ncrs_ai::{Levels, Revision};
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
            let id = talk.index_document(
                &format!("archive/Alfa/2020/2020-{month:02}-{day}.pdf"),
                &letter(
                    "Alfa AG",
                    &format!("{day}.{month:02}.2020"),
                    "Rechnung",
                    day,
                ),
            );
            talk.set_fields(
                id,
                &ncrs_index::DocumentFields {
                    sender: Some("Alfa AG".to_owned()),
                    kind: Some("invoice".to_owned()),
                    date: chrono::NaiveDate::from_ymd_opt(2020, month, day),
                    ..ncrs_index::DocumentFields::default()
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
        assert!(
            has_chip(&talk, &ChipReply::RuleNow),
            "the four archive documents would move under the new levels"
        );
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
        assert!(has_chip(&talk, &ChipReply::LevelsDefault));

        talk.press(&ChipReply::LevelsKeep);
        assert!(
            has_chip(&talk, &ChipReply::RuleCreate),
            "the trial of what the files of the folder share: {:?}",
            talk.labels()
        );
        talk.press(&ChipReply::RuleCreate);

        let rule = the_only_rule(&talk);
        assert_eq!(rule.spec.filing_pattern, "Alfa/{year}/{date}");
        assert_eq!(
            rule.spec.conditions.list,
            vec![Condition::new(ConditionKind::Sender, "Alfa AG")]
        );
    }

    #[test]
    fn the_first_levels_do_not_fit_and_the_second_answer_does() {
        let mut talk = archive_talk();
        talk.model.push_levels(Ok(levels("{year}/{sender}/{date}")));
        talk.model.push_levels(Ok(levels("{year}/{date}")));

        open_area(&mut talk, "Alfa");

        assert_eq!(talk.model.recognized().len(), 2, "asked once more");
        let card = levels_card(&talk).expect("a card of levels");
        assert_eq!(card.footer, None, "no deviations");
        assert_eq!(talk.last_parts().len(), 1);
        assert!(has_chip(&talk, &ChipReply::LevelsKeep));
    }

    #[test]
    fn the_depth_is_forced_only_when_all_files_of_the_folder_have_it() {
        use ncrs_ai::ScriptedChatModel;
        use ncrs_hermes_chat::RuleAsk;
        use ncrs_index::DocumentFields;

        let file = |path: &str| (path.to_owned(), DocumentFields::default());
        let sample = vec![file("2020/a.pdf"), file("2021/b.pdf")];
        let asked_depths = |all: Vec<(String, DocumentFields)>| {
            let model = ScriptedChatModel::new(ncrs_ai::LocalModelStatus::NotLoadedYet, []);
            model.push_levels(Ok(levels("{year}/")));
            let ask = RuleAsk::Recognize {
                files: sample.clone(),
                all,
                kinds: ncrs_index::Kinds::default(),
            };
            ask.run(&model);
            model.recognized_depths().first().copied()
        };

        let mixed = vec![file("2020/a.pdf"), file("2021/x/b.pdf")];
        assert_eq!(asked_depths(mixed), Some(None));
        let uniform = vec![file("2020/a.pdf"), file("2021/b.pdf")];
        assert_eq!(asked_depths(uniform), Some(Some(1)));
    }

    #[test]
    fn levels_that_still_do_not_fit_offer_only_the_change() {
        let mut talk = archive_talk();
        talk.model.push_levels(Ok(levels("{year}/{sender}/{date}")));
        talk.model.push_levels(Ok(levels("{year}/{kind}/{date}")));

        open_area(&mut talk, "Alfa");

        assert_eq!(talk.model.recognized().len(), 2);
        let texts = talk.last_texts();
        assert_eq!(
            texts[0],
            say(
                Msg::HermesChatLevelsPoor,
                &[("fitting", "0"), ("total", "4")]
            )
        );
        assert!(!has_chip(&talk, &ChipReply::LevelsKeep));
        assert!(!has_chip(&talk, &ChipReply::LevelsDefault));
        assert!(has_chip(&talk, &ChipReply::LevelsChange));
        assert_eq!(talk.rule_count(), 0);
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

        assert!(!has_chip(&talk, &ChipReply::RuleCreate), "no dead button");
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
        talk.model.push_revision(Ok(Revision {
            levels: levels("{date}_{sender}_{kind}"),
            done: false,
        }));
        talk.type_text("leave out the year");
        talk.press(&ChipReply::LevelsApply);
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
        assert!(levels_card(&talk).is_none(), "the proposal has no card");
        assert!(
            talk.last_texts()[0].contains("Telefon / Jahr / Monat"),
            "{:?}",
            talk.last_texts()
        );
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
        assert!(!has_chip(&talk, &ChipReply::RuleCreate), "no dead button");
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

    #[test]
    fn the_proposal_has_the_target_in_words_and_no_levels_card() {
        let mut talk = archive_talk();
        propose_for_telefon(&mut talk);

        assert!(levels_card(&talk).is_none());
        let texts = talk.last_texts();
        assert!(!texts[0].contains('{'), "{texts:?}");
        assert!(
            texts[0].contains("Telefon / Jahr · Dateiname:"),
            "{texts:?}"
        );
    }

    #[test]
    fn the_trial_says_how_many_more_would_move() {
        let mut talk = archive_talk();
        for (month, day) in [(1, 10), (3, 11), (6, 12), (9, 13)] {
            let id = talk.index_inbox_document(
                &format!("inbox/2020-{month:02}-{day}.pdf"),
                &letter(
                    "Alfa AG",
                    &format!("{day}.{month:02}.2020"),
                    "Rechnung",
                    day + 50,
                ),
            );
            talk.set_fields(
                id,
                &ncrs_index::DocumentFields {
                    sender: Some("Alfa AG".to_owned()),
                    kind: Some("invoice".to_owned()),
                    date: chrono::NaiveDate::from_ymd_opt(2020, month, day),
                    ..ncrs_index::DocumentFields::default()
                },
            );
        }
        propose_for_telefon(&mut talk);

        let texts = talk.last_texts();
        assert_eq!(texts.len(), 2, "{texts:?}");
        let lines: Vec<&str> = texts[1].lines().collect();
        assert_eq!(
            lines.len(),
            5,
            "3 examples, the rest, the archive: {lines:?}"
        );
        assert_eq!(
            lines[3],
            say(Msg::HermesChatRuleExamplesMore, &[("count", "5")])
        );
        assert_eq!(
            lines[4],
            say(
                Msg::HermesChatRuleTrialCounts,
                &[("right", "0"), ("moves", "8"), ("inbox", "4")]
            )
        );
        assert!(
            lines[0].contains("2020-01-10.pdf → Telefon/2020/") && lines[0].ends_with(".pdf"),
            "{lines:?}"
        );
    }

    #[test]
    fn documents_that_lie_right_are_counted_and_nothing_to_move_offers_no_filing() {
        let mut talk = archive_talk();
        talk.model.push_levels(Ok(levels("{year}/{date}")));
        open_area(&mut talk, "Alfa");
        talk.press(&ChipReply::LevelsKeep);
        talk.model.push_rule(Ok(spec("Rechnung", "Alfa/{year}")));
        talk.type_text("Rechnungen");

        let texts = talk.last_texts();
        assert_eq!(texts.len(), 2, "{texts:?}");
        assert_eq!(
            texts[1],
            say(
                Msg::HermesChatRuleTrialCounts,
                &[("right", "4"), ("moves", "0"), ("inbox", "0")]
            )
        );
        talk.press(&ChipReply::RuleCreate);
        assert!(!has_chip(&talk, &ChipReply::RuleNow));
    }

    #[test]
    fn the_levels_card_shows_different_values_from_the_files() {
        let mut talk = archive_talk();
        for (year, month) in [(2019, 2), (2021, 5)] {
            talk.index_document(
                &format!("archive/Alfa/{year}/{year}-{month:02}-01.pdf"),
                &letter("Alfa AG", &format!("01.{month:02}.{year}"), "Rechnung", 1),
            );
        }
        talk.model.push_levels(Ok(levels("{year}/{date}")));

        open_area(&mut talk, "Alfa");

        let card = levels_card(&talk).expect("a card of levels");
        let mut years: Vec<&str> = card.rows[0].text.split(", ").collect();
        years.sort_unstable();
        assert_eq!(years, ["2019", "2020", "2021"]);
        assert_eq!(
            card.rows[1].text.split(", ").count(),
            2,
            "{:?}",
            card.rows[1]
        );
    }

    #[test]
    fn the_empty_folder_card_shows_no_invented_values() {
        let mut talk = archive_talk();

        open_area(&mut talk, "Telefon");

        let card = levels_card(&talk).expect("a card of levels");
        for row in &card.rows {
            assert_ne!(row.text, "–", "{row:?}");
            assert!(!row.text.contains("Telekom"), "{row:?}");
            assert!(!row.text.contains("invoice"), "{row:?}");
        }
    }

    #[test]
    fn opening_the_chat_during_a_read_keeps_the_rule_session() {
        let mut talk = archive_talk();
        talk.model.push_levels(Ok(levels("{year}/{date}")));
        let folder = talk.path("archive");
        let (_task, _action) = ncrs_hermes_chat::update(
            &mut talk.state,
            Message::ChangeArea {
                folder,
                area: "Alfa".to_owned(),
            },
            &talk.ctx,
        );
        assert!(talk.state.pending_work().is_some(), "the read is running");

        talk.send(Message::Open);

        assert!(levels_card(&talk).is_some(), "{:?}", talk.last_texts());
        assert!(talk.state.session.rule.is_some());
    }

    #[test]
    fn without_the_ai_the_rule_question_is_not_asked() {
        let mut talk = archive_talk();
        open_area(&mut talk, "Telefon");
        talk.press(&ChipReply::LevelsKeep);
        talk.ctx.ai_on = false;

        talk.type_text("Rechnungen kommen hierhin");

        assert!(has_chip(&talk, &ChipReply::TurnOnAi));
        assert!(talk.model.described().is_empty());
        assert!(talk.state.session.rule.is_none());
        assert_eq!(talk.rule_count(), 0);
    }

    #[test]
    fn a_folder_with_files_and_no_ai_does_not_start_a_rule() {
        let mut talk = archive_talk();
        talk.ctx.ai_on = false;

        open_area(&mut talk, "Alfa");

        assert!(has_chip(&talk, &ChipReply::TurnOnAi));
        assert!(talk.state.session.rule.is_none());
        assert_eq!(talk.rule_count(), 0);
    }

    #[test]
    fn opening_a_folder_outside_the_targets_by_typing_only_reads_it() {
        let mut talk = archive_talk();
        talk.write(
            "home/Documents/Taxes/a.pdf",
            &letter("Alfa AG", "01.01.2020", "Rechnung", 1),
        );
        let before = talk.files();
        talk.send(Message::Open);

        talk.type_text_understood(
            "open ~/Documents/Taxes",
            r#"{"intent":"open_folder","folder":"~/Documents/Taxes"}"#,
        );

        assert!(!has_chip(&talk, &ChipReply::ConfirmTidy));
        assert_eq!(talk.files(), before);
        assert!(talk.state.session.rule.is_none());
    }

    #[test]
    fn dropping_every_level_stores_the_folder_with_one_slash() {
        let mut talk = archive_talk();
        open_area(&mut talk, "Telefon");
        talk.press(&ChipReply::LevelsChange);
        talk.model.push_revision(Ok(Revision {
            levels: Levels {
                folders: Vec::new(),
                file: None,
            },
            done: false,
        }));
        talk.type_text("no levels at all");
        talk.press(&ChipReply::LevelsApply);
        talk.model.push_rule(Ok(spec("Rechnung", "Telefon/{year}")));
        talk.type_text("Rechnungen");

        talk.press(&ChipReply::RuleCreate);

        assert_eq!(the_only_rule(&talk).spec.filing_pattern, "Telefon/");
    }

    #[test]
    fn a_nested_folder_does_not_repeat_its_own_path_in_the_levels() {
        let mut talk = archive_talk();
        talk.model.push_levels(Ok(levels("{date}")));
        talk.send(Message::Open);
        talk.type_text_understood(
            "öffne mal /Alfa/2020",
            r#"{"intent":"open_folder","folder":"/Alfa/2020"}"#,
        );
        talk.press(&ChipReply::LevelsKeep);
        talk.model
            .push_rule(Ok(spec("Rechnung", "Alfa/2020/{date}")));
        talk.type_text("Rechnungen");

        talk.press(&ChipReply::RuleCreate);

        assert_eq!(the_only_rule(&talk).spec.filing_pattern, "Alfa/2020/{date}");
    }
}
