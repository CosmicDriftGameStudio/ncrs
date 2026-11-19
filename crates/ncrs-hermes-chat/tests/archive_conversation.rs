//! The chat against the test archive: real folders, a real index, a folder
//! backed trash, no model.

mod common;

#[cfg(test)]
mod tests {

    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    use crate::common::{count, say, Talk};
    use chrono::NaiveDate;
    use ncrs_hermes_chat::{Action, ChipReply, Message};
    use ncrs_i18n::Msg;
    use ncrs_intake::test_archive::test_archive;

    const UNDATED: (i32, u32, u32) = (2021, 6, 15);

    fn instant(date: NaiveDate) -> SystemTime {
        let seconds = date.and_hms_opt(12, 0, 0).unwrap().and_utc().timestamp();
        SystemTime::UNIX_EPOCH + Duration::from_secs(u64::try_from(seconds).unwrap())
    }

    /// Written files get the letter date as their modification time, so the date
    /// the chat falls back on does not depend on the day the test runs.
    fn archive_talk() -> Talk {
        let dir = tempfile::tempdir().unwrap();
        let archive = test_archive();
        archive.write_to(dir.path()).unwrap();
        let undated = NaiveDate::from_ymd_opt(UNDATED.0, UNDATED.1, UNDATED.2).unwrap();
        for (folder, files) in [("archive", &archive.archive), ("inbox", &archive.inbox)] {
            for file in files {
                let path = dir.path().join(folder).join(&file.relative_path);
                let handle = fs::OpenOptions::new().write(true).open(path).unwrap();
                handle
                    .set_modified(instant(file.date.unwrap_or(undated)))
                    .unwrap();
            }
        }
        Talk::in_dir(dir, &["archive"], &["inbox"])
    }

    fn archive(talk: &Talk) -> PathBuf {
        talk.path("archive")
    }

    const AREAS: usize = 9;

    #[test]
    fn conversations_one_and_two_on_the_test_archive_end_at_the_tab() {
        let mut talk = archive_talk();
        let before = talk.files();

        // Conversation 1: open the archive; its areas are offered as folders.
        talk.send(Message::Open);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatGreeting, &[])]);
        talk.press(&ChipReply::Folder(archive(&talk)));
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatAreasHere, &[("count", &count(AREAS))])]
        );
        assert!(talk.state.chips().iter().any(|chip| chip.label == "Bank"));
        assert_eq!(talk.files(), before);

        // Conversation 2: the inbox against the archive, once. The chat hands
        // the plan to the tab and changes nothing itself.
        talk.type_text_understood(
            "Räum Inbox auf",
            r#"{"intent":"tidy_folder","source":"Inbox"}"#,
        );
        talk.press(&ChipReply::ConfirmTidy);
        talk.choose_archive_if_asked();
        talk.press(&ChipReply::Once);
        // Folder, sentence, confirm, target, once: five entries of the user.
        assert_eq!(talk.entries, 5);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatHandover, &[])]);
        assert!(
            talk.actions.iter().any(|action| matches!(
                action,
                Action::OpenFiling { source, target, .. }
                    if source.ends_with("inbox") && target.ends_with("archive")
            )),
            "{:?}",
            talk.actions
        );
        assert_eq!(talk.files(), before, "the chat moves nothing");
    }
}
