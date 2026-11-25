//! The chat against the test archive: real folders, a real index, a scripted
//! model.

mod common;

#[cfg(test)]
mod tests {

    use std::fs;
    use std::time::{Duration, SystemTime};

    use crate::common::{count, say, Talk};
    use chrono::NaiveDate;
    use ncrs_ai::Levels;
    use ncrs_hermes_chat::{Action, ChipReply, Message, Part};
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

    const AREAS: usize = 9;
    const PATTERN: &str = "{year}/{date}_{sender}_{kind}";

    #[test]
    fn the_test_archive_is_looked_at_and_the_inbox_is_only_pointed_to() {
        let mut talk = archive_talk();
        let before = talk.files();

        // The archive: its areas, its levels, and the rule suggestions.
        talk.send(Message::Open);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatGreeting, &[])]);
        for _ in 0..2 {
            talk.model
                .push_levels(Ok(Levels::from_pattern(PATTERN).unwrap()));
        }
        talk.press(&ChipReply::Folder(talk.path("archive")));
        assert!(
            talk.last_parts()
                .iter()
                .any(|part| matches!(part, Part::Levels(_))),
            "{:?}",
            talk.last_parts()
        );
        assert!(
            talk.labels().contains(&say(
                Msg::HermesChatShowAll,
                &[("areas", &count(AREAS))]
            )),
            "{:?}",
            talk.labels()
        );
        talk.press(&ChipReply::SuggestRules);
        let suggested = talk.suggestions();
        assert_eq!(suggested.len(), 1, "{suggested:?}");
        assert_eq!(suggested[0].0, talk.path("archive"));

        // The inbox: the chat points to the dialog and changes nothing.
        talk.type_text_understood(
            "Räum Inbox auf",
            r#"{"intent":"tidy_folder","source":"Inbox"}"#,
        );
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatSortHint, &[])]);
        let inbox = Some(talk.path("inbox"));
        talk.press(&ChipReply::OpenSort(inbox.clone()));
        assert_eq!(
            talk.actions.last(),
            Some(&Action::OpenSortDialog { folder: inbox })
        );
        assert_eq!(talk.files(), before, "the chat moves nothing");
        talk.assert_saves_no_rule();
    }
}
