//! The opening question: folders where new files arrive are tidied, folders
//! where documents are kept are read for their order, any other folder is
//! asked about once.

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
    use ncrs_hermes_chat::{Action, ChipReply, Entry, Message, Part};
    use ncrs_i18n::Msg;
    use ncrs_ui_kit::hermes_state::FolderRole;

    fn talk() -> Talk {
        let talk = Talk::in_dir(tempfile::tempdir().unwrap(), &["archive"], &["inbox"]);
        for folder in ["archive", "papers"] {
            for (month, day) in [(1, 10), (3, 11), (6, 12)] {
                talk.write(
                    &format!("{folder}/Alfa/2020/2020-{month:02}-{day} Alfa Rechnung.pdf"),
                    &letter(
                        "Alfa AG",
                        &format!("{day}.{month:02}.2020"),
                        "Rechnung",
                        day,
                    ),
                );
            }
        }
        talk.write(
            "inbox/neu_alfa.pdf",
            &letter("Alfa AG", "05.03.2021", "Rechnung", 77),
        );
        talk
    }

    fn areas_cards(talk: &Talk) -> usize {
        talk.state
            .thread()
            .iter()
            .flat_map(|entry| match entry {
                Entry::Hermes { parts, .. } => parts.as_slice(),
                _ => &[],
            })
            .filter(|part| matches!(part, Part::Areas(_)))
            .count()
    }

    #[test]
    fn the_greeting_labels_what_each_folder_is_for() {
        let mut talk = talk();
        talk.open();
        let labels = talk.labels();
        let source = say(Msg::HermesChatGreetingSourceChip, &[("folder", "inbox")]);
        let target = say(Msg::HermesChatGreetingTargetChip, &[("folder", "archive")]);
        assert!(labels.contains(&source), "{labels:?}");
        assert!(labels.contains(&target), "{labels:?}");
    }

    #[test]
    fn a_source_starts_tidying_without_an_areas_card() {
        let mut talk = talk();
        talk.open();
        talk.press(&ChipReply::Folder(talk.path("inbox")));
        assert_eq!(areas_cards(&talk), 0);
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatAskOnce, &[("folder", "inbox")])]
        );
    }

    #[test]
    fn the_archive_shows_its_areas() {
        let mut talk = talk();
        talk.open();
        talk.press(&ChipReply::Folder(talk.path("archive")));
        assert_eq!(areas_cards(&talk), 1);
    }

    #[test]
    fn a_folder_without_a_role_asks_once_and_a_source_answer_tidies_it() {
        let mut talk = talk();
        talk.open();
        let papers = talk.path("papers");
        talk.send(Message::FolderPicked(Some(papers.clone())));
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatAskRole, &[("folder", "papers")])]
        );
        assert_eq!(areas_cards(&talk), 0);
        talk.press(&ChipReply::RoleSource);
        assert_eq!(
            talk.actions,
            vec![Action::SetFolderRole {
                folder: papers,
                role: FolderRole::Inbox,
            }]
        );
        assert_eq!(areas_cards(&talk), 0);
    }

    #[test]
    fn a_folder_without_a_role_answered_as_kept_here_shows_its_areas() {
        let mut talk = talk();
        talk.open();
        talk.send(Message::FolderPicked(Some(talk.path("papers"))));
        talk.press(&ChipReply::RoleTarget);
        assert_eq!(areas_cards(&talk), 1);
        assert!(talk.actions.is_empty());
    }

    fn photo_talk() -> Talk {
        let mut talk = talk();
        std::fs::write(talk.path("inbox/scan.png"), [0xff, 0xd8, 0xff, 0x00]).unwrap();
        talk.ctx.ocr_models_present = false;
        talk
    }

    #[test]
    fn photos_without_text_recognition_are_asked_about_and_can_be_skipped() {
        let mut talk = photo_talk();
        talk.open();
        talk.press(&ChipReply::Folder(talk.path("inbox")));
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatAskPhotos,
                &[("folder", "inbox"), ("count", "1")]
            )]
        );
        talk.press(&ChipReply::PhotosNo);
        let texts: Vec<String> = talk
            .state
            .thread()
            .iter()
            .filter_map(|entry| match entry {
                Entry::Hermes { parts, .. } => Some(parts.clone()),
                _ => None,
            })
            .flatten()
            .filter_map(|part| match part {
                Part::Text(text) => Some(text),
                _ => None,
            })
            .collect();
        assert!(texts.contains(&say(Msg::HermesChatPhotosSkipped, &[("count", "1")])));
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatAskOnce, &[("folder", "inbox")])]
        );
    }

    #[test]
    fn reading_the_photos_opens_the_models_and_goes_on_once_they_are_there() {
        let mut talk = photo_talk();
        talk.open();
        talk.press(&ChipReply::Folder(talk.path("inbox")));
        talk.press(&ChipReply::PhotosYes);
        assert_eq!(talk.actions, vec![Action::OpenOcrModels]);
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatPhotosModels, &[])]
        );

        talk.ctx.ocr_models_present = true;
        talk.press(&ChipReply::PhotosYes);
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatAskOnce, &[("folder", "inbox")])],
            "the photos are not asked about twice"
        );
    }
}
