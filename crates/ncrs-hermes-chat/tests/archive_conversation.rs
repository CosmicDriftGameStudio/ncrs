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
    use ncrs_filing::chat_work::{TrialOutcome, TrialPlan};
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
        talk.press(&ChipReply::Once);
        talk.press(transfer);
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

        // Conversation 2: the source from Hermes' question, one target, so no
        // question where to; then once, moving.
        talk.press(&ChipReply::Folder(talk.path("inbox")));
        assert_eq!(
            hermes_texts(&talk)
                .into_iter()
                .find(|text| text.starts_with("Gern!")),
            Some(say(
                Msg::HermesChatSourceSummaryOneTarget,
                &[
                    ("folder", "inbox"),
                    ("count", &count(INBOX_FILES)),
                    ("target", "archive")
                ]
            ))
        );
        talk.press(&ChipReply::Once);
        talk.press(&ChipReply::ModeMove);
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
        // Folder, keep, source, once, move, file: six entries of the user from
        // Open to the first filed file.
        assert_eq!(talk.entries, 6);
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
