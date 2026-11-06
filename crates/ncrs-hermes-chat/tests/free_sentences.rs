//! Sentences typed freely: an order for every area, file names from parts of
//! the document, and a follow-up that only refines the sentence before.

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
    use ncrs_hermes_chat::ChipReply;
    use ncrs_i18n::Msg;

    const SMALL_TALK: &str = r#"{"intent":"smalltalk"}"#;

    /// Alfa is filed by year, Beta has a month folder below the year.
    fn archive_talk() -> Talk {
        let talk = Talk::in_dir(tempfile::tempdir().unwrap(), &["archive"], &["inbox"]);
        for (month, day) in [(1, 10), (3, 11), (6, 12), (9, 13), (11, 14)] {
            talk.write(
                &format!("archive/Alfa/2020/2020-{month:02}-{day} Alfa Rechnung.pdf"),
                &letter(
                    "Alfa AG",
                    &format!("{day}.{month:02}.2020"),
                    "Rechnung",
                    day,
                ),
            );
            talk.write(
                &format!("archive/Beta/2020/{month:02}/2020-{month:02}-{day} Beta Rechnung.pdf"),
                &letter(
                    "Beta GmbH",
                    &format!("{day}.{month:02}.2020"),
                    "Rechnung",
                    day + 50,
                ),
            );
        }
        talk.write(
            "inbox/neu_alfa.pdf",
            &letter("Alfa AG", "05.03.2021", "Rechnung", 77),
        );
        talk
    }

    fn has_chip(talk: &Talk, reply: &ChipReply) -> bool {
        talk.state.chips().iter().any(|chip| &chip.reply == reply)
    }

    #[test]
    fn two_small_talk_sentences_in_a_row_are_both_answered_as_small_talk() {
        let mut talk = archive_talk();
        talk.open();

        talk.type_text_understood("hallo", SMALL_TALK);
        talk.type_text_understood("danke", SMALL_TALK);

        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatSmallTalk, &[])]);
    }

    #[test]
    fn an_unusable_sentence_is_asked_about_once_and_the_next_gets_the_fallback() {
        let mut talk = archive_talk();
        talk.open();
        let other = r#"{"intent":"other"}"#;

        talk.type_text_understood("hmmm", other);
        talk.type_text_understood("noch mal hmmm", other);

        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatNotUnderstoodFollowUp, &[])]
        );
        assert!(has_chip(&talk, &ChipReply::Cancel));

        talk.type_text_understood("und wieder hmmm", other);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatFallback, &[])]);
    }

    #[test]
    fn small_talk_after_an_unusable_sentence_is_answered_as_small_talk() {
        let mut talk = archive_talk();
        talk.open();

        talk.type_text_understood("hmmm", r#"{"intent":"other"}"#);
        talk.type_text_understood("was kannst du?", SMALL_TALK);

        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatSmallTalk, &[])]);
    }

    #[test]
    fn a_statement_with_a_folder_button_is_sent_to_the_model_as_no_question() {
        let mut talk = archive_talk();
        talk.open();
        talk.type_text_understood(
            "Räum Nirgendwo auf",
            r#"{"intent":"tidy_folder","source":"Nirgendwo"}"#,
        );
        assert!(has_chip(&talk, &ChipReply::PickFolder));

        talk.type_text_understood("Downloads aufräumen", SMALL_TALK);

        let turns = talk.model.turns();
        assert!(turns[1].question.is_none(), "{:?}", turns[1].question);
    }

    #[test]
    fn a_typed_path_that_is_no_folder_in_a_step_without_folders_goes_to_the_model() {
        let mut talk = archive_talk();
        talk.open();

        talk.model.push_reply(Ok(SMALL_TALK));
        talk.type_text("/ hallo, was geht");

        assert!(talk.state.pending_resolve().is_none());
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatSmallTalk, &[])]);
    }
}
