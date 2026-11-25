//! The paths of the chat the test archive does not reach, on small folders
//! built by hand, driven through `update`.

mod common;

#[cfg(test)]
mod tests {

    use crate::common::{letter, say, Talk};
    use ncrs_ai::Levels;
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

    /// The next recognition answers with these levels, asked again or not.
    fn recognize(talk: &Talk, pattern: &str) {
        for _ in 0..2 {
            talk.model
                .push_levels(Ok(Levels::from_pattern(pattern).unwrap()));
        }
    }

    fn shows_levels(talk: &Talk) -> bool {
        talk.last_parts()
            .iter()
            .any(|part| matches!(part, Part::Levels(_)))
    }

    fn areas_cards(talk: &Talk) -> Vec<ncrs_hermes_chat::AreasCard> {
        talk.state
            .thread()
            .iter()
            .filter_map(|entry| match entry {
                Entry::Hermes { parts, .. } => Some(parts),
                _ => None,
            })
            .flatten()
            .filter_map(|part| match part {
                Part::Areas(card) => Some(card.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn show_all_lists_every_area_and_the_levels_again() {
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
        recognize(&talk, "{sender}/{year}/");
        talk.send(Message::FolderPicked(Some(talk.path("elsewhere"))));
        talk.press(&ChipReply::ShowAll);

        let cards = areas_cards(&talk);
        assert_eq!(cards.len(), 2, "{cards:?}");
        assert!(cards[0].rows.len() < cards[1].rows.len());
        assert!(cards[0].more.is_some());
        assert!(cards[1].more.is_none());
        assert!(shows_levels(&talk), "{:?}", talk.last_parts());
        assert!(!talk
            .state
            .chips()
            .iter()
            .any(|chip| chip.reply == ChipReply::ShowAll));
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
        recognize(&talk, "{year}/");
        let archive = ChipReply::Folder(talk.path("archive"));
        talk.press(&archive);
        assert_eq!(
            working_line(&talk),
            Some(say(Msg::HermesChatStartingWait, &[]).as_str())
        );
        assert!(talk.state.pending_work().is_none());
        ready(&mut talk);
        assert!(shows_levels(&talk), "{:?}", talk.last_parts());
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
        ncrs_intake::canonical_path(path)
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    /// A talk that waits for a folder, with `b` an existing folder
    /// and the typed path `a/../b` pointing at it.
    fn asking_for_a_folder() -> (Talk, String) {
        let mut talk = small_talk();
        std::fs::create_dir_all(talk.path("a")).unwrap();
        std::fs::create_dir_all(talk.path("b")).unwrap();
        talk.send(Message::Open);
        let typed = talk.path("a").join("..").join("b");
        (talk, typed.to_string_lossy().into_owned())
    }

    #[test]
    fn a_typed_folder_is_resolved_and_asked_about_before_it_is_read() {
        let (mut talk, typed) = asking_for_a_folder();
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
    fn cancelling_the_typed_folder_reads_nothing() {
        let (mut talk, typed) = asking_for_a_folder();
        talk.type_text(&typed);
        talk.press(&ChipReply::Cancel);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatCancelled, &[])]);
        assert!(talk.actions.is_empty());
        assert!(talk.state.pending_work().is_none());
        assert!(talk.state.pending_resolve().is_none());
    }

    #[test]
    fn a_typed_missing_folder_is_not_a_folder_and_is_not_read() {
        let (mut talk, _) = asking_for_a_folder();
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
        let (mut talk, _) = asking_for_a_folder();
        talk.type_text_understood("hmmmm", r#"{"intent":"other"}"#);
        talk.press(&ChipReply::IntentStructure);
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
            vec![say(Msg::HermesChatConfirmTypedFolder, &[("name", "later")])]
        );
        assert!(talk.state.pending_work().is_none());
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
        recognize(&talk, "Alfa/");
        talk.press(&ChipReply::Retry);

        assert!(shows_levels(&talk), "{:?}", talk.last_parts());
        assert!(talk
            .state
            .chips()
            .iter()
            .all(|chip| chip.reply != ChipReply::Retry));
    }

    #[test]
    fn an_unknown_name_to_open_is_not_found() {
        let mut talk = small_talk();
        talk.type_text_understood(
            "öffne Nirgendwo",
            r#"{"intent":"open_folder","folder":"Nirgendwo"}"#,
        );
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatNotAFolder, &[("name", "Nirgendwo")])]
        );
        assert!(talk.actions.is_empty());
    }

    #[test]
    fn a_name_that_fits_two_folders_asks_which_and_goes_on_with_the_chosen() {
        let mut talk = small_talk();
        talk.write("home/Belege/a.txt", "a");
        talk.write("elsewhere/Belege/b.txt", "b");
        talk.ctx.favourites = vec![talk.path("elsewhere/Belege")];
        talk.type_text_understood(
            "öffne Belege",
            r#"{"intent":"open_folder","folder":"Belege"}"#,
        );
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
        assert!(talk.works.is_empty(), "nothing is read before the choice");

        recognize(&talk, "{year}/");
        talk.press(&ChipReply::NamedFolder(std::path::PathBuf::from(
            &favourite,
        )));
        assert!(shows_levels(&talk), "{:?}", talk.last_parts());
        assert!(talk.actions.is_empty());
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
