//! The settings page names one area: the chat reads its folder and offers
//! the orders for exactly that area.

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
    use ncrs_hermes_chat::{ChipReply, Message};

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

    fn order_chips_for(talk: &Talk) -> Vec<String> {
        talk.state
            .chips()
            .iter()
            .filter_map(|chip| match &chip.reply {
                ChipReply::ChangeAreaOrder { area, .. } => Some(area.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_named_area_is_offered_its_orders() {
        let mut talk = talk_with_areas();
        let folder = talk.path("archive");

        talk.send(Message::ChangeArea {
            folder,
            area: "Alfa".to_owned(),
        });

        let areas = order_chips_for(&talk);
        assert!(!areas.is_empty());
        assert!(areas.iter().all(|area| area == "Alfa"), "{areas:?}");
        assert!(talk.state.open);
    }

    #[test]
    fn an_unknown_area_falls_back_to_the_summary() {
        let mut talk = talk_with_areas();
        let folder = talk.path("archive");

        talk.send(Message::ChangeArea {
            folder,
            area: "Gamma".to_owned(),
        });

        assert!(order_chips_for(&talk).is_empty());
    }
}
