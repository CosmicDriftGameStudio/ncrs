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

    use crate::common::{count, say, Talk};
    use chrono::NaiveDate;
    use ncrs_filing::chat_work::{StayGroups, TrialOutcome, TrialPlan};
    use ncrs_hermes_chat::{ChipReply, Entry, FileCard, Message, Part};
    use ncrs_i18n::Msg;
    use ncrs_index::{KindLanguage, Kinds};
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
    const ARCHIVE_FILES: usize = 1602;
    const DUPLICATES: usize = 40;
    const AREAS: usize = 9;
    const MONTHLY_STATEMENTS: usize = 37;
    fn assert_areas_card(talk: &Talk) {
        let Some(Entry::Hermes { parts, .. }) = talk.state.thread().last() else {
            panic!()
        };
        let Some(Part::Areas(card)) = parts.get(1) else {
            panic!("{parts:?}")
        };
        assert_eq!(card.rows.len(), 3);
        assert_eq!(
            (card.rows[0].name.as_str(), card.rows[0].documents),
            ("Bank", 354)
        );
        let bank = &card.rows[0];
        assert!(bank.description.contains(" / "), "{}", bank.description);
        assert!(
            bank.example.starts_with("Bank/") && bank.example.contains('.'),
            "{}",
            bank.example
        );
        assert_eq!(card.more.as_ref().map(|more| more.documents), Some(554));
        assert_eq!(card.unordered.as_ref().map(|row| row.documents), Some(190));
    }

    fn hermes_texts(talk: &Talk) -> Vec<String> {
        talk.state
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
            .collect()
    }

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

        // Conversation 1: read the archive.
        talk.send(Message::Open);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatGreeting, &[])]);
        talk.press(&ChipReply::Folder(archive(&talk)));
        assert_eq!(
            talk.last_texts(),
            vec![
                say(
                    Msg::HermesChatReadSummary,
                    &[("count", &count(ARCHIVE_FILES)), ("areas", &count(AREAS))]
                ),
                say(Msg::HermesChatAskKeep, &[]),
            ]
        );
        assert_areas_card(&talk);

        talk.press(&ChipReply::KeepYes);
        assert!(talk.actions.is_empty(), "the archive is a target already");
        // The duplicates of the archive wait for the first filing.
        assert_eq!(
            talk.last_texts(),
            vec![
                say(Msg::HermesChatKept, &[("folder", "archive")]),
                say(Msg::HermesChatAskSource, &[]),
            ]
        );
        assert_eq!(talk.files(), before);

        // Conversation 2: the source from Hermes' question; even with one
        // target Hermes asks where to, then once, moving.
        talk.press(&ChipReply::Folder(talk.path("inbox")));
        assert_eq!(
            hermes_texts(&talk)
                .into_iter()
                .find(|text| text.starts_with("Gern!")),
            Some(say(
                Msg::HermesChatSourceSummary,
                &[("folder", "inbox"), ("count", &count(INBOX_FILES))]
            ))
        );
        talk.press(&ChipReply::Folder(talk.path("archive")));
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
        // Folder, keep, source, target, once, move, file: seven entries of the user from
        // Open to the first filed file.
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
                Msg::HermesChatNothingToFile,
                &[("stays", &count(INBOX_FILES))]
            )]
        );
        assert_eq!(talk.files(), copied, "nothing is copied twice");

        talk.type_text_understood("mach das rückgängig", r#"{"intent":"undo"}"#);
        assert!(talk.last_texts()[0].contains(&format!("**{filed} Dateien** kopiert")));
        talk.press(&ChipReply::PutBack);
        assert_eq!(talk.files(), before);
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
        assert!(talk.state.chips().is_empty());
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
        talk.press(&ChipReply::KeepYes);
        talk.press(&ChipReply::Folder(talk.path("inbox")));
        talk.choose_archive_if_asked();
        talk.press(&ChipReply::Once);
        talk.press(&ChipReply::ModeMove);
        talk.leave_stay_questions();
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
        assert_eq!(talk.model.turns().len(), 1, "the chip never asks the model");
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

    fn statement_word() -> String {
        Kinds::built_in(KindLanguage::default())
            .kind_word("bank_statement")
            .into_owned()
    }

    /// "Kontoauszüge lieber nach Monat" on the archive up to the question which
    /// banks.
    fn ask_statements_by_month(talk: &mut Talk) {
        talk.send(Message::Open);
        talk.press(&ChipReply::Folder(archive(talk)));
        talk.press(&ChipReply::WantChange);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatAskChange, &[])]);
        talk.type_text_understood(
            "Kontoauszüge lieber nach Monat",
            r#"{"intent":"change_structure","area":"Kontoauszüge","order":"by_month"}"#,
        );

        let segments: Vec<String> = [
            "Bank",
            "Sparkasse Chemnitz",
            "Kontoauszüge",
            "2024",
            "12",
            "Kontoauszug_2024-12-28.pdf",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let banks = say(Msg::HermesChatPluralBank, &[]);
        assert_eq!(
            talk.last_parts(),
            [
                Part::Text(say(
                    Msg::HermesChatChangePreview,
                    &[("kind", &statement_word())]
                )),
                Part::File(FileCard {
                    segments,
                    highlighted: Some(4)
                }),
                Part::Text(say(Msg::HermesChatChangeScope, &[("plural", &banks)])),
            ]
        );
        let labels = talk.labels();
        assert_eq!(
            labels[0],
            say(Msg::HermesChatScopeAll, &[("plural", &banks)])
        );
        assert!(
            labels.contains(&say(
                Msg::HermesChatScopeOnly,
                &[("value", "Sparkasse Chemnitz")]
            )),
            "{labels:?}"
        );
    }

    /// Files the inbox against the archive after the change and checks which
    /// documents got a month folder.
    fn trial_after_change(talk: &mut Talk) -> Vec<PathBuf> {
        talk.press(&ChipReply::FineAsIs);
        talk.press(&ChipReply::Folder(talk.path("inbox")));
        talk.choose_archive_if_asked();
        talk.press(&ChipReply::Once);
        talk.press(&ChipReply::ModeMove);
        talk.plan()
            .entries
            .iter()
            .filter_map(|entry| match &entry.outcome {
                TrialOutcome::Filed { to } => Some(to.clone()),
                _ => None,
            })
            .collect()
    }

    fn assert_sparkasse_has_months(filed: &[PathBuf]) {
        let statements: Vec<_> = filed
            .iter()
            .filter(|to| to.starts_with("Bank/Sparkasse Chemnitz/Kontoauszüge"))
            .collect();
        assert_eq!(statements.len(), MONTHLY_STATEMENTS);
        for to in statements {
            let parts: Vec<_> = to.iter().map(|part| part.to_string_lossy()).collect();
            let (year, month) = (&parts[3], &parts[4]);
            assert!(year.len() == 4 && month.len() == 2, "{to:?}");
            assert!(
                parts[5].contains(&format!("{year}-{month}-")),
                "the month folder is the month of the date in the name: {to:?}"
            );
        }
    }

    fn assert_others_keep_their_path(filed: &[PathBuf]) {
        let others: Vec<_> = filed
            .iter()
            .filter(|to| !to.starts_with("Bank/Sparkasse Chemnitz/Kontoauszüge"))
            .collect();
        assert!(!others.is_empty());
        for to in others {
            let parts: Vec<_> = to.iter().map(|part| part.to_string_lossy()).collect();
            assert!(
                parts.get(3).is_none_or(|part| part.len() != 2),
                "no month folder outside the statements: {to:?}"
            );
        }
    }

    #[test]
    fn bank_statements_by_month_for_all_banks_change_the_trial_of_the_session() {
        let mut talk = archive_talk();
        ask_statements_by_month(&mut talk);
        assert!(
            talk.state.session.changes.is_empty(),
            "nothing before the press"
        );
        talk.press(&ChipReply::ScopeAll);
        assert_eq!(
            talk.last_texts(),
            vec![
                say(Msg::HermesChatChangeDone, &[]),
                say(Msg::HermesChatAnythingElse, &[]),
            ]
        );
        let filed = trial_after_change(&mut talk);
        assert_sparkasse_has_months(&filed);
        assert_others_keep_their_path(&filed);
    }

    #[test]
    fn bank_statements_by_month_for_one_bank_change_only_that_bank() {
        let mut talk = archive_talk();
        ask_statements_by_month(&mut talk);
        talk.press(&ChipReply::ScopeOnly("Sparkasse Chemnitz".to_owned()));
        assert_eq!(
            talk.state.session.changes[0].only,
            Some((1, "Sparkasse Chemnitz".to_owned()))
        );
        let filed = trial_after_change(&mut talk);
        assert_sparkasse_has_months(&filed);
        assert_others_keep_their_path(&filed);
    }
}
