//! The documents no rule could place: the opening bubble says how many wait,
//! and "Zeigen" walks through them group by group.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::{NaiveDate, TimeZone as _, Utc};
    use ncrs_hermes_chat::{Action, ChipReply, Message};
    use ncrs_i18n::Msg;
    use ncrs_index::{
        DocumentFields, DocumentId, DocumentSource, DocumentStatus, FieldOrigin, FileStamp,
        NewDocument, ReviewReason,
    };
    use ncrs_intake::hash_path;

    use crate::common::{Talk, LANG};

    fn text(msg: Msg) -> String {
        LANG.text(msg).to_owned()
    }

    fn fields(date: NaiveDate) -> DocumentFields {
        DocumentFields {
            sender: Some("Company A".to_owned()),
            kind: Some("invoice".to_owned()),
            date: Some(date),
            ..DocumentFields::default()
        }
    }

    fn insert(talk: &Talk, relative: &str, source: DocumentSource, date: NaiveDate) -> DocumentId {
        talk.write(relative, &format!("content of {relative}"));
        let path = talk.path(relative);
        let metadata = fs::metadata(&path).unwrap();
        let mut guard = talk.index.lock().unwrap();
        let index = guard.as_mut().unwrap();
        let id = index
            .insert_document(&NewDocument {
                hash: hash_path(&path).unwrap(),
                path,
                source,
                received_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                stamp: Some(FileStamp {
                    size: metadata.len(),
                    mtime: metadata.modified().unwrap(),
                }),
            })
            .unwrap();
        index
            .set_document_fields(id, &fields(date), |_| FieldOrigin::Unknown)
            .unwrap();
        index.set_document_text(id, "Invoice").unwrap();
        id
    }

    /// Invoices of Company A in `folder` below the archive.
    fn neighbours_in(talk: &Talk, folder: &str) {
        for month in 1..=5 {
            let date = NaiveDate::from_ymd_opt(2024, month, 1).unwrap();
            insert(
                talk,
                &format!("archive/{folder}/{date}_invoice.pdf"),
                DocumentSource::Archive,
                date,
            );
        }
    }

    fn wait(talk: &Talk, name: &str, day: u32) {
        let id = insert(
            talk,
            &format!("inbox/{name}"),
            DocumentSource::Inbox,
            NaiveDate::from_ymd_opt(2024, 6, day).unwrap(),
        );
        let mut guard = talk.index.lock().unwrap();
        let index = guard.as_mut().unwrap();
        index
            .set_document_status(id, DocumentStatus::InReview)
            .unwrap();
        index.add_review(id, ReviewReason::NoMatchingRule).unwrap();
    }

    fn talk_with(waiting: u32) -> Talk {
        let talk =
            Talk::in_dir(tempfile::tempdir().unwrap(), &["archive"], &["inbox"]).with_review();
        neighbours_in(&talk, "Insurance/2024");
        for day in 1..=waiting {
            wait(&talk, &format!("scan-{day}.pdf"), day);
        }
        talk
    }

    fn opener(talk: &Talk) -> Vec<String> {
        talk.last_texts()
    }

    fn has_show_chip(talk: &Talk) -> bool {
        talk.state
            .chips()
            .iter()
            .any(|chip| chip.reply == ChipReply::WaitingShow)
    }

    #[test]
    fn without_waiting_documents_the_greeting_has_no_note() {
        let mut talk = talk_with(0);
        talk.open();
        assert_eq!(opener(&talk), [text(Msg::HermesChatGreeting)]);
        assert!(!has_show_chip(&talk));
    }

    #[test]
    fn one_waiting_document_is_said_in_the_singular_with_a_show_chip() {
        let mut talk = talk_with(1);
        talk.open();
        assert_eq!(
            opener(&talk),
            [
                text(Msg::HermesChatGreeting),
                "1 Dokument wartet auf dich.".to_owned()
            ]
        );
        assert!(has_show_chip(&talk));
    }

    #[test]
    fn three_waiting_documents_are_counted_with_a_show_chip() {
        let mut talk = talk_with(3);
        talk.open();
        assert_eq!(
            opener(&talk),
            [
                text(Msg::HermesChatGreeting),
                "3 Dokumente warten auf dich.".to_owned()
            ]
        );
        assert!(has_show_chip(&talk));
    }

    #[test]
    fn a_group_with_one_folder_is_asked_once_and_filed_together() {
        let mut talk = talk_with(3);
        talk.open();
        talk.press(&ChipReply::WaitingShow);
        let question = talk.last_texts().join(" ");
        assert!(
            question.starts_with("3 davon gehören nach „Insurance / 2024“"),
            "{question}"
        );

        talk.press(&ChipReply::WaitingFile);

        assert_eq!(
            talk.last_texts(),
            ["Das waren alle. **3** eingeordnet, 0 warten weiter.".to_owned()]
        );
        for day in 1..=3 {
            assert!(!talk.path(&format!("inbox/scan-{day}.pdf")).exists());
        }
        let filed = fs::read_dir(talk.path("archive/Insurance/2024"))
            .unwrap()
            .count();
        assert_eq!(filed, 5 + 3);
    }

    #[test]
    fn a_group_can_be_left_and_stays_waiting() {
        let mut talk = talk_with(2);
        talk.open();
        talk.press(&ChipReply::WaitingShow);
        talk.press(&ChipReply::WaitingSkip);

        assert_eq!(
            talk.last_texts(),
            ["Das waren alle. **0** eingeordnet, 2 warten weiter.".to_owned()]
        );
        assert!(talk.path("inbox/scan-1.pdf").exists());
    }

    #[test]
    fn a_document_nothing_is_sure_about_is_asked_alone_with_the_nearest_folder() {
        let mut talk =
            Talk::in_dir(tempfile::tempdir().unwrap(), &["archive"], &["inbox"]).with_review();
        neighbours_in(&talk, "Insurance/2024");
        neighbours_in(&talk, "Contracts/2024");
        wait(&talk, "scan-1.pdf", 1);
        talk.open();
        talk.press(&ChipReply::WaitingShow);

        let question = talk.last_texts();
        assert_eq!(
            question[0],
            "Bei „scan-1.pdf“ bin ich nicht sicher, wohin es gehört."
        );
        assert!(
            question[1].starts_with("Ähnliche Dokumente liegen in"),
            "{question:?}"
        );
        assert!(!talk
            .state
            .chips()
            .iter()
            .any(|chip| chip.reply == ChipReply::WaitingFile));
    }

    fn unsure_talk() -> Talk {
        let mut talk =
            Talk::in_dir(tempfile::tempdir().unwrap(), &["archive"], &["inbox"]).with_review();
        neighbours_in(&talk, "Insurance/2024");
        neighbours_in(&talk, "Contracts/2024");
        wait(&talk, "scan-1.pdf", 1);
        talk.open();
        talk.press(&ChipReply::WaitingShow);
        talk
    }

    #[test]
    fn an_unsure_document_is_filed_in_the_folder_of_the_similar_ones() {
        let mut talk = unsure_talk();
        let folder = talk
            .state
            .chips()
            .iter()
            .find_map(|chip| match &chip.reply {
                ChipReply::WaitingFileInto(folder) => Some(folder.clone()),
                _ => None,
            })
            .expect("a chip with the proposed folder");

        talk.press(&ChipReply::WaitingFileInto(folder.clone()));

        assert_eq!(
            talk.last_texts(),
            ["Das waren alle. **1** eingeordnet, 0 warten weiter.".to_owned()]
        );
        assert!(!talk.path("inbox/scan-1.pdf").exists());
        assert!(talk
            .dir
            .path()
            .join("archive")
            .join(folder)
            .join("scan-1.pdf")
            .exists());
    }

    #[test]
    fn an_unsure_document_is_filed_in_a_folder_the_user_picks() {
        let mut talk = unsure_talk();

        talk.press(&ChipReply::WaitingOtherFolder);
        assert_eq!(talk.actions.last(), Some(&Action::PickFolder));
        talk.send(Message::FolderPicked(Some(
            talk.path("archive/Insurance/2024"),
        )));

        assert_eq!(
            talk.last_texts(),
            ["Das waren alle. **1** eingeordnet, 0 warten weiter.".to_owned()]
        );
        assert!(!talk.path("inbox/scan-1.pdf").exists());
        assert!(talk.path("archive/Insurance/2024/scan-1.pdf").exists());
    }

    #[test]
    fn documents_that_arrive_while_the_chat_is_open_and_idle_are_offered() {
        let mut talk = talk_with(0);
        talk.open();
        assert!(!has_show_chip(&talk));
        wait(&talk, "scan-1.pdf", 1);

        talk.send(Message::WaitingCheck);

        assert_eq!(opener(&talk), ["1 Dokument wartet auf dich.".to_owned()]);
        assert!(has_show_chip(&talk));
        let entries = talk.state.thread().len();
        talk.ctx.now += chrono::Duration::seconds(60);
        talk.send(Message::WaitingCheck);
        assert_eq!(talk.state.thread().len(), entries, "told once, not again");
    }
}
