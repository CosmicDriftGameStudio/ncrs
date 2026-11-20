//! The paths of the chat the test archive does not reach, on small folders
//! built by hand, driven through `update`.

mod common;

#[cfg(test)]
mod tests {

    use crate::common::{letter, say, Talk};
    use ncrs_filing::chat_work::ChatResult;
    use ncrs_hermes_chat::{Action, Availability, ChipReply, Entry, Message, Part};
    use ncrs_i18n::Msg;

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

    #[test]
    fn show_all_lists_every_area_and_asks_again() {
        let mut talk = small_talk();
        for area in ["Gamma", "Delta", "Epsilon"] {
            talk.write(
                &format!("elsewhere/{area}/2020/2020-04-04 {area} Rechnung.pdf"),
                &letter(&format!("{area} AG"), "04.04.2020", "Rechnung", 5),
            );
        }
        talk.write(
            "elsewhere/Alfa/2020/2020-01-10.pdf",
            &letter("Alfa AG", "10.01.2020", "Rechnung", 1),
        );
        talk.write(
            "elsewhere/Beta/2020/2020-01-10.pdf",
            &letter("Beta AG", "10.01.2020", "Rechnung", 2),
        );
        talk.write(
            "elsewhere/Zeta/2020/2020-01-10.pdf",
            &letter("Zeta AG", "10.01.2020", "Rechnung", 3),
        );
        talk.open();
        let elsewhere = talk.path("elsewhere");
        talk.send(Message::ChangeArea {
            folder: elsewhere,
            area: "Alfa".to_owned(),
        });
        let Part::Areas(first) = talk.last_parts()[1].clone() else {
            panic!("{:?}", talk.last_parts())
        };
        talk.press(&ChipReply::ShowAll);
        let Part::Areas(all) = talk.last_parts()[0].clone() else {
            panic!()
        };
        assert!(first.rows.len() < all.rows.len());
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
    fn always_on_a_source_the_app_already_watches_hands_over_and_changes_no_setting() {
        let mut talk = small_talk();
        tidy_until_once(&mut talk);
        talk.press(&ChipReply::Always);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatHandover, &[])]);
        assert!(
            matches!(
                talk.actions.as_slice(),
                [Action::OpenFiling {
                    watch_source: false,
                    ..
                }]
            ),
            "{:?}",
            talk.actions
        );
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
        assert!(
            matches!(
                talk.actions.as_slice(),
                [Action::OpenFiling { source, watch_source: true, .. }] if *source == downloads
            ),
            "{:?}",
            talk.actions
        );
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
        let alfa = ChipReply::Folder(talk.path("archive/Alfa"));
        assert!(talk.state.chips().iter().any(|chip| chip.reply == alfa));
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
    fn a_pressed_start_is_answered_whether_hermes_comes_or_not() {
        let mut talk = small_talk();
        talk.ctx.availability = Availability::Off;
        talk.open();

        talk.press(&ChipReply::TurnOn);
        assert_eq!(talk.actions, vec![Action::TurnOnHermes]);
        assert!(talk.state.waiting_for_hermes);
        talk.ctx.availability = Availability::Failed("no key".to_owned());
        talk.send(Message::HermesReady);
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatStartFailed, &[("reason", "no key")])]
        );
        assert_eq!(talk.labels(), vec![say(Msg::HermesChatRetry, &[])]);
        let still_waiting = |asked: &Talk| {
            asked
                .state
                .thread()
                .iter()
                .any(|entry| matches!(entry, Entry::Working(_)))
        };
        assert!(!still_waiting(&talk));

        talk.press(&ChipReply::TurnOn);
        assert!(talk.state.waiting_for_hermes);
        let access = talk.access.clone();
        talk.ctx.availability = Availability::Ready(Box::new(access));
        talk.send(Message::HermesReady);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatGreeting, &[])]);
        assert!(!still_waiting(&talk));
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
    fn a_source_inside_a_target_is_opened_not_tidied_into_itself() {
        let mut talk = small_talk();
        let archive = talk.path("archive");
        let tidy = |path: &std::path::Path| {
            format!(
                r#"{{"intent":"tidy_folder","source":"{}"}}"#,
                path.display().to_string().replace('\\', "\\\\")
            )
        };
        talk.type_text_understood(&format!("Räum {} auf", archive.display()), &tidy(&archive));
        let alfa = ChipReply::Folder(archive.join("Alfa"));
        assert!(talk.state.chips().iter().any(|chip| chip.reply == alfa));
        assert!(!talk
            .state
            .chips()
            .iter()
            .any(|chip| chip.reply == ChipReply::ConfirmTidy));

        let inside = archive.join("Alfa");
        talk.type_text_understood(&format!("Räum {} auf", inside.display()), &tidy(&inside));
        assert!(!talk
            .state
            .chips()
            .iter()
            .any(|chip| chip.reply == ChipReply::ConfirmTidy));
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
        let typed = talk.path("a").join("..").join("b");
        (talk, typed.to_string_lossy().into_owned())
    }

    #[test]
    fn a_typed_source_is_resolved_and_asked_before_anything_is_set() {
        let (mut talk, typed) = asking_for_the_source();
        talk.type_text(&typed);
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatConfirmTypedFolder, &[("name", "b")])]
        );
        assert!(talk.actions.is_empty());
        assert!(talk.state.pending_work().is_none());

        talk.press(&ChipReply::ReadAnyway);
        assert!(!talk
            .state
            .chips()
            .iter()
            .any(|chip| chip.reply == ChipReply::ReadAnyway));
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
    fn retrying_a_typed_folder_that_was_missing_resolves_and_confirms_it_again() {
        for asking in [ChipReply::IntentTidy, ChipReply::IntentStructure] {
            let (mut talk, _) = asking_for_the_source();
            talk.type_text_understood("hmmmm", r#"{"intent":"other"}"#);
            talk.press(&asking);
            let typed = talk.path("a").join("..").join("later");
            talk.type_text(&typed.to_string_lossy());
            assert_eq!(
                talk.last_texts(),
                vec![say(Msg::HermesChatNotAFolder, &[("name", "later")])]
            );

            std::fs::create_dir_all(talk.path("later")).unwrap();
            talk.press(&ChipReply::Retry);

            assert_eq!(
                talk.last_texts(),
                vec![say(Msg::HermesChatConfirmTypedFolder, &[("name", "later")])],
                "{asking:?}"
            );
            assert!(talk.state.pending_work().is_none());
        }
    }

    #[test]
    fn retrying_a_folder_that_could_not_be_read_reads_it_again() {
        let mut talk = small_talk();
        talk.open();
        talk.type_text_understood("hmmmm", r#"{"intent":"other"}"#);
        talk.press(&ChipReply::IntentStructure);
        let gone = talk.path("gone");
        talk.send(Message::ChipPressed {
            label: "gone".to_owned(),
            reply: ChipReply::Folder(gone.clone()),
        });
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatFolderGone,
                &[("path", &gone.to_string_lossy())]
            )]
        );

        std::fs::create_dir_all(gone.join("Alfa")).unwrap();
        talk.write(
            "gone/Alfa/x.pdf",
            &letter("Alfa AG", "01.01.2020", "Rechnung", 1),
        );
        talk.press(&ChipReply::Retry);

        assert!(talk
            .state
            .session
            .readings
            .contains_key(&canonical_path(&gone)));
        assert!(talk
            .state
            .chips()
            .iter()
            .all(|chip| chip.reply != ChipReply::Retry));
    }

    fn canonical_path(path: &std::path::Path) -> std::path::PathBuf {
        std::fs::canonicalize(path).unwrap()
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

    fn plan_the_model_failed_on(failure: ncrs_ai::AiError) -> ncrs_filing::chat_work::TrialPlan {
        ncrs_filing::chat_work::TrialPlan {
            source_root: "/inbox".into(),
            target_root: "/archive".into(),
            entries: Vec::new(),
            filed: 0,
            unsure: 0,
            already_there: 0,
            stays: 0,
            taken: std::collections::BTreeSet::new(),
            groups: ncrs_filing::chat_work::StayGroups::default(),
            model_left: std::collections::BTreeSet::new(),
            model_failure: Some(failure),
        }
    }

    fn times_said(talk: &Talk, sentence: &str) -> usize {
        talk.state
            .thread
            .iter()
            .filter(|entry| {
                matches!(entry, Entry::Hermes { parts, .. }
                    if parts.iter().any(|part| matches!(part, Part::Text(text) if text == sentence)))
            })
            .count()
    }

    #[test]
    fn a_model_that_cannot_choose_folders_is_said_once_and_the_trial_goes_on() {
        let mut talk = small_talk();

        talk.send(Message::Done(Box::new(ChatResult::Trial(Ok(Box::new(
            plan_the_model_failed_on(ncrs_ai::AiError::Unsupported),
        ))))));

        let hint = say(Msg::HermesChatModelUnsupported, &[]);
        assert_eq!(times_said(&talk, &hint), 1);
        assert!(talk.state.session.trial.is_some());
    }

    #[test]
    fn a_model_that_fails_otherwise_ends_the_trial_in_the_fallback() {
        let mut talk = small_talk();

        talk.send(Message::Done(Box::new(ChatResult::Trial(Ok(Box::new(
            plan_the_model_failed_on(ncrs_ai::AiError::Model("closed")),
        ))))));

        assert!(talk.state.session.trial.is_none());
        let hint = say(Msg::HermesChatModelUnsupported, &[]);
        assert_eq!(times_said(&talk, &hint), 0);
    }

    #[test]
    fn a_model_that_cannot_run_is_said_in_one_sentence_without_the_start_frame() {
        let mut talk = small_talk();
        talk.ctx.availability =
            Availability::ModelFailed(ncrs_ai::ModelProblem::NotLoaded("no memory"));

        talk.open();

        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatModelNotLoaded, &[])]
        );
    }
}
