//! The paths of the chat the test archive does not reach, on small folders
//! built by hand, driven through `update`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

#[cfg(test)]
mod tests {

    use crate::common::{letter, say, Talk};
    use ncrs_hermes_chat::{Action, Availability, ChipReply, Entry, Message, Part, RowOutcome};
    use ncrs_i18n::Msg;
    use ncrs_index::BatchKind;

    /// An archive that files Alfa by year and holds Beta in two places, and an
    /// inbox with one letter of each.
    fn small_talk() -> Talk {
        let talk = Talk::in_dir(tempfile::tempdir().unwrap(), &["archive"], &["inbox"]);
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
        for (folder, base) in [("Privat", 20), ("Firma", 30)] {
            for month in [2, 5, 8] {
                talk.write(
                    &format!("archive/{folder}/Beta/2020/{base}{month}.pdf"),
                    &letter(
                        "Beta GmbH",
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
        talk
    }

    const TIDY_INBOX: &str = r#"{"intent":"tidy_folder","source":"Inbox"}"#;

    fn tidy_until_once(talk: &mut Talk) {
        talk.type_text_understood("Räum Inbox auf", TIDY_INBOX);
        talk.press(&ChipReply::ConfirmTidy);
        talk.choose_archive_if_asked();
    }

    fn tidy_until_trial(talk: &mut Talk, transfer: &ChipReply) {
        tidy_until_once(talk);
        talk.press(&ChipReply::Once);
        talk.press(transfer);
    }

    fn user_bubbles(talk: &Talk) -> Vec<&str> {
        talk.state
            .thread()
            .iter()
            .filter_map(|entry| match entry {
                Entry::User(text) => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_unsure_file_is_decided_in_its_row_and_filed_where_the_user_chose() {
        let mut talk = small_talk();
        talk.open();
        tidy_until_trial(&mut talk, &ChipReply::ModeMove);
        let plan = talk.plan();
        assert_eq!((plan.filed, plan.unsure), (1, 1), "{:?}", plan.entries);
        assert_eq!(
            talk.labels()[0],
            say(Msg::HermesChatFileAll, &[("count", "1")])
        );

        let Some(Part::Trial(card)) = talk.last_parts().get(1).cloned() else {
            panic!("{:?}", talk.last_parts())
        };
        let row = card
            .rows
            .iter()
            .find(|row| row.name == "neu_beta.pdf")
            .unwrap();
        let RowOutcome::Unsure {
            question,
            choices,
            chosen,
        } = &row.outcome
        else {
            panic!("{row:?}")
        };
        assert_eq!(chosen, &None);
        assert_eq!(choices.len(), 2);
        assert_eq!(
            question,
            &say(
                Msg::HermesChatTrialUnsure,
                &[
                    ("kind", "Invoice"),
                    ("first", &choices[0].label),
                    ("second", &choices[1].label)
                ]
            )
        );

        let picked = choices[1].clone();
        talk.entries += 1;
        talk.send(Message::ChipPressed {
            label: picked.label.clone(),
            reply: picked.reply.clone(),
        });
        assert_eq!(
            user_bubbles(&talk).last().copied(),
            Some(format!("neu_beta.pdf: {}", picked.label).as_str())
        );
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatUnsureChosen,
                &[("name", "neu_beta.pdf"), ("path", &picked.label)]
            )]
        );
        assert_eq!(
            talk.labels()[0],
            say(Msg::HermesChatFileAll, &[("count", "2")])
        );
        let marked = talk.state.thread().iter().any(|entry| {
            matches!(entry, Entry::Hermes { parts, .. } if parts.iter().any(|part| matches!(part,
            Part::Trial(shown) if shown.rows.iter().any(|listed| matches!(
                listed.outcome, RowOutcome::Unsure { chosen: Some(1), .. })))))
        });
        assert!(marked, "the row shows the choice");

        talk.press(&ChipReply::FileAll);
        let files = talk.files();
        let chosen_folder = picked.label.replace('\\', "/");
        assert!(files.contains_key(&format!("archive/{chosen_folder}/neu_beta.pdf")));
        assert!(!files.contains_key("inbox/neu_beta.pdf"));
        assert!(!files.contains_key("inbox/neu_alfa.pdf"));
        assert_eq!(
            talk.last_texts()[0],
            say(
                Msg::HermesChatFiled,
                &[("done", "2"), ("stays", "0"), ("folder", "inbox")]
            )
        );
    }

    #[test]
    fn more_of_the_trial_comes_ten_at_a_time_until_the_end() {
        let mut talk = small_talk();
        for number in 0..23 {
            talk.write(&format!("inbox/junk{number:02}.txt"), "nothing to see");
        }
        talk.open();
        tidy_until_trial(&mut talk, &ChipReply::ModeCopy);
        assert_eq!(talk.plan().entries.len(), 25);
        // The junk has no sender and no kind: 23 single pieces are asked about.
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatSinglePieces, &[("count", "23")])]
        );
        talk.leave_stay_questions();
        for expected in ["10", "5"] {
            talk.press(&ChipReply::ShowMore);
            assert_eq!(
                talk.last_texts(),
                vec![say(Msg::HermesChatTrialNext, &[("count", expected)])]
            );
        }
        talk.press(&ChipReply::ShowMore);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatTrialEnd, &[])]);
        assert_eq!(talk.labels().len(), 3, "file, more, cancel stay");
    }

    #[test]
    fn show_all_lists_every_area_and_asks_again() {
        let mut talk = small_talk();
        for area in ["Gamma", "Delta", "Epsilon"] {
            talk.write(
                &format!("archive/{area}/2020/2020-04-04 {area} Rechnung.pdf"),
                &letter(&format!("{area} AG"), "04.04.2020", "Rechnung", 5),
            );
        }
        talk.open();
        talk.press(&ChipReply::Folder(talk.path("archive")));
        let Part::Areas(first) = talk.last_parts()[1].clone() else {
            panic!()
        };
        talk.press(&ChipReply::ShowAll);
        let Part::Areas(all) = talk.last_parts()[0].clone() else {
            panic!()
        };
        assert_eq!((first.rows.len(), all.rows.len()), (4, 6));
        assert!(first.more.is_some());
        assert!(all.more.is_none());
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatAskKeep, &[])]);
        assert!(!talk
            .state
            .chips()
            .iter()
            .any(|chip| chip.reply == ChipReply::ShowAll));
    }

    #[test]
    fn always_on_a_source_the_app_already_watches_asks_for_the_mode_and_changes_no_setting() {
        let mut talk = small_talk();
        tidy_until_once(&mut talk);
        talk.press(&ChipReply::Always);
        assert_eq!(
            talk.last_texts(),
            vec![
                say(Msg::HermesChatAskMode, &[("folder", "inbox")]),
                say(Msg::HermesChatModeConsequence, &[("folder", "inbox")])
            ]
        );
        assert!(talk.actions.is_empty(), "{:?}", talk.actions);
        talk.press(&ChipReply::ModeMove);
        assert!(talk.state.trial_plan().is_some());
    }

    #[test]
    fn always_makes_a_source_that_is_not_watched_yet_a_watched_one() {
        let mut talk = Talk::in_dir(tempfile::tempdir().unwrap(), &["archive"], &[]);
        talk.write(
            "archive/Alfa/2020/2020-01-10 Alfa Rechnung.pdf",
            &letter("Alfa AG", "10.01.2020", "Rechnung", 10),
        );
        talk.write(
            "downloads/neu_alfa.pdf",
            &letter("Alfa AG", "05.03.2021", "Rechnung", 77),
        );
        let downloads = talk.path("downloads");
        talk.open();
        talk.send(Message::FolderPicked(Some(downloads.clone())));
        talk.press(&ChipReply::RoleSource);
        talk.actions.clear();
        talk.press(&ChipReply::Folder(talk.path("archive")));
        talk.press(&ChipReply::Always);
        assert_eq!(
            talk.actions,
            vec![Action::SetFolderRole {
                folder: downloads,
                role: ncrs_ui_kit::hermes_state::FolderRole::Inbox,
            }]
        );
    }

    #[test]
    fn cancel_leaves_everything_and_no_run_behind() {
        let mut talk = small_talk();
        let before = talk.files();
        tidy_until_trial(&mut talk, &ChipReply::ModeMove);
        talk.press(&ChipReply::Cancel);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatCancelled, &[])]);
        assert_eq!(talk.files(), before);
        let guard = talk.index.lock().unwrap();
        let batch = guard
            .as_ref()
            .unwrap()
            .last_undoable_batch(BatchKind::Hermes)
            .unwrap();
        assert_eq!(batch, None);
    }

    #[test]
    fn search_and_shell_and_unknown_sentences_run_nothing() {
        let mut talk = small_talk();
        let before = talk.files();

        talk.type_text_understood(
            "such meine Rechnung",
            r#"{"intent":"search","text":"Rechnung"}"#,
        );
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatSearchNotYet, &[])]
        );
        talk.press(&ChipReply::OpenSearch);
        assert_eq!(talk.actions, vec![Action::OpenSearch]);

        talk.type_text_understood("hmmmm wie geht das", r#"{"intent":"other"}"#);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatFallback, &[])]);
        assert_eq!(
            talk.labels(),
            vec![
                say(Msg::HermesChatIntentSearch, &[]),
                say(Msg::HermesChatIntentTidy, &[]),
                say(Msg::HermesChatIntentStructure, &[]),
                say(Msg::HermesChatIntentUndo, &[]),
            ]
        );
        assert!(talk.state.pending_work().is_none());
        assert_eq!(talk.files(), before);
    }

    #[test]
    fn without_a_filing_the_chat_asks_for_setup_or_waits() {
        let mut talk = small_talk();
        talk.ctx.availability = Availability::Off;
        talk.open();
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatOff, &[])]);
        talk.press(&ChipReply::TurnOn);
        assert_eq!(talk.actions, vec![Action::TurnOnHermes]);

        let mut waiting = small_talk();
        waiting.ctx.availability = Availability::Starting;
        waiting.open();
        assert_eq!(waiting.last_texts(), greeting());
        assert!(waiting
            .state
            .chips()
            .iter()
            .any(|chip| chip.reply == ChipReply::PickFolder));
    }

    fn working_line(talk: &Talk) -> Option<&str> {
        match talk.state.thread().last() {
            Some(Entry::Working(line)) => Some(line),
            _ => None,
        }
    }

    #[test]
    fn a_folder_chip_pressed_while_starting_waits_and_then_reads() {
        let mut talk = small_talk();
        talk.ctx.availability = Availability::Starting;
        talk.open();
        let archive = ChipReply::Folder(talk.path("archive"));
        talk.press(&archive);
        assert_eq!(
            working_line(&talk),
            Some(say(Msg::HermesChatStartingWait, &[]).as_str())
        );
        assert!(talk.state.pending_work().is_none());
        ready(&mut talk);
        assert!(talk
            .state
            .chips()
            .iter()
            .any(|chip| chip.reply == ChipReply::KeepYes));
        assert!(!talk.state.waits_for_hermes());
    }

    #[test]
    fn a_work_tick_shows_the_counts_of_the_running_read() {
        let mut talk = small_talk();
        talk.open();
        let archive = ChipReply::Folder(talk.path("archive"));
        let (_task, _action) = ncrs_hermes_chat::update(
            &mut talk.state,
            Message::ChipPressed {
                label: "archive".to_owned(),
                reply: archive,
            },
            &talk.ctx,
        );
        let Some(ncrs_filing::chat_work::ChatWork::Read { progress, .. }) =
            talk.state.pending_work().cloned()
        else {
            panic!("{:?}", talk.state.pending_work())
        };
        progress
            .found
            .store(1234, std::sync::atomic::Ordering::Relaxed);
        let tick = |talk_in_tick: &mut Talk| {
            let (_tick_task, _tick_action) = ncrs_hermes_chat::update(
                &mut talk_in_tick.state,
                Message::WorkTick,
                &talk_in_tick.ctx,
            );
            working_line(talk_in_tick).unwrap().to_owned()
        };
        let first = tick(&mut talk);
        assert!(first.contains("1234"), "{first}");
        progress
            .total
            .store(2516, std::sync::atomic::Ordering::Relaxed);
        progress
            .done
            .store(800, std::sync::atomic::Ordering::Relaxed);
        let second = tick(&mut talk);
        assert!(
            second.contains("800") && second.contains("2516"),
            "{second}"
        );
        assert_ne!(first.matches('.').count(), second.matches('.').count());
    }

    #[test]
    fn a_failed_reading_ends_the_wait_and_offers_the_fallback_chips() {
        let mut talk = small_talk();
        talk.open();
        let archive = ChipReply::Folder(talk.path("archive"));
        let (_task, _action) = ncrs_hermes_chat::update(
            &mut talk.state,
            Message::ChipPressed {
                label: "archive".to_owned(),
                reply: archive,
            },
            &talk.ctx,
        );
        assert!(talk.state.pending_work().is_some());
        talk.send(Message::Failed("volume gone".to_owned()));
        assert!(talk.state.pending_work().is_none());
        assert_eq!(
            talk.labels(),
            vec![
                say(Msg::HermesChatIntentSearch, &[]),
                say(Msg::HermesChatIntentTidy, &[]),
                say(Msg::HermesChatIntentStructure, &[]),
                say(Msg::HermesChatIntentUndo, &[]),
            ]
        );
        talk.type_text_understood("hmmmm wie geht das", r#"{"intent":"other"}"#);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatFallback, &[])]);
    }

    fn greeting() -> Vec<String> {
        vec![say(Msg::HermesChatGreeting, &[])]
    }

    fn ready(talk: &mut Talk) {
        let access = talk.access.clone();
        talk.ctx.availability = Availability::Ready(Box::new(access));
        talk.send(Message::HermesReady);
    }

    #[test]
    fn opened_while_starting_the_greeting_follows_when_hermes_is_ready() {
        let mut talk = small_talk();
        talk.ctx.availability = Availability::Starting;
        talk.open();
        assert_eq!(talk.last_texts(), greeting());
        ready(&mut talk);
        ready(&mut talk);
        assert_eq!(talk.state.thread().len(), 1);
    }

    #[test]
    fn closed_while_starting_the_greeting_comes_when_opened_ready() {
        let mut talk = small_talk();
        talk.ctx.availability = Availability::Starting;
        talk.open();
        talk.send(Message::Close);
        let access = talk.access.clone();
        talk.ctx.availability = Availability::Ready(Box::new(access));
        talk.open();
        assert_eq!(talk.last_texts(), greeting());
    }

    #[test]
    fn a_sentence_typed_while_starting_gets_the_setup_answer_when_the_start_fails() {
        let mut talk = small_talk();
        talk.ctx.availability = Availability::Starting;
        talk.open();
        talk.type_text("hmmmm wie geht das");
        assert!(talk.state.waits_for_hermes());
        talk.ctx.availability = Availability::Off;
        talk.send(Message::HermesReady);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatOff, &[])]);
        assert_eq!(talk.labels(), vec![say(Msg::HermesChatTurnOn, &[])]);
        assert!(!talk.state.waits_for_hermes());
        assert!(talk.state.pending_work().is_none());
    }

    #[test]
    fn a_start_that_failed_says_why_and_a_retry_restarts_hermes() {
        let mut talk = small_talk();
        talk.ctx.availability = Availability::Starting;
        talk.open();
        talk.type_text("hmmmm wie geht das");
        talk.ctx.availability = Availability::Failed("no key".to_owned());
        talk.send(Message::HermesReady);
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatStartFailed, &[("reason", "no key")])]
        );
        assert_eq!(talk.labels(), vec![say(Msg::HermesChatRetry, &[])]);
        assert!(!talk.state.waits_for_hermes());
        talk.press(&ChipReply::TurnOn);
        assert_eq!(talk.actions, vec![Action::TurnOnHermes]);
    }

    #[test]
    fn a_failed_switching_on_says_the_reason_and_offers_it_again() {
        let mut talk = small_talk();
        talk.ctx.availability = Availability::Starting;
        talk.open();
        talk.send(Message::TurnOnFailed("disk full".to_owned()));
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatTurnOnFailed, &[("reason", "disk full")])]
        );
        assert!(!talk.state.waits_for_hermes());
        talk.press(&ChipReply::TurnOn);
        assert_eq!(talk.actions, vec![Action::TurnOnHermes]);
    }

    #[test]
    fn a_sentence_typed_while_starting_is_answered_when_hermes_is_ready() {
        let mut talk = small_talk();
        talk.ctx.availability = Availability::Starting;
        talk.open();
        talk.type_text("hmmmm wie geht das");
        assert_eq!(
            working_line(&talk),
            Some(say(Msg::HermesChatStartingWait, &[]).as_str())
        );
        let access = talk.access.clone();
        talk.ctx.availability = Availability::Ready(Box::new(access));
        talk.model.push_reply(Ok(r#"{"intent":"other"}"#));
        talk.send(Message::HermesReady);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatFallback, &[])]);
    }

    #[test]
    fn a_source_inside_a_target_is_not_tidied_into_itself() {
        let mut talk = small_talk();
        let archive = talk.path("archive");
        let tidy = |path: &std::path::Path| {
            format!(
                r#"{{"intent":"tidy_folder","source":"{}"}}"#,
                path.display().to_string().replace('\\', "\\\\")
            )
        };
        talk.type_text_understood(&format!("Räum {} auf", archive.display()), &tidy(&archive));
        talk.press(&ChipReply::ConfirmTidy);
        talk.choose_archive_if_asked();
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatSourceIsTarget, &[("folder", "archive")])]
        );

        let inside = archive.join("Alfa");
        talk.type_text_understood(&format!("Räum {} auf", inside.display()), &tidy(&inside));
        talk.press(&ChipReply::ConfirmTidy);
        talk.choose_archive_if_asked();
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatSourceIsTarget, &[("folder", "Alfa")])]
        );
        assert!(talk.state.pending_work().is_none());
    }

    #[test]
    fn a_typed_folder_nobody_knows_is_confirmed_before_it_is_read() {
        let mut talk = small_talk();
        talk.write("elsewhere/x.txt", "x");
        let elsewhere = talk.path("elsewhere");
        talk.open();
        talk.type_text(&elsewhere.to_string_lossy());
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatConfirmTypedFolder,
                &[("name", "elsewhere")]
            )]
        );
        assert!(talk.state.pending_work().is_none());
        talk.press(&ChipReply::Cancel);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatCancelled, &[])]);
    }

    fn canonical(path: &std::path::Path) -> String {
        std::fs::canonicalize(path)
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    /// A talk that waits for the source folder, with `b` an existing folder
    /// and the typed path `a/../b` pointing at it.
    fn asking_for_the_source() -> (Talk, String) {
        let mut talk = small_talk();
        std::fs::create_dir_all(talk.path("a")).unwrap();
        std::fs::create_dir_all(talk.path("b")).unwrap();
        talk.send(Message::Open);
        talk.press(&ChipReply::Folder(talk.path("archive")));
        talk.press(&ChipReply::KeepYes);
        let typed = talk.path("a").join("..").join("b");
        (talk, typed.to_string_lossy().into_owned())
    }

    #[test]
    fn a_typed_source_is_resolved_and_asked_before_anything_is_set() {
        let (mut talk, typed) = asking_for_the_source();
        talk.type_text(&typed);
        let real = canonical(&talk.path("b"));
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatConfirmTypedFolder, &[("name", "b")])]
        );
        assert!(talk.actions.is_empty());
        assert!(talk.state.pending_work().is_none());

        talk.press(&ChipReply::ReadAnyway);
        assert_eq!(
            talk.actions,
            vec![Action::SetFolderRole {
                folder: std::path::PathBuf::from(&real),
                role: ncrs_ui_kit::hermes_state::FolderRole::Inbox,
            }]
        );
    }

    #[test]
    fn cancelling_the_typed_source_sets_nothing() {
        let (mut talk, typed) = asking_for_the_source();
        talk.type_text(&typed);
        talk.press(&ChipReply::Cancel);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatCancelled, &[])]);
        assert!(talk.actions.is_empty());
        assert!(talk.state.pending_work().is_none());
        assert!(talk.state.pending_resolve().is_none());
    }

    #[test]
    fn a_typed_missing_folder_is_not_a_folder_and_sets_nothing() {
        let (mut talk, _) = asking_for_the_source();
        let missing = talk.path("nowhere").to_string_lossy().into_owned();
        talk.type_text(&missing);
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatNotAFolder, &[("name", "nowhere")])]
        );
        assert!(talk.actions.is_empty());
        assert!(talk.state.pending_work().is_none());
    }

    #[test]
    fn a_typed_other_target_is_asked_before_the_proposal_changes() {
        let mut talk = small_talk();
        std::fs::create_dir_all(talk.path("b")).unwrap();
        talk.type_text_understood("Räum Inbox auf", TIDY_INBOX);
        talk.press(&ChipReply::OtherTarget);
        let typed = talk.path("b").to_string_lossy().into_owned();
        talk.type_text(&typed);
        let real = canonical(&talk.path("b"));
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatConfirmTypedFolder, &[("name", "b")])]
        );
        let target = |asked: &Talk| {
            asked
                .state
                .session
                .proposal
                .as_ref()
                .unwrap()
                .target
                .clone()
        };
        assert_eq!(target(&talk), None, "not set before the yes");
        talk.press(&ChipReply::ReadAnyway);
        assert_eq!(target(&talk), Some(std::path::PathBuf::from(real)));
    }

    fn tidy_sentence(talk: &mut Talk, name: &str) {
        talk.type_text_understood(
            &format!("Räum {name} auf"),
            &format!(r#"{{"intent":"tidy_folder","source":"{name}"}}"#),
        );
    }

    #[test]
    fn a_short_name_for_one_folder_is_asked_with_its_full_path_before_it_is_read() {
        let mut talk = small_talk();
        talk.write("home/Downloads/x.txt", "x");
        tidy_sentence(&mut talk, "Downloads");
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatConfirmTidy,
                &[("source", "Downloads"), ("mode", "")]
            )]
        );
        assert!(talk.actions.is_empty());
        assert!(talk.state.pending_work().is_none());

        talk.press(&ChipReply::ConfirmTidy);
        talk.choose_archive_if_asked();
        assert!(
            talk.last_texts().iter().all(|sentence| *sentence
                != say(Msg::HermesChatUnknownPathConfirm, &[("name", "Downloads")])),
            "nothing asks a second time"
        );
        assert!(talk.state.pending_naming().is_none());
    }

    #[test]
    fn an_unknown_name_is_not_found_and_leaves_no_proposal() {
        let mut talk = small_talk();
        tidy_sentence(&mut talk, "Nirgendwo");
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatNotAFolder, &[("name", "Nirgendwo")])]
        );
        assert!(talk.state.session.proposal.is_none());
        assert!(talk.actions.is_empty());
    }

    #[test]
    fn a_name_that_fits_two_folders_asks_which_and_goes_on_with_the_chosen() {
        let mut talk = small_talk();
        talk.write("home/Belege/a.txt", "a");
        talk.write("elsewhere/Belege/b.txt", "b");
        talk.ctx.favourites = vec![talk.path("elsewhere/Belege")];
        tidy_sentence(&mut talk, "Belege");
        let own = canonical(&talk.path("home/Belege"));
        let favourite = canonical(&talk.path("elsewhere/Belege"));
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatWhichFolder, &[("name", "Belege")])]
        );
        let labels = talk.labels();
        assert!(
            labels.contains(&own) && labels.contains(&favourite),
            "{labels:?}"
        );
        assert!(talk.state.session.proposal.is_none());

        talk.press(&ChipReply::NamedFolder(std::path::PathBuf::from(
            &favourite,
        )));
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatConfirmTidy,
                &[("source", "Belege"), ("mode", "")]
            )]
        );
        assert!(talk.actions.is_empty());
    }

    #[test]
    fn a_refused_rule_says_what_does_not_fit() {
        use ncrs_filing::chat_work::{ChatResult, RuleSaveFailure};
        use ncrs_intake::{ConditionProblem, RuleProblem};

        let mut talk = small_talk();
        talk.send(Message::Done(Box::new(ChatResult::RuleSaved(Err(
            RuleSaveFailure::Refused(RuleProblem::Condition(ConditionProblem::Empty)),
        )))));
        let reason = say(Msg::HermesChatRuleProblemEmptyValue, &[]);
        assert!(talk
            .state
            .thread()
            .iter()
            .any(|entry| matches!(entry, Entry::Hermes { parts, .. }
                if parts.contains(&Part::Text(say(Msg::HermesChatRuleRefused, &[("reason", &reason)]))))));
    }

    #[test]
    fn a_found_pattern_is_previewed_on_one_file_and_adopted_for_the_session() {
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
            talk.write(
                &format!("archive/Beta/2020/{month:02}/2020-{month:02}-{day} Beta Rechnung.pdf"),
                &letter(
                    "Beta GmbH",
                    &format!("{day}.{month:02}.2020"),
                    "Rechnung",
                    day + 50,
                ),
            );
        }
        talk.open();
        talk.press(&ChipReply::Folder(talk.path("archive")));
        talk.press(&ChipReply::WantChange);
        let offered: Vec<ChipReply> = talk
            .state
            .chips()
            .iter()
            .map(|chip| chip.reply.clone())
            .filter(|reply| matches!(reply, ChipReply::AdoptPattern(_)))
            .collect();
        assert_eq!(offered.len(), 2, "{:?}", talk.labels());

        // The pattern with the month folder is the one only Beta has.
        let with_month = offered
            .iter()
            .find(|reply| {
                talk.state
                    .session
                    .patterns
                    .get(match reply {
                        ChipReply::AdoptPattern(index) => *index,
                        _ => 0,
                    })
                    .is_some_and(|choice| choice.levels.len() == 2)
            })
            .unwrap()
            .clone();
        talk.press(&with_month);
        let parts = talk.last_parts();
        let cards: Vec<&ncrs_hermes_chat::FileCard> = parts
            .iter()
            .filter_map(|part| match part {
                Part::File(card) => Some(card),
                _ => None,
            })
            .collect();
        assert_eq!(cards.len(), 2, "{parts:?}");
        assert_eq!(cards[0].segments.len(), 3);
        assert_eq!(
            cards[1].segments[..3],
            ["Alfa", "2020", "11"].map(String::from)[..]
        );
        assert!(
            talk.state.session.changes.is_empty(),
            "nothing before the yes"
        );

        talk.press(&ChipReply::ConfirmChange);
        assert_eq!(talk.state.session.changes.len(), 1);
    }
}
