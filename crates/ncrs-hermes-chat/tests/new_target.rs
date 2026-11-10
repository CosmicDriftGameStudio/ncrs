//! "New folder …" and "I'll decide per file" in the question where the files
//! of a source go.

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
    use ncrs_filing::chat_work::ChatResult;
    use ncrs_hermes_chat::{Action, ChipReply, Message, NewLayout, Part};

    const TIDY_INBOX: &str = r#"{"intent":"tidy_folder","source":"Inbox"}"#;

    /// Two filing locations and an inbox with two letters; the talk stands at
    /// "Where should this go?".
    fn asking_where() -> Talk {
        let mut talk = Talk::in_dir(
            tempfile::tempdir().unwrap(),
            &["archive", "archive2"],
            &["inbox"],
        );
        for (name, base) in [("archive", 10), ("archive2", 20)] {
            for month in [1, 2, 3] {
                talk.write(
                    &format!("{name}/Alfa/2020/{base}{month}.pdf"),
                    &letter(
                        "Alfa AG",
                        &format!("1{month}.0{month}.2020"),
                        "Rechnung",
                        base + month,
                    ),
                );
            }
        }
        talk.write(
            "inbox/neu_alfa.pdf",
            &letter("Alfa AG", "05.03.2021", "Rechnung", 77),
        );
        talk.write(
            "inbox/neu_beta.pdf",
            &letter("Beta GmbH", "06.03.2021", "Rechnung", 78),
        );
        talk.type_text_understood("Räum Inbox auf", TIDY_INBOX);
        talk.press(&ChipReply::ConfirmTidy);
        talk
    }

    #[test]
    fn the_question_offers_a_new_folder_and_a_decision_per_file() {
        let talk = asking_where();
        let replies: Vec<ChipReply> = talk
            .state
            .chips()
            .iter()
            .map(|chip| chip.reply.clone())
            .collect();
        assert!(
            replies.contains(&ChipReply::NewTarget),
            "{:?}",
            talk.labels()
        );
        assert!(
            replies.contains(&ChipReply::DecidePerFile),
            "{:?}",
            talk.labels()
        );
    }

    #[test]
    fn a_new_empty_folder_asks_for_its_order_and_the_trial_files_by_it() {
        let mut talk = asking_where();
        talk.press(&ChipReply::NewTarget);
        assert!(talk.actions.contains(&Action::PickFolder));
        let fresh = talk.path("fresh");
        std::fs::create_dir_all(&fresh).unwrap();
        talk.send(Message::FolderPicked(Some(fresh.clone())));
        assert!(
            talk.state
                .chips()
                .iter()
                .any(|chip| chip.reply == ChipReply::NewLayout(NewLayout::Year)),
            "{:?}",
            talk.labels()
        );
        assert!(
            talk.actions.contains(&Action::SetFolderRole {
                folder: fresh.clone(),
                role: ncrs_ui_kit::hermes_state::FolderRole::FilingLocation,
            }),
            "{:?}",
            talk.actions
        );
        talk.press(&ChipReply::NewLayout(NewLayout::Year));
        talk.press(&ChipReply::Once);
        talk.press(&ChipReply::ModeCopy);
        let plan = talk.plan();
        assert_eq!(plan.filed, 2, "{plan:?}");
        talk.press(&ChipReply::FileAll);
        let filed = std::fs::read_dir(fresh.join("2021")).unwrap().count();
        assert_eq!(filed, 2);
    }

    #[test]
    fn a_new_folder_that_holds_an_order_shows_its_areas_first() {
        let mut talk = asking_where();
        talk.write(
            "other/Gamma/2020/g1.pdf",
            &letter("Gamma AG", "01.01.2020", "Rechnung", 91),
        );
        talk.write(
            "other/Gamma/2020/g2.pdf",
            &letter("Gamma AG", "02.01.2020", "Rechnung", 92),
        );
        talk.press(&ChipReply::NewTarget);
        talk.send(Message::FolderPicked(Some(talk.path("other"))));
        assert!(
            talk.last_parts()
                .iter()
                .any(|part| matches!(part, Part::Areas(_))),
            "{:?}",
            talk.last_parts()
        );
        talk.press(&ChipReply::KeepYes);
        assert!(
            talk.state
                .chips()
                .iter()
                .any(|chip| chip.reply == ChipReply::Once),
            "{:?}",
            talk.labels()
        );
    }

    #[test]
    fn closing_the_dialog_of_the_new_folder_asks_where_the_files_go_again() {
        let mut talk = asking_where();
        let question: Vec<ChipReply> = talk
            .state
            .chips()
            .iter()
            .map(|chip| chip.reply.clone())
            .collect();
        talk.press(&ChipReply::NewTarget);
        talk.send(Message::FolderPicked(None));
        let again: Vec<ChipReply> = talk
            .state
            .chips()
            .iter()
            .map(|chip| chip.reply.clone())
            .collect();
        assert_eq!(again, question);
        assert!(talk.state.session.tidy.is_some());
    }

    #[test]
    fn a_failing_run_while_deciding_reports_what_was_filed_and_offers_undo() {
        let mut talk = asking_where();
        talk.press(&ChipReply::DecidePerFile);
        talk.press(&ChipReply::ModeCopy);
        talk.state.session.deciding.as_mut().expect("deciding").done = 1;
        talk.send(Message::Done(Box::new(ChatResult::Filed(Err(
            "disk full".to_owned()
        )))));
        assert!(talk.state.session.deciding.is_none());
        assert!(talk.state.session.readings.is_empty());
        assert!(talk
            .state
            .chips()
            .iter()
            .any(|chip| chip.reply == ChipReply::Undo));
    }

    #[test]
    fn deciding_per_file_files_each_file_where_it_was_sent_and_leaves_the_skipped() {
        let mut talk = asking_where();
        talk.press(&ChipReply::DecidePerFile);
        talk.press(&ChipReply::ModeCopy);
        talk.press(&ChipReply::DecideTarget(talk.path("archive2")));
        talk.press(&ChipReply::DecideSkip);
        let archive2 = talk.path("archive2");
        let landed: Vec<String> = std::fs::read_dir(&archive2)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("neu_"))
            .collect();
        assert_eq!(landed.len(), 1, "{landed:?}");
        assert!(!talk.path("archive").join(&landed[0]).exists());
        assert!(talk
            .state
            .chips()
            .iter()
            .any(|chip| chip.reply == ChipReply::Undo));
    }

    #[test]
    fn deciding_to_skip_every_file_files_nothing() {
        let mut talk = asking_where();
        talk.press(&ChipReply::DecidePerFile);
        talk.press(&ChipReply::ModeCopy);
        talk.press(&ChipReply::DecideSkip);
        talk.press(&ChipReply::DecideSkip);
        assert!(talk.state.session.deciding.is_none());
        assert!(!talk
            .files()
            .keys()
            .any(|path| path.starts_with("archive2/neu_")));
    }
}
