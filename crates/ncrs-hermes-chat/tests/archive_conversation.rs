//! The chat against the test archive: real folders, a real index, a folder
//! backed trash, no model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

#[cfg(test)]
mod tests {

    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    use crate::common::{count, letter, say, Talk};
    use chrono::NaiveDate;
    use ncrs_filing::chat_work::{StayGroups, TrialOutcome, TrialPlan};
    use ncrs_hermes_chat::{ChipReply, Entry, Message, Part};
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

    fn tidy_inbox_until_trial(talk: &mut Talk, transfer: &ChipReply) {
        talk.type_text_understood(
            "Räum Inbox auf",
            r#"{"intent":"tidy_folder","source":"Inbox"}"#,
        );
        talk.press(&ChipReply::ConfirmTidy);
        talk.choose_archive_if_asked();
        talk.press(&ChipReply::Once);
        talk.press(transfer);
        talk.leave_stay_questions();
    }

    /// What the trial of the inbox against the archive finds, measured on the test
    /// archive once the duplicates are in the trash.
    const FILED: usize = 56;
    const UNSURE: usize = 0;
    const ALREADY_THERE: usize = 0;
    const STAYS: usize = 244;
    const INBOX_FILES: usize = 300;
    const DUPLICATES: usize = 40;
    const AREAS: usize = 9;
    fn stay_reasons(plan: &TrialPlan) -> BTreeMap<String, usize> {
        let mut reasons = BTreeMap::new();
        for entry in &plan.entries {
            if let TrialOutcome::Stays(reason) = &entry.outcome {
                *reasons.entry(format!("{reason:?}")).or_insert(0) += 1;
            }
        }
        reasons
    }

    #[test]
    fn conversations_one_two_and_undo_on_the_test_archive() {
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

        // Conversation 2: the inbox against the archive, once, moving.
        talk.type_text_understood(
            "Räum Inbox auf",
            r#"{"intent":"tidy_folder","source":"Inbox"}"#,
        );
        talk.press(&ChipReply::ConfirmTidy);
        talk.choose_archive_if_asked();
        talk.press(&ChipReply::Once);
        talk.press(&ChipReply::ModeMove);
        talk.leave_stay_questions();
        let plan = talk.plan();
        assert_eq!(
            (plan.filed, plan.unsure, plan.already_there, plan.stays),
            (FILED, UNSURE, ALREADY_THERE, STAYS)
        );
        let reasons = stay_reasons(plan);
        // Of the 60 files the intake report gives a path, four collide by name.
        assert_eq!(
            reasons,
            BTreeMap::from([
                ("NameTaken".to_owned(), 4),
                ("NoNeighbours".to_owned(), 167),
                ("NoPattern".to_owned(), 73),
            ])
        );
        assert_eq!(plan.entries.len(), INBOX_FILES);
        assert_eq!(talk.files(), before, "the trial moves nothing");
        let file_all = say(Msg::HermesChatFileAll, &[("count", &count(FILED))]);
        assert_eq!(talk.state.chips()[0].label, file_all);

        talk.press(&ChipReply::FileAll);
        // Folder, sentence, confirm, target, once, move, file: seven entries of
        // the user from Open to the first filed file.
        assert_eq!(talk.entries, 7);
        assert_eq!(
            talk.last_texts()[0],
            say(
                Msg::HermesChatFiled,
                &[
                    ("done", &count(FILED)),
                    ("stays", &count(INBOX_FILES - FILED)),
                    ("folder", "inbox")
                ]
            )
        );
        let filed = talk.files();
        let inbox_left = filed
            .keys()
            .filter(|path| path.starts_with("inbox/"))
            .count();
        assert_eq!(inbox_left, INBOX_FILES - FILED);
        assert_eq!(filed.len(), before.len());

        // The duplicates of the archive are offered with the first filing.
        assert_eq!(
            talk.last_texts()[1],
            say(
                Msg::HermesChatDuplicatesOffer,
                &[
                    ("names", "Downloads alt, Scans"),
                    ("count", &count(DUPLICATES))
                ]
            )
        );
        talk.press(&ChipReply::ToTrash);
        let Some(Entry::Hermes {
            parts: trashed_parts,
            ..
        }) = talk.state.thread().last()
        else {
            panic!()
        };
        assert_eq!(
            trashed_parts[0],
            Part::Text(say(
                Msg::HermesChatTrashed,
                &[("count", &count(DUPLICATES))]
            ))
        );
        assert_eq!(talk.files().len(), filed.len() - DUPLICATES);

        // Undo the trash run first, it is the newest, then the filing.
        talk.press(&ChipReply::Undo);
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatUndoAskTrash,
                &[
                    (
                        "name",
                        &say(Msg::HermesChatTrashRunName, &[("folder", "archive")])
                    ),
                    ("count", &count(DUPLICATES)),
                    ("when", &talk.last_run_when()),
                    ("folder", ""),
                ]
            )]
        );
        talk.press(&ChipReply::PutBack);
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatUndone, &[("count", &count(DUPLICATES))])]
        );
        assert_eq!(talk.files(), filed);
        talk.type_text_understood("mach das rückgängig", r#"{"intent":"undo"}"#);
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatUndoAskMove,
                &[
                    ("name", &say(Msg::HermesChatRunName, &[("folder", "inbox")])),
                    ("count", &count(FILED)),
                    ("when", &talk.last_run_when()),
                    ("folder", "inbox"),
                ]
            )]
        );
        talk.press(&ChipReply::PutBack);
        assert_eq!(talk.files(), before);
    }

    #[test]
    fn copy_mode_files_copies_once_and_undo_removes_only_the_copies() {
        let mut talk = archive_talk();
        let before = talk.files();
        talk.send(Message::Open);
        tidy_inbox_until_trial(&mut talk, &ChipReply::ModeCopy);
        let filed = talk.plan().filed;
        talk.press(&ChipReply::FileAll);
        let copied = talk.files();
        assert_eq!(copied.len(), before.len() + filed);
        for (path, hash) in &before {
            assert_eq!(copied.get(path), Some(hash), "{path} stays as it was");
        }

        // The same sentence again finds everything it copied already there.
        tidy_inbox_until_trial(&mut talk, &ChipReply::ModeCopy);
        assert_eq!(talk.plan().filed, 0);
        assert_eq!(talk.plan().already_there, filed);
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatTrialStuckSome,
                &[
                    ("total", &count(INBOX_FILES)),
                    ("dupes", &count(filed)),
                    ("rest", &count(INBOX_FILES - filed)),
                    ("ai_note", ""),
                ]
            )]
        );
        let labels = talk.labels();
        assert!(labels.len() >= 3, "{labels:?}");
        assert!(
            labels.contains(&say(Msg::HermesChatLaterAgain, &[])),
            "{labels:?}"
        );
        assert_eq!(talk.files(), copied, "nothing is copied twice");

        talk.type_text_understood("mach das rückgängig", r#"{"intent":"undo"}"#);
        assert!(talk.last_texts()[0].contains(&format!("**{filed} Dateien** kopiert")));
        talk.press(&ChipReply::PutBack);
        assert_eq!(talk.files(), before);
    }

    /// The inbox copied to the archive, then the same sentence again: every
    /// file is in the archive already, so nothing is left to file.
    fn all_copied_already(second_transfer: &ChipReply) -> (Talk, BTreeMap<String, String>) {
        let mut talk = archive_talk();
        talk.send(Message::Open);
        tidy_inbox_until_trial(&mut talk, &ChipReply::ModeCopy);
        talk.press(&ChipReply::FileAll);
        let copied = talk.files();
        tidy_inbox_until_trial(&mut talk, second_transfer);
        assert_eq!(talk.plan().filed, 0);
        (talk, copied)
    }

    #[test]
    fn the_duplicates_chip_trashes_the_originals_in_one_run_that_can_be_undone() {
        let (mut talk, copied) = all_copied_already(&ChipReply::ModeMove);
        let dupes = talk.plan().already_there;
        assert!(dupes > 0);
        talk.press(&ChipReply::TrashDuplicatesNow);
        assert_eq!(
            talk.files().len(),
            copied.len() - dupes,
            "the originals left, the copies in the archive stay"
        );
        assert!(talk
            .files()
            .keys()
            .any(|path| path.starts_with("archive") || path.contains("archive")));
        assert_eq!(
            talk.last_texts()[0],
            say(Msg::HermesChatTrashed, &[("count", &count(dupes))])
        );
        talk.press(&ChipReply::Undo);
        talk.press(&ChipReply::PutBack);
        assert_eq!(talk.files(), copied);
    }

    #[test]
    fn copying_offers_no_duplicates_to_delete() {
        let (talk, _) = all_copied_already(&ChipReply::ModeCopy);
        let labels = talk.labels();
        assert!(
            !labels.iter().any(|label| label.contains("Doppelte")),
            "{labels:?}"
        );
        assert!(!talk
            .state
            .chips()
            .iter()
            .any(|chip| chip.reply == ChipReply::TrashDuplicatesNow));
    }

    #[test]
    fn the_staying_files_are_listed_once_and_the_chip_goes() {
        let mut talk = Talk::in_dir(tempfile::tempdir().unwrap(), &["archive"], &["inbox"]);
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
        }
        talk.write("inbox/one.txt", "x");
        talk.write("inbox/two.txt", "y");
        talk.send(Message::Open);
        talk.type_text_understood(
            "Räum Inbox auf",
            r#"{"intent":"tidy_folder","source":"Inbox"}"#,
        );
        talk.press(&ChipReply::ConfirmTidy);
        talk.choose_archive_if_asked();
        talk.press(&ChipReply::Once);
        talk.press(&ChipReply::ModeCopy);
        talk.press(&ChipReply::ShowStaying);
        let parts = talk.last_parts();
        let Some(Part::Trial(card)) = parts.get(1) else {
            panic!("{parts:?}")
        };
        assert!(!card.rows.is_empty());
        assert!(!talk
            .state
            .chips()
            .iter()
            .any(|chip| chip.reply == ChipReply::ShowStaying));
        assert!(talk
            .state
            .chips()
            .iter()
            .any(|chip| chip.reply == ChipReply::Later));
    }

    #[test]
    fn another_folder_at_the_end_asks_for_the_folder_and_keeps_the_transfer() {
        let (mut talk, _) = all_copied_already(&ChipReply::ModeCopy);
        talk.press(&ChipReply::TrialOtherTarget);
        assert!(talk.actions.contains(&ncrs_hermes_chat::Action::PickFolder));
        let proposal = talk.state.session.proposal.as_ref().unwrap();
        assert_eq!(proposal.transfer, Some(ncrs_index::Transfer::Copy));
        assert_eq!(proposal.target, None);
    }

    #[test]
    fn cancel_and_trial_never_transfer() {
        let mut talk = archive_talk();
        let before = talk.files();
        talk.send(Message::Open);
        tidy_inbox_until_trial(&mut talk, &ChipReply::ModeMove);
        assert!(talk.plan().filed > 0);
        assert_eq!(talk.files(), before, "a trial moves nothing");

        talk.type_text_understood(
            "abbrechen",
            &format!(
                r#"{{"intent":"answer","choice":"{}"}}"#,
                say(Msg::HermesChatCancel, &[])
            ),
        );
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatCancelled, &[])]);
        assert!(talk.state.chips().iter().all(|chip| matches!(
            chip.reply,
            ChipReply::IntentSearch
                | ChipReply::IntentTidy
                | ChipReply::IntentStructure
                | ChipReply::IntentUndo
        )));
        assert_eq!(talk.files(), before);

        // Nothing was filed, so there is nothing to put back.
        talk.type_text_understood("mach das rückgängig", r#"{"intent":"undo"}"#);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatUndoNone, &[])]);
    }

    /// The numbers of the test archive: five groups, 135 files in them, 105
    /// single pieces and four taken names make the 244 that stay.
    fn assert_first_groups(groups: &StayGroups) {
        assert_eq!(groups.groups, 5);
        assert_eq!(groups.asked.len(), 5);
        let sizes: Vec<usize> = groups.asked.iter().map(|group| group.count()).collect();
        assert_eq!(sizes, [37, 27, 26, 25, 20]);
        assert_eq!(groups.files_in_first(5), 135);
        assert_eq!(groups.single_pieces, 105);
        assert_eq!(groups.name_taken.len(), 4);
        assert_eq!(
            groups.asked.len() + groups.rest.len() + groups.name_taken.len(),
            5 + 105 + 4
        );
    }

    fn stored_rules(talk: &Talk) -> Vec<(Option<String>, String, bool, bool)> {
        let guard = talk.index.lock().unwrap();
        let rules = guard.as_ref().unwrap().rules().unwrap();
        rules
            .into_iter()
            .map(|rule| {
                (
                    rule.spec.name,
                    rule.spec.filing_pattern,
                    rule.spec.confirmed,
                    rule.spec.enabled,
                )
            })
            .collect()
    }

    #[test]
    fn a_typed_answer_to_trash_changes_nothing_until_the_chip_is_pressed() {
        let mut talk = archive_talk();
        talk.send(Message::Open);
        talk.press(&ChipReply::Folder(archive(&talk)));
        tidy_inbox_until_trial(&mut talk, &ChipReply::ModeMove);
        talk.press(&ChipReply::FileAll);
        let trash_label = talk
            .state
            .chips()
            .iter()
            .find(|chip| chip.reply == ChipReply::ToTrash)
            .unwrap()
            .label
            .clone();
        let filed = talk.files();

        talk.type_text_understood(
            "ja, weg damit",
            &format!(r#"{{"intent":"answer","choice":"{trash_label}"}}"#),
        );
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatTypedChoiceConfirm,
                &[("choice", &trash_label)]
            )]
        );
        assert_eq!(
            talk.labels(),
            [trash_label, say(Msg::HermesChatCancel, &[])]
        );
        assert_eq!(talk.files(), filed, "a typed answer trashes nothing");

        talk.press(&ChipReply::ToTrash);
        assert_eq!(talk.files().len(), filed.len() - DUPLICATES);
        assert_eq!(
            talk.model.turns().len(),
            2,
            "the tidy sentence and the typed answer, never the chip"
        );
    }

    #[test]
    fn staying_groups_are_asked_about_and_the_answers_become_rules() {
        let mut talk = archive_talk();
        let before = talk.files();
        talk.send(Message::Open);
        talk.type_text_understood(
            "Räum Inbox auf",
            r#"{"intent":"tidy_folder","source":"Inbox"}"#,
        );
        talk.press(&ChipReply::ConfirmTidy);
        talk.choose_archive_if_asked();
        talk.press(&ChipReply::Once);
        talk.press(&ChipReply::ModeMove);

        let groups = talk.plan().groups.clone();
        println!(
            "stay groups: groups {} first5_files {} single_pieces {}",
            groups.groups,
            groups.files_in_first(5),
            groups.single_pieces
        );
        assert_first_groups(&groups);

        // The first question: the largest group, the offered areas, the way out.
        let vodafone = &groups.asked[0];
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatGroupWhere,
                &[
                    ("count", &count(vodafone.count())),
                    ("label", &vodafone.label),
                    ("examples", &vodafone.examples.join(", ")),
                ]
            )]
        );
        let labels = talk.labels();
        assert_eq!(
            labels[..3],
            vodafone.areas[..],
            "the offered areas come first"
        );
        assert_eq!(
            labels[3..],
            [
                say(Msg::HermesChatNewFolder, &[]),
                say(Msg::HermesChatLeaveThem, &[])
            ]
        );

        // The first group goes to an area chip.
        talk.press(&ChipReply::GroupArea(vodafone.areas[0].clone()));
        let after_first = talk.plan().clone();
        assert_eq!(after_first.filed, 92, "the rule files the group at once");
        assert_eq!(talk.files(), before, "asking and the trial move nothing");
        let sentence = talk.last_texts();
        assert!(
            sentence[0].starts_with("27 Dateien von Finanzamt Chemnitz-Süd"),
            "the next group is asked about: {sentence:?}"
        );
        assert_eq!(after_first.groups.asked.len(), 4);

        // The second group is answered in words: the model names the chip.
        let finanzamt = &after_first.groups.asked[0];
        assert_eq!(finanzamt.label, "Finanzamt Chemnitz-Süd");
        let steuer = finanzamt.areas[0].clone();
        assert_eq!(steuer, "Steuer");
        talk.type_text_understood(
            "das kommt alles zu steuer",
            &format!(r#"{{"intent":"answer","choice":"{steuer}"}}"#),
        );
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatTypedChoiceConfirm,
                &[("choice", &steuer)]
            )]
        );
        assert_eq!(talk.plan().filed, 92, "a typed answer files nothing yet");
        talk.press(&ChipReply::GroupArea(steuer.clone()));
        let after_second = talk.plan().clone();
        assert_eq!((after_second.filed, after_second.stays), (119, 181));
        assert_eq!(
            stay_reasons(&after_second),
            BTreeMap::from([
                ("NameTaken".to_owned(), 5),
                ("NoNeighbours".to_owned(), 130),
                ("NoPattern".to_owned(), 46),
            ])
        );
        assert_eq!(after_second.groups.asked.len(), 3);
        assert!(after_second.filed > after_first.filed);
        assert_eq!(after_second.entries.len(), INBOX_FILES);

        // The rules are stored, confirmed, named after the groups.
        assert_eq!(
            stored_rules(&talk),
            [
                (
                    Some("Vodafone GmbH".to_owned()),
                    "Bank/".to_owned(),
                    true,
                    true
                ),
                (
                    Some("Finanzamt Chemnitz-Süd".to_owned()),
                    "Steuer/".to_owned(),
                    true,
                    true
                ),
            ]
        );

        // The rest is left as it is; then everything the trial filed is filed.
        talk.leave_stay_questions();
        let ready = after_second.filed;
        assert_eq!(
            talk.state.chips()[0].label,
            say(Msg::HermesChatFileAll, &[("count", &count(ready))])
        );
        talk.press(&ChipReply::FileAll);
        let filed = talk.files();
        let inbox_left = filed
            .keys()
            .filter(|path| path.starts_with("inbox/"))
            .count();
        assert_eq!(inbox_left, INBOX_FILES - ready);
        assert!(
            filed
                .keys()
                .any(|path| path.starts_with("archive/Bank/20")
                    && path.ends_with("Vodafone-GmbH.pdf"))
        );

        talk.type_text_understood("mach das rückgängig", r#"{"intent":"undo"}"#);
        talk.press(&ChipReply::PutBack);
        assert_eq!(talk.files(), before, "undo puts everything back");
        assert_eq!(
            talk.index
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .rules()
                .unwrap()
                .len(),
            2,
            "the rules stay: they are taken back on the rules page"
        );
    }

    #[test]
    fn a_new_folder_is_named_in_the_chat_and_taken_names_are_numbered() {
        let mut talk = archive_talk();
        let before = talk.files();
        talk.send(Message::Open);
        talk.type_text_understood(
            "Räum Inbox auf",
            r#"{"intent":"tidy_folder","source":"Inbox"}"#,
        );
        talk.press(&ChipReply::ConfirmTidy);
        talk.choose_archive_if_asked();
        talk.press(&ChipReply::Once);
        talk.press(&ChipReply::ModeMove);

        talk.press(&ChipReply::GroupNewFolder);
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatNewFolderName, &[])]
        );
        // The name does not go through the model: none is scripted.
        for bad in ["../Heim", "a/b", ".."] {
            talk.type_text(bad);
            assert_eq!(
                talk.last_texts(),
                vec![say(Msg::HermesChatFolderNameBad, &[])]
            );
        }
        talk.type_text("Handy");
        assert!(talk.plan().filed > FILED);
        assert!(talk.plan().entries.iter().any(
            |entry| matches!(&entry.outcome, TrialOutcome::Filed { to } if to.starts_with("Handy"))
        ));
        assert!(!talk
            .files()
            .keys()
            .any(|path| path.starts_with("archive/Handy")));

        // The groups left, then the single pieces shown ten at a time.
        while talk
            .state
            .chips()
            .iter()
            .any(|chip| chip.reply == ChipReply::GroupLeave)
        {
            talk.press(&ChipReply::GroupLeave);
        }
        let pieces = talk.plan().groups.rest.len();
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatSinglePieces,
                &[("count", &count(pieces))]
            )]
        );
        talk.press(&ChipReply::SinglesShow);
        let Some(Part::Trial(card)) = talk.last_parts().get(1) else {
            panic!("{:?}", talk.last_parts())
        };
        assert_eq!(card.rows.len(), 10);
        talk.press(&ChipReply::SinglesLeave);

        // Taken names are filed with a number; no rule is stored for that.
        let taken = talk.plan().groups.name_taken.clone();
        assert!(!taken.is_empty());
        let filed_before = talk.plan().filed;
        talk.press(&ChipReply::NamesNumbered);
        assert_eq!(talk.plan().filed, filed_before + taken.len());
        assert!(taken.iter().all(|name| !talk.plan().taken.is_empty()
            && talk.plan().entries[name.entry].outcome
                == TrialOutcome::Filed {
                    to: name.suggested.clone()
                }));
        let rule_count = talk
            .index
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .rules()
            .unwrap()
            .len();
        assert_eq!(rule_count, 1);

        talk.press(&ChipReply::FileAll);
        let filed = talk.files();
        assert!(filed.keys().any(|path| path.starts_with("archive/Handy/")));
        talk.type_text_understood("mach das rückgängig", r#"{"intent":"undo"}"#);
        talk.press(&ChipReply::PutBack);
        assert_eq!(talk.files(), before);
    }
}
