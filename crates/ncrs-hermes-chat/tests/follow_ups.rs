//! What the playthrough with the real model showed: offers that lead
//! nowhere, dead ends and questions that were never asked.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

#[cfg(test)]
mod tests {

    use crate::common::{letter, say, Talk};
    use ncrs_hermes_chat::{Action, ChipReply, Entry, Part};
    use ncrs_i18n::Msg;
    use ncrs_intake::test_archive::test_archive;

    const TIDY_INBOX: &str = r#"{"intent":"tidy_folder","source":"Inbox"}"#;
    const BANK_BY_MONTH: &str =
        r#"{"intent":"change_structure","area":"Bankstatements","order":"by_month"}"#;
    const KIND_WITHOUT_FILES: &str =
        r#"{"intent":"change_structure","area":"Vertrag","order":"by_month"}"#;

    fn archive_talk() -> Talk {
        let dir = tempfile::tempdir().unwrap();
        test_archive().write_to(dir.path()).unwrap();
        Talk::in_dir(dir, &["archive"], &["inbox"])
    }

    fn small_talk() -> Talk {
        let talk = Talk::in_dir(tempfile::tempdir().unwrap(), &["archive"], &["inbox"]);
        for (month, day) in [(1, 10), (3, 11), (6, 12)] {
            talk.write(
                &format!("archive/Alfa/2020/2020-{month:02}-{day} Alfa Rechnung.pdf"),
                &letter(
                    "Alfa AG",
                    &format!("{day}.{month:02}.2020"),
                    "Rechnung",
                    day,
                ),
            );
        }
        talk.write(
            "inbox/neu.pdf",
            &letter("Alfa AG", "05.03.2021", "Rechnung", 77),
        );
        talk
    }

    fn replies(talk: &Talk) -> Vec<ChipReply> {
        talk.state
            .chips()
            .iter()
            .map(|chip| chip.reply.clone())
            .collect()
    }

    fn has_file_card(talk: &Talk) -> bool {
        talk.last_parts()
            .iter()
            .any(|part| matches!(part, Part::File(_)))
    }

    fn area_chips(talk: &Talk) -> Vec<String> {
        replies(talk)
            .into_iter()
            .filter_map(|reply| match reply {
                ChipReply::ChangeArea(area) => Some(area),
                _ => None,
            })
            .collect()
    }

    fn all_texts(talk: &Talk) -> Vec<String> {
        talk.state
            .thread()
            .iter()
            .filter_map(|entry| match entry {
                Entry::Hermes { parts, .. } => Some(parts),
                _ => None,
            })
            .flatten()
            .filter_map(|part| match part {
                Part::Text(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_pattern_that_would_move_nothing_is_not_offered() {
        let mut talk = archive_talk();
        talk.open();
        talk.press(&ChipReply::Folder(talk.path("archive")));
        for reply in replies(&talk) {
            if matches!(reply, ChipReply::AdoptPattern(_)) {
                talk.press(&reply);
                assert!(
                    replies(&talk).contains(&ChipReply::ConfirmChange),
                    "{:?}",
                    talk.last_texts()
                );
                return;
            }
        }
        // Every folder of a small archive follows the pattern already.
        let mut small = small_talk();
        small.open();
        small.press(&ChipReply::Folder(small.path("archive")));
        assert!(
            !replies(&small)
                .iter()
                .any(|reply| matches!(reply, ChipReply::AdoptPattern(_))),
            "{:?}",
            small.labels()
        );
    }

    #[test]
    fn an_area_word_matches_a_folder_it_contains_without_regard_to_case() {
        let mut talk = archive_talk();
        talk.open();
        talk.press(&ChipReply::Folder(talk.path("archive")));
        talk.press(&ChipReply::WantChange);
        talk.type_text_understood("Kontoauszüge nach Monat", BANK_BY_MONTH);
        assert!(has_file_card(&talk), "{:?}", talk.last_parts());
    }

    #[test]
    fn a_word_no_folder_fits_asks_which_area_and_keeps_the_order() {
        let mut talk = archive_talk();
        talk.open();
        talk.press(&ChipReply::Folder(talk.path("archive")));
        talk.press(&ChipReply::WantChange);
        talk.type_text_understood("mach es nach Monat", KIND_WITHOUT_FILES);
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatChangeAreaAsk, &[])]
        );
        let areas = area_chips(&talk);
        assert!(areas.contains(&"Bank".to_owned()), "{areas:?}");
        assert!(replies(&talk).contains(&ChipReply::PickFolder));

        talk.press(&ChipReply::ChangeArea("Bank".to_owned()));
        assert!(has_file_card(&talk), "{:?}", talk.last_parts());
        assert!(area_chips(&talk).is_empty());
    }

    #[test]
    fn a_word_that_is_no_area_and_no_kind_says_a_whole_folder_cannot_be_proposed() {
        let mut talk = archive_talk();
        talk.open();
        talk.press(&ChipReply::Folder(talk.path("archive")));
        talk.press(&ChipReply::WantChange);
        talk.type_text_understood(
            "mach einen Vorschlag",
            r#"{"intent":"change_structure","area":"Zzzzqq","order":"by_month"}"#,
        );
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatNotYetPropose, &[])]
        );
        assert!(replies(&talk).contains(&ChipReply::IntentSearch));
    }

    #[test]
    fn a_reading_says_how_far_the_ai_is_while_documents_wait_for_it() {
        let mut talk = archive_talk();
        talk.open();
        {
            let mut slot = talk.index.lock().unwrap();
            let index = slot.as_mut().unwrap();
            let document = ncrs_index::NewDocument {
                hash: ncrs_index::ContentHash([9; 32]),
                path: talk.path("archive").join("waiting.pdf"),
                source: ncrs_index::DocumentSource::Archive,
                received_at: chrono::Utc::now(),
                stamp: None,
            };
            let id = index.insert_document(&document).unwrap();
            index.set_document_text(id, "Rechnung").unwrap();
        }
        talk.press(&ChipReply::Folder(talk.path("archive")));
        assert!(
            talk.last_texts().contains(&say(
                Msg::HermesChatReadingAiProgress,
                &[("done", "0"), ("total", "1")]
            )),
            "{:?}",
            talk.last_texts()
        );
    }

    #[test]
    fn a_typed_path_of_an_existing_folder_is_taken_as_a_folder_not_as_a_sentence() {
        let mut talk = archive_talk();
        talk.open();
        talk.press(&ChipReply::Folder(talk.path("archive")));
        std::fs::create_dir_all(talk.path("elsewhere")).unwrap();
        let typed = talk.path("elsewhere").to_string_lossy().into_owned();
        talk.type_text(&typed);
        let real = std::fs::canonicalize(talk.path("elsewhere"))
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatUnknownPathConfirm, &[("path", &real)])]
        );
    }

    #[test]
    fn a_new_unknown_word_asks_again_after_an_area_was_picked() {
        let mut talk = archive_talk();
        talk.open();
        talk.press(&ChipReply::Folder(talk.path("archive")));
        talk.press(&ChipReply::WantChange);
        talk.type_text_understood("mach es nach Monat", KIND_WITHOUT_FILES);
        talk.press(&ChipReply::ChangeArea("Bank".to_owned()));
        talk.press(&ChipReply::Cancel);
        talk.type_text_understood("mach es nach Monat", KIND_WITHOUT_FILES);
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatChangeAreaAsk, &[])]
        );
    }

    #[test]
    fn a_single_target_is_asked_for_too_so_new_folder_and_per_file_are_reachable() {
        let mut talk = small_talk();
        talk.type_text_understood("Räum Inbox auf", TIDY_INBOX);
        talk.press(&ChipReply::ConfirmTidy);
        let archive = ChipReply::Folder(talk.path("archive"));
        let chips = talk.state.chips().to_vec();
        assert_eq!(chips.len(), 3, "{:?}", talk.labels());
        assert_eq!(chips[0].reply, archive);
        assert!(chips[0].primary);
        assert_eq!(chips[1].reply, ChipReply::NewTarget);
        assert_eq!(chips[2].reply, ChipReply::DecidePerFile);
    }

    #[test]
    fn always_on_a_watched_inbox_says_what_happens_and_changes_no_setting() {
        let mut talk = small_talk();
        talk.type_text_understood("Räum Inbox auf", TIDY_INBOX);
        talk.press(&ChipReply::ConfirmTidy);
        talk.choose_archive_if_asked();
        talk.press(&ChipReply::Always);
        assert!(talk.actions.is_empty(), "{:?}", talk.actions);
        assert!(all_texts(&talk).contains(&say(Msg::HermesChatAlwaysSet, &[("folder", "inbox")])));
    }

    #[test]
    fn the_folder_to_tidy_is_shown_from_home_not_as_the_resolved_path() {
        let mut talk = small_talk();
        talk.write("home/Downloads/x.txt", "x");
        talk.type_text_understood(
            "Räum Downloads auf",
            r#"{"intent":"tidy_folder","source":"Downloads"}"#,
        );
        let said = talk.last_texts().join(" ");
        assert!(said.contains("~/Downloads"), "{said}");
        assert!(
            !said.contains(&talk.root.to_string_lossy().into_owned()),
            "{said}"
        );
    }

    #[test]
    fn structure_is_asked_for_only_on_filing_locations() {
        let mut talk = small_talk();
        talk.open();
        talk.type_text_understood("Kontoauszüge nach Monat", BANK_BY_MONTH);
        let folders: Vec<ChipReply> = replies(&talk)
            .into_iter()
            .filter(|reply| matches!(reply, ChipReply::Folder(_)))
            .collect();
        assert_eq!(folders, vec![ChipReply::Folder(talk.path("archive"))]);
        assert!(replies(&talk).contains(&ChipReply::PickFolder));
    }

    #[test]
    fn a_move_of_many_files_and_an_undo_without_a_run_offer_the_usual_ways_on() {
        let mut talk = small_talk();
        talk.open();
        talk.type_text_understood(
            "pack alle Rechnungen nach Alt",
            r#"{"intent":"bulk_move","target":"Alt"}"#,
        );
        assert!(
            replies(&talk).contains(&ChipReply::IntentSearch),
            "{:?}",
            talk.labels()
        );
        talk.type_text_understood("mach das rückgängig", r#"{"intent":"undo"}"#);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatUndoNone, &[])]);
        assert!(
            replies(&talk).contains(&ChipReply::IntentSearch),
            "{:?}",
            talk.labels()
        );
        assert!(!replies(&talk).contains(&ChipReply::IntentUndo));
        assert!(talk
            .actions
            .iter()
            .all(|action| *action != Action::OpenSearch));
    }

    #[test]
    fn small_talk_and_leaving_things_as_they_are_get_an_answer_and_the_usual_ways_on() {
        let mut talk = small_talk();
        talk.open();
        talk.type_text_understood("hallo", r#"{"intent":"smalltalk"}"#);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatSmallTalk, &[])]);
        assert!(replies(&talk).contains(&ChipReply::IntentSearch));
        talk.type_text_understood("lass alles so", r#"{"intent":"leave_as_is"}"#);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatLeaveAsIs, &[])]);
        assert!(replies(&talk).contains(&ChipReply::IntentTidy));
        talk.type_text_understood("asdf", r#"{"intent":"other"}"#);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatFallback, &[])]);
    }

    fn change_in_area(talk: &mut Talk, area: &str, order: &str) {
        talk.type_text_understood(
            "mach es anders",
            &format!(r#"{{"intent":"change_structure","area":"Vertrag","order":"{order}"}}"#),
        );
        talk.press(&ChipReply::ChangeArea(area.to_owned()));
    }

    #[test]
    fn an_area_that_has_the_asked_order_says_so_and_offers_the_usual_ways_on() {
        let mut talk = archive_talk();
        talk.open();
        talk.press(&ChipReply::Folder(talk.path("archive")));
        talk.press(&ChipReply::WantChange);
        change_in_area(&mut talk, "Versicherung", "by_sender");
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatChangeAlready,
                &[("area", "Versicherung")]
            )]
        );
        assert!(replies(&talk).contains(&ChipReply::IntentSearch));
        assert!(!has_file_card(&talk));
    }

    #[test]
    fn an_area_without_a_matching_branch_offers_the_areas_again() {
        let mut talk = archive_talk();
        talk.open();
        talk.press(&ChipReply::Folder(talk.path("archive")));
        talk.press(&ChipReply::WantChange);
        change_in_area(&mut talk, "Versicherung", "by_month");
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatChangeNotFound,
                &[("word", "Versicherung")]
            )]
        );
        assert!(area_chips(&talk).contains(&"Bank".to_owned()));
        talk.press(&ChipReply::ChangeArea("Bank".to_owned()));
        assert!(has_file_card(&talk), "{:?}", talk.last_parts());
    }

    #[test]
    fn a_statement_with_the_usual_ways_on_is_not_a_question_for_the_model() {
        let mut talk = small_talk();
        talk.open();
        talk.type_text_understood("hallo", r#"{"intent":"smalltalk"}"#);
        talk.type_text_understood("Kontoauszüge nach Monat", BANK_BY_MONTH);
        let turns = talk.model.turns();
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[1].question, None);

        talk.type_text_understood("hallo", r#"{"intent":"smalltalk"}"#);
        talk.press(&ChipReply::IntentSearch);
        assert!(talk.actions.contains(&Action::OpenSearch));
        assert_eq!(talk.model.turns().len(), 3);
    }
}
