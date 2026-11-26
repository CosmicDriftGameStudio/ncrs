//! Opening a folder of the archive and recognising its order: the overview,
//! the levels, and the click that hands them to the rule suggestions. The
//! chat saves and moves nothing.

mod common;

#[cfg(test)]
mod tests {
    use crate::common::{count, letter, say, Talk};
    use ncrs_ai::{Levels, Revision};
    use ncrs_hermes_chat::{Action, ChipReply, Message, Part};
    use ncrs_i18n::Msg;

    const DEFAULT_PATTERN: &str = "{year}/{date}_{sender}_{kind}";

    fn levels(pattern: &str) -> Levels {
        Levels::from_pattern(pattern).unwrap()
    }

    /// `Alfa` holds four invoices filed by year; `Telefon` is empty.
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
        talk
    }

    fn open_area(talk: &mut Talk, area: &str) {
        let folder = talk.path("archive").join(area);
        talk.send(Message::FolderPicked(Some(folder)));
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

    /// Presses the one button that hands the order on, and checks that it
    /// comes out once, with the folder and the pattern.
    fn suggest(talk: &mut Talk, folder: &str, pattern: Option<&str>) {
        assert!(talk.suggestions().is_empty(), "{:?}", talk.actions);
        talk.press(&ChipReply::SuggestRules);
        assert_eq!(
            talk.suggestions(),
            vec![(talk.path(folder), pattern.map(str::to_owned))]
        );
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatSuggestionsOpen, &[])]
        );
        assert!(talk.state.chips().is_empty(), "{:?}", talk.labels());
        assert!(talk.state.session.rule.is_none());
        talk.assert_saves_no_rule();
    }

    #[test]
    fn an_empty_folder_gets_the_default_levels_and_one_button_after_them() {
        let mut talk = archive_talk();
        let before = talk.files();

        open_area(&mut talk, "Telefon");
        assert_eq!(
            talk.last_texts()[0],
            say(Msg::HermesChatFolderEmpty, &[("folder", "Telefon")])
        );
        let card = levels_card(&talk).expect("a card of levels");
        assert_eq!(card.rows.len(), 2, "year and the file name");
        talk.press(&ChipReply::LevelsKeep);

        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatLevelsConfirmed,
                &[("folder", "Telefon")]
            )]
        );
        assert_eq!(
            talk.labels(),
            vec![say(Msg::HermesChatSuggestRules, &[])],
            "one way on"
        );
        suggest(&mut talk, "archive/Telefon", Some(DEFAULT_PATTERN));
        assert_eq!(talk.files(), before);
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
        suggest(&mut talk, "archive/Alfa", Some("{year}/{date}"));
    }

    #[test]
    fn the_whole_target_shows_its_areas_before_the_levels() {
        let mut talk = archive_talk();
        talk.model.push_levels(Ok(levels("Alfa/{year}/{date}")));
        talk.send(Message::Open);

        talk.press(&ChipReply::Folder(talk.path("archive")));

        let overview = talk.state.thread().iter().any(|entry| {
            matches!(entry, ncrs_hermes_chat::Entry::Hermes { parts, .. }
                if parts.iter().any(|part| matches!(part, Part::Areas(_))))
        });
        assert!(overview, "the card of the areas");
        assert!(levels_card(&talk).is_some(), "{:?}", talk.last_texts());
        talk.press(&ChipReply::LevelsKeep);
        assert!(has_chip(&talk, &ChipReply::SuggestRules));
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
    fn levels_that_still_do_not_fit_offer_the_change_and_no_pattern() {
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
        suggest(&mut talk, "archive/Alfa", None);
    }

    #[test]
    fn files_that_deviate_are_shown_and_the_default_can_be_taken() {
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
        talk.press(&ChipReply::LevelsDefault);
        let default_card = levels_card(&talk).expect("the default levels");
        assert_eq!(default_card.rows.len(), 2);
        talk.press(&ChipReply::LevelsApply);
        suggest(&mut talk, "archive/Alfa", Some(DEFAULT_PATTERN));
    }

    #[test]
    fn leaving_a_level_out_changes_the_pattern() {
        let mut talk = archive_talk();
        open_area(&mut talk, "Telefon");
        talk.press(&ChipReply::LevelsChange);
        talk.model.push_revision(Ok(Revision {
            levels: levels("{date}_{sender}_{kind}"),
            done: false,
        }));
        talk.type_text("leave out the year");
        talk.press(&ChipReply::LevelsApply);

        suggest(&mut talk, "archive/Telefon", Some("{date}_{sender}_{kind}"));
    }

    #[test]
    fn dropping_every_level_hands_on_no_pattern() {
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

        suggest(&mut talk, "archive/Telefon", None);
    }

    #[test]
    fn a_sentence_after_the_levels_drops_the_confirmed_order() {
        let mut talk = archive_talk();
        open_area(&mut talk, "Telefon");
        talk.press(&ChipReply::LevelsKeep);

        talk.type_text_understood("Rechnung", r#"{"intent":"search","text":"Rechnung"}"#);

        assert!(!has_chip(&talk, &ChipReply::SuggestRules), "no dead button");
        assert!(talk.state.session.rule.is_none());
        assert!(talk.suggestions().is_empty());
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
        talk.press(&ChipReply::LevelsKeep);
        suggest(&mut talk, "archive/Alfa", Some("{year}/{date}"));
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
    fn opening_the_chat_again_drops_an_old_recognition() {
        let mut talk = archive_talk();
        open_area(&mut talk, "Telefon");
        assert!(talk.state.session.rule.is_some());

        talk.send(Message::Open);

        assert!(talk.state.session.rule.is_none());
        assert!(!has_chip(&talk, &ChipReply::LevelsKeep), "no dead button");
        talk.type_text_understood("Rechnung", r#"{"intent":"search","text":"Rechnung"}"#);
        assert!(
            talk.state.session.rule.is_none(),
            "the sentence went to the router"
        );
    }

    #[test]
    fn a_folder_outside_the_targets_is_looked_at_the_same_way() {
        let mut talk = archive_talk();
        talk.write(
            "inbox/a.pdf",
            &letter("Alfa AG", "01.01.2020", "Rechnung", 1),
        );
        talk.model.push_levels(Ok(levels("{kind}")));
        talk.model.push_levels(Ok(levels("{kind}")));
        talk.send(Message::Open);

        talk.send(Message::FolderPicked(Some(talk.path("inbox"))));

        assert!(levels_card(&talk).is_some(), "{:?}", talk.last_texts());
        suggest(&mut talk, "inbox", None);
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
    fn opening_the_chat_during_a_read_keeps_the_recognition() {
        let mut talk = archive_talk();
        talk.model.push_levels(Ok(levels("{year}/{date}")));
        let folder = talk.path("archive/Alfa");
        let (_task, _action) = ncrs_hermes_chat::update(
            &mut talk.state,
            Message::FolderPicked(Some(folder)),
            &talk.ctx,
        );
        assert!(talk.state.pending_work().is_some(), "the read is running");

        talk.send(Message::Open);

        assert!(levels_card(&talk).is_some(), "{:?}", talk.last_texts());
        assert!(talk.state.session.rule.is_some());
    }

    #[test]
    fn an_empty_folder_needs_no_ai() {
        let mut talk = archive_talk();
        talk.ctx.ai_on = false;

        open_area(&mut talk, "Telefon");
        talk.press(&ChipReply::LevelsKeep);

        suggest(&mut talk, "archive/Telefon", Some(DEFAULT_PATTERN));
    }

    #[test]
    fn a_folder_with_files_and_no_ai_starts_no_recognition() {
        let mut talk = archive_talk();
        talk.ctx.ai_on = false;

        open_area(&mut talk, "Alfa");

        assert!(has_chip(&talk, &ChipReply::TurnOnAi));
        assert!(talk.state.session.rule.is_none());
    }

    #[test]
    fn a_nested_folder_hands_on_the_pattern_below_itself() {
        let mut talk = archive_talk();
        talk.model.push_levels(Ok(levels("{date}")));
        talk.send(Message::Open);
        talk.type_text_understood(
            "öffne mal /Alfa/2020",
            r#"{"intent":"open_folder","folder":"/Alfa/2020"}"#,
        );
        talk.press(&ChipReply::LevelsKeep);

        suggest(&mut talk, "archive/Alfa/2020", Some("{date}"));
    }

    #[test]
    fn every_way_to_take_the_levels_ends_in_one_suggestion() {
        for way in [
            ChipReply::LevelsKeep,
            ChipReply::LevelsApply,
            ChipReply::LevelsDone,
        ] {
            let mut talk = archive_talk();
            talk.model.push_levels(Ok(levels("{year}/{date}")));
            open_area(&mut talk, "Alfa");
            let expected = if way == ChipReply::LevelsKeep {
                "{year}/{date}"
            } else {
                talk.press(&ChipReply::LevelsDefault);
                DEFAULT_PATTERN
            };

            talk.press(&way);

            suggest(&mut talk, "archive/Alfa", Some(expected));
        }
    }

    /// Every sentence and button of the chat, one conversation: the files
    /// stay, and no rule, folder or run comes out of it.
    #[test]
    fn no_way_through_the_chat_changes_a_file() {
        let mut talk = archive_talk();
        talk.write(
            "inbox/a.pdf",
            &letter("Alfa AG", "01.01.2020", "Rechnung", 1),
        );
        let before = talk.files();
        talk.send(Message::Open);

        for reply in [
            r#"{"intent":"tidy_folder","source":"inbox"}"#,
            r#"{"intent":"tidy_folder","source":"inbox","target":"archive"}"#,
            r#"{"intent":"bulk_move","target":"Alt"}"#,
            r#"{"intent":"change_structure","area":"*","order":"by_month"}"#,
            r#"{"intent":"undo"}"#,
            r#"{"intent":"leave_as_is"}"#,
            r#"{"intent":"other"}"#,
        ] {
            talk.type_text_understood("ein Satz", reply);
        }
        for intent in [
            ChipReply::IntentTidy,
            ChipReply::IntentStructure,
            ChipReply::IntentSearch,
            ChipReply::IntentUndo,
        ] {
            talk.type_text_understood("ein Satz", r#"{"intent":"other"}"#);
            talk.press(&intent);
        }
        talk.model.push_levels(Ok(levels("{year}/{date}")));
        open_area(&mut talk, "Alfa");
        talk.press(&ChipReply::LevelsKeep);
        talk.press(&ChipReply::SuggestRules);

        assert_eq!(talk.files(), before);
        talk.assert_saves_no_rule();
        for action in &talk.actions {
            assert!(
                matches!(action, Action::SuggestRules { .. } | Action::OpenSearch),
                "{action:?}"
            );
        }
    }
}
