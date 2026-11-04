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
    use ncrs_hermes_chat::{ChipReply, Entry, Part};
    use ncrs_i18n::Msg;

    const EVERYWHERE_BY_YEAR: &str =
        r#"{"intent":"change_structure","area":"*","order":"by_year"}"#;
    const ALFA_NAMES: &str = r#"{"intent":"change_structure","area":"Alfa","name":["year","month","day"],"separator":"_"}"#;
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

    fn read_archive(talk: &mut Talk) {
        talk.open();
        talk.press(&ChipReply::Folder(talk.path("archive")));
        talk.press(&ChipReply::WantChange);
    }

    fn has_chip(talk: &Talk, reply: &ChipReply) -> bool {
        talk.state.chips().iter().any(|chip| &chip.reply == reply)
    }

    #[test]
    fn every_area_by_year_previews_a_pattern_and_applies_it() {
        let mut talk = archive_talk();
        read_archive(&mut talk);

        talk.type_text_understood("überall nach jahr", EVERYWHERE_BY_YEAR);

        let cards = talk
            .last_parts()
            .iter()
            .filter(|part| matches!(part, Part::File(_)))
            .count();
        assert_eq!(cards, 2, "{:?}", talk.last_parts());
        assert!(
            talk.state.session.changes.is_empty(),
            "nothing before the yes"
        );
        talk.press(&ChipReply::ConfirmChange);
        assert_eq!(talk.state.session.changes.len(), 1);
    }

    #[test]
    fn a_name_from_the_date_renames_on_filing_and_undo_brings_the_old_name_back() {
        let mut talk = archive_talk();
        let before = talk.files();
        read_archive(&mut talk);

        talk.type_text_understood("in alfa dateinamen mit datum", ALFA_NAMES);
        assert_eq!(
            talk.last_texts()[0],
            say(Msg::HermesChatChangePreviewNames, &[("area", "Alfa")])
        );
        for reply in [ChipReply::ConfirmChange, ChipReply::ScopeAll] {
            if has_chip(&talk, &reply) {
                talk.press(&reply);
                break;
            }
        }
        assert_eq!(talk.state.session.changes.len(), 1);

        talk.type_text_understood(
            "Räum Inbox auf",
            r#"{"intent":"tidy_folder","source":"Inbox"}"#,
        );
        talk.press(&ChipReply::ConfirmTidy);
        talk.choose_archive_if_asked();
        talk.press(&ChipReply::Once);
        talk.press(&ChipReply::ModeMove);
        talk.leave_stay_questions();
        talk.press(&ChipReply::FileAll);
        let filed = talk.files();
        assert!(
            filed.keys().any(|path| path.starts_with("archive/Alfa/")
                && path.ends_with("2021_03_05_Alfa Rechnung.pdf")),
            "{:?}",
            filed
                .keys()
                .filter(|path| path.contains("2021"))
                .collect::<Vec<_>>()
        );

        assert!(
            filed.contains_key("archive/Alfa/2020/2020-01-10 Alfa Rechnung.pdf"),
            "documents already in the archive keep their names: only what is filed gets the name"
        );

        talk.type_text_understood("mach das rückgängig", r#"{"intent":"undo"}"#);
        talk.press(&ChipReply::PutBack);
        assert_eq!(talk.files(), before);
    }

    #[test]
    fn the_next_sentence_is_shown_the_previous_change() {
        let mut talk = archive_talk();
        read_archive(&mut talk);
        talk.type_text_understood("überall nach jahr", EVERYWHERE_BY_YEAR);

        talk.type_text_understood("dann erstmal nur alfa", SMALL_TALK);

        let turns = talk.model.turns();
        assert_eq!(turns[0].previous_request, None);
        let previous = turns[1].previous_request.clone().unwrap();
        assert!(
            previous.contains(r#""area":"*""#) && previous.contains(r#""order":"by_year""#),
            "{previous}"
        );
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
        let follow_up_area = talk
            .state
            .chips()
            .iter()
            .any(|chip| matches!(chip.reply, ChipReply::FollowUpArea(_)));
        assert!(!follow_up_area);
        assert!(has_chip(&talk, &ChipReply::Cancel));

        talk.type_text_understood("und wieder hmmm", other);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatFallback, &[])]);
    }

    #[test]
    fn small_talk_after_a_confirmed_change_is_no_misread_follow_up() {
        let mut talk = archive_talk();
        read_archive(&mut talk);
        talk.type_text_understood("überall nach jahr", EVERYWHERE_BY_YEAR);
        talk.press(&ChipReply::ConfirmChange);
        assert_eq!(talk.state.session.changes.len(), 1);

        talk.type_text_understood("danke", SMALL_TALK);

        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatSmallTalk, &[])]);
    }

    #[test]
    fn small_talk_after_a_change_asks_again_with_the_areas() {
        let mut talk = archive_talk();
        read_archive(&mut talk);
        talk.type_text_understood("überall nach jahr", EVERYWHERE_BY_YEAR);

        talk.type_text_understood("dann erstmal nur alfa", SMALL_TALK);

        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatNotUnderstoodFollowUp, &[])]
        );
        let areas: Vec<_> = talk
            .state
            .chips()
            .iter()
            .filter_map(|chip| match &chip.reply {
                ChipReply::FollowUpArea(area) => Some(area.clone()),
                _ => None,
            })
            .collect();
        assert!(areas.contains(&"Alfa".to_owned()), "{areas:?}");
        assert!(has_chip(&talk, &ChipReply::Cancel));
        assert!(matches!(
            talk.state.thread().last(),
            Some(Entry::Hermes { .. })
        ));
    }
}
