//! What the playthrough with the real model showed: offers that lead
//! nowhere, dead ends and questions that were never asked.

mod common;

#[cfg(test)]
mod tests {

    use crate::common::{letter, say, Talk};
    use ncrs_ai::Levels;
    use ncrs_hermes_chat::{Action, ChipReply, Entry, Part};
    use ncrs_i18n::Msg;
    use ncrs_intake::test_archive::test_archive;

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
    fn a_reading_says_how_far_the_ai_is_while_documents_wait_for_it() {
        let mut talk = archive_talk();
        talk.open();
        let document = ncrs_index::NewDocument {
            hash: ncrs_index::ContentHash([9; 32]),
            path: talk.path("archive").join("waiting.pdf"),
            source: ncrs_index::DocumentSource::Archive,
            received_at: chrono::Utc::now(),
            stamp: None,
        };
        let id = talk
            .index
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .insert_document(&document)
            .unwrap();
        talk.index
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .set_document_text(id, "Rechnung")
            .unwrap();
        for _ in 0..2 {
            talk.model
                .push_levels(Ok(Levels::from_pattern("{year}/").unwrap()));
        }
        talk.press(&ChipReply::Folder(talk.path("archive")));
        let texts = all_texts(&talk);
        assert!(
            texts.contains(&say(
                Msg::HermesChatReadingAiProgress,
                &[("done", "0"), ("total", "1")]
            )),
            "{texts:?}"
        );
    }

    #[test]
    fn a_typed_path_of_an_existing_folder_is_taken_as_a_folder_not_as_a_sentence() {
        let mut talk = archive_talk();
        talk.open();
        // Past the greeting, no folder is asked for any more.
        talk.type_text_understood("hallo", r#"{"intent":"smalltalk"}"#);
        talk.press(&ChipReply::IntentTidy);
        std::fs::create_dir_all(talk.path("elsewhere")).unwrap();
        let typed = talk.path("elsewhere").to_string_lossy().into_owned();
        talk.type_text(&typed);
        let real = ncrs_intake::canonical_path(&talk.path("elsewhere"))
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let name = std::path::Path::new(&real)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatUnknownPathConfirm, &[("name", &name)])]
        );
    }

    #[test]
    fn a_move_of_many_files_and_an_undo_without_a_run_offer_the_usual_ways_on() {
        let mut talk = small_talk();
        talk.open();
        talk.type_text_understood(
            "pack alle Rechnungen nach Alt",
            r#"{"intent":"bulk_move","target":"Alt"}"#,
        );
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatSortHint, &[])]);
        assert_eq!(
            replies(&talk),
            vec![ChipReply::OpenSort(None), ChipReply::OpenRuleForm]
        );
        talk.type_text_understood("mach das rückgängig", r#"{"intent":"undo"}"#);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatUndoNone, &[])]);
        assert!(
            replies(&talk).contains(&ChipReply::IntentSearch),
            "{:?}",
            talk.labels()
        );
        assert!(!replies(&talk).contains(&ChipReply::IntentUndo));
        assert!(talk.actions.is_empty(), "{:?}", talk.actions);
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

    #[test]
    fn a_statement_with_the_usual_ways_on_is_not_a_question_for_the_model() {
        let mut talk = small_talk();
        talk.open();
        talk.type_text_understood("hallo", r#"{"intent":"smalltalk"}"#);
        talk.type_text_understood("danke", r#"{"intent":"smalltalk"}"#);
        let turns = talk.model.turns();
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[1].question, None);

        talk.type_text_understood("hallo", r#"{"intent":"smalltalk"}"#);
        talk.press(&ChipReply::IntentSearch);
        assert!(talk.actions.contains(&Action::OpenSearch));
        assert_eq!(talk.model.turns().len(), 3);
    }
}
