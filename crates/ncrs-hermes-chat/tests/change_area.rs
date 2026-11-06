//! The settings page names one area: the chat reads its folder and takes it
//! up as a rule flow for exactly that area.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

#[cfg(test)]
mod tests {
    use crate::common::{letter, Talk};
    use ncrs_ai::Levels;
    use ncrs_hermes_chat::{ChipReply, Message, Part};

    fn talk_with_areas() -> Talk {
        let talk = Talk::in_dir(tempfile::tempdir().unwrap(), &["archive"], &[]);
        for area in ["Alfa", "Beta"] {
            for (month, day) in [(1, 10), (3, 11), (6, 12)] {
                talk.write(
                    &format!("archive/{area}/2020/2020-{month:02}-{day} {area} Rechnung.pdf"),
                    &letter(
                        &format!("{area} AG"),
                        &format!("{day}.{month:02}.2020"),
                        "Rechnung",
                        day,
                    ),
                );
            }
        }
        talk
    }

    fn has_levels_card(talk: &Talk) -> bool {
        talk.last_parts()
            .iter()
            .any(|part| matches!(part, Part::Levels(_)))
    }

    #[test]
    fn the_named_area_gets_the_levels_of_exactly_that_folder() {
        let mut talk = talk_with_areas();
        talk.model.push_levels(Ok(
            Levels::from_pattern("{year}/{date} Alfa Rechnung").unwrap()
        ));
        let folder = talk.path("archive");

        talk.send(Message::ChangeArea {
            folder,
            area: "Alfa".to_owned(),
        });

        assert!(has_levels_card(&talk));
        let asked = talk.model.recognized();
        assert_eq!(asked[0].len(), 3, "only the files of Alfa: {asked:?}");
        assert!(
            asked[0].iter().all(|(path, _)| path.starts_with("2020/")),
            "the paths are below the folder: {asked:?}"
        );
        assert!(talk
            .state
            .chips()
            .iter()
            .any(|chip| chip.reply == ChipReply::LevelsKeep));
        assert!(talk.state.open);
    }

    #[test]
    fn an_unknown_area_does_not_open_a_rule_flow() {
        let mut talk = talk_with_areas();
        let folder = talk.path("archive");

        talk.send(Message::ChangeArea {
            folder,
            area: "Gamma".to_owned(),
        });

        assert!(!has_levels_card(&talk));
        assert!(talk.model.recognized().is_empty());
    }
}
