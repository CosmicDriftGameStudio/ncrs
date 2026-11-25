//! The opening sentence: the targets as buttons and the picker; a folder that
//! is pressed or picked is looked at for its order, whatever it is used for.

mod common;

#[cfg(test)]
mod tests {
    use crate::common::{letter, say, Talk};
    use ncrs_ai::Levels;
    use ncrs_hermes_chat::{Action, ChipReply, Entry, Message, Part};
    use ncrs_i18n::Msg;

    fn talk_with(targets: &[&str]) -> Talk {
        let talk = Talk::in_dir(tempfile::tempdir().unwrap(), targets, &["inbox"]);
        for folder in ["archive", "papers"] {
            for (month, day) in [(1, 10), (3, 11), (6, 12)] {
                talk.write(
                    &format!("{folder}/Alfa/2020/2020-{month:02}-{day}.pdf"),
                    &letter(
                        "Alfa AG",
                        &format!("{day}.{month:02}.2020"),
                        "Rechnung",
                        day,
                    ),
                );
            }
        }
        std::fs::create_dir_all(talk.path("inbox")).unwrap();
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

    fn shows_levels(talk: &Talk) -> bool {
        talk.last_parts()
            .iter()
            .any(|part| matches!(part, Part::Levels(_)))
    }

    #[test]
    fn the_greeting_offers_every_target_by_its_name_and_the_picker() {
        let mut talk = talk_with(&["archive", "papers"]);

        talk.open();

        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatGreeting, &[])]);
        assert_eq!(
            talk.labels(),
            vec![
                "archive".to_owned(),
                "papers".to_owned(),
                say(Msg::HermesChatChooseFolder, &[])
            ],
            "no source, and no question what a folder is for"
        );
        let replies: Vec<_> = talk
            .state
            .chips()
            .iter()
            .map(|chip| chip.reply.clone())
            .collect();
        assert_eq!(
            replies,
            vec![
                ChipReply::Folder(talk.path("archive")),
                ChipReply::Folder(talk.path("papers")),
                ChipReply::PickFolder
            ]
        );
    }

    #[test]
    fn without_a_target_the_picker_is_the_main_button() {
        let mut talk = talk_with(&[]);

        talk.open();

        assert_eq!(
            talk.labels(),
            vec![say(Msg::HermesChatChooseFolder, &[])]
        );
        assert!(talk.state.chips()[0].primary);
        talk.press(&ChipReply::PickFolder);
        assert_eq!(talk.actions, vec![Action::PickFolder]);
    }

    #[test]
    fn a_target_shows_its_areas_and_then_its_levels() {
        let mut talk = talk_with(&["archive"]);
        talk.model
            .push_levels(Ok(Levels::from_pattern("Alfa/{year}/{date}").unwrap()));
        talk.open();

        talk.press(&ChipReply::Folder(talk.path("archive")));

        assert_eq!(areas_cards(&talk), 1);
        assert!(shows_levels(&talk), "{:?}", talk.last_parts());
        assert!(talk.actions.is_empty(), "{:?}", talk.actions);
    }

    #[test]
    fn any_picked_folder_is_looked_at_without_a_question() {
        let mut talk = talk_with(&["archive"]);
        talk.model
            .push_levels(Ok(Levels::from_pattern("Alfa/{year}/{date}").unwrap()));
        talk.open();

        talk.send(Message::FolderPicked(Some(talk.path("papers"))));

        assert_eq!(areas_cards(&talk), 1);
        assert!(shows_levels(&talk), "{:?}", talk.last_parts());
        assert!(talk.actions.is_empty(), "{:?}", talk.actions);
    }

    #[test]
    fn a_picker_that_is_closed_says_so_and_reads_nothing() {
        let mut talk = talk_with(&["archive"]);
        talk.open();

        talk.send(Message::FolderPicked(None));

        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatLaterReply, &[])]);
        assert!(talk.works.is_empty(), "{:?}", talk.works);
    }
}
