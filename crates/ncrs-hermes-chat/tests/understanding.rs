//! What the chat does with typed sentences: they go to the (scripted) model,
//! Hermes asks before it acts, and a button press never reaches the model.

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
    use ncrs_ai::{AiError, Compute, LocalModelStatus, LOCAL_MODEL};
    use ncrs_filing::chat_work::ChangeOrder;
    use ncrs_hermes_chat::{Action, ChipReply, Entry, Message, Part};
    use ncrs_i18n::Msg;
    use ncrs_ui_kit::format;

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
        talk.write(
            "inbox/neu_alfa.pdf",
            &letter("Alfa AG", "05.03.2021", "Rechnung", 77),
        );
        talk
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

    fn loaded() -> LocalModelStatus {
        LocalModelStatus::Loaded {
            name: "test".to_owned(),
            memory_bytes: 1,
            compute: Compute::Cpu,
        }
    }

    #[test]
    fn a_typed_sentence_sends_one_turn_with_the_question_and_its_buttons() {
        let mut talk = small_talk();
        talk.open();
        let greeting = say(Msg::HermesChatGreeting, &[]);
        let buttons = talk.labels();
        talk.type_text_understood("ach nee", r#"{"intent":"other"}"#);

        let turns = talk.model.turns();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].message, "ach nee");
        let question = turns[0].question.clone().unwrap();
        assert_eq!(question.text, greeting);
        assert_eq!(question.options, buttons);
        assert_eq!(turns[0].previous_search, None);

        talk.press(&ChipReply::IntentSearch);
        assert_eq!(talk.model.turns().len(), 1, "a button never asks the model");
    }

    #[test]
    fn an_answer_presses_the_button_it_names() {
        let mut talk = small_talk();
        talk.open();
        let archive = talk.path("archive");
        let label = talk
            .state
            .chips()
            .iter()
            .find(|chip| chip.reply == ChipReply::Folder(archive.clone()))
            .unwrap()
            .label
            .clone();
        talk.type_text_understood(
            "den Eingang bitte",
            &format!(r#"{{"intent":"answer","choice":"{label}"}}"#),
        );
        assert_eq!(user_bubbles(&talk), ["den Eingang bitte"]);
        assert_eq!(
            talk.last_texts()[0],
            say(Msg::HermesChatReadSummaryOneArea, &[("count", "5")])
        );
    }

    #[test]
    fn the_working_line_says_thinking_or_waking() {
        let mut talk = small_talk();
        talk.open();
        talk.type_text_understood("hm", r#"{"intent":"other"}"#);
        assert_eq!(talk.working, Some(say(Msg::HermesChatThinking, &[])));

        talk.model.set_status(LocalModelStatus::NotLoadedYet);
        talk.type_text_understood("hm", r#"{"intent":"other"}"#);
        assert_eq!(talk.working, Some(say(Msg::HermesChatWaking, &[])));
        assert!(
            !talk
                .state
                .thread()
                .iter()
                .any(|entry| matches!(entry, Entry::Working(_))),
            "the line goes with the answer"
        );
    }

    #[test]
    fn a_missing_model_is_offered_and_the_sentence_is_sent_once_it_is_there() {
        let mut talk = small_talk();
        talk.model.set_status(LocalModelStatus::NotDownloaded);
        talk.open();
        talk.type_text("räum die Inbox auf");
        assert!(talk.model.turns().is_empty());
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatModelMissing,
                &[("size", &format::megabytes(LOCAL_MODEL.size))]
            )]
        );
        assert_eq!(
            talk.labels(),
            vec![
                say(Msg::HermesChatModelDownload, &[]),
                say(Msg::HermesChatLater, &[])
            ]
        );
        talk.press(&ChipReply::DownloadModel);
        assert_eq!(talk.actions, vec![Action::DownloadModel]);

        talk.model.set_status(loaded());
        talk.model
            .push_reply(Ok(r#"{"intent":"tidy_folder","source":"Inbox"}"#));
        talk.send(Message::ModelReady);
        assert_eq!(talk.model.refresh_count(), 1);
        let turns = talk.model.turns();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].message, "räum die Inbox auf");
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatConfirmTidy,
                &[
                    ("source", &talk.path("inbox").to_string_lossy()),
                    ("mode", "")
                ]
            )]
        );
        assert_eq!(
            user_bubbles(&talk)
                .iter()
                .filter(|text| text.starts_with("räum"))
                .count(),
            1,
            "no second bubble for the sentence sent again"
        );
    }

    fn offer_text() -> String {
        say(
            Msg::HermesChatModelMissing,
            &[("size", &format::megabytes(LOCAL_MODEL.size))],
        )
    }

    fn first_hermes_texts(talk: &Talk) -> Vec<String> {
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

    #[test]
    fn opening_without_the_model_offers_the_download_and_no_greeting() {
        let mut talk = small_talk();
        talk.model.set_status(LocalModelStatus::NotDownloaded);
        talk.open();
        assert_eq!(first_hermes_texts(&talk), vec![offer_text()]);
        assert_eq!(
            talk.labels(),
            vec![
                say(Msg::HermesChatModelDownload, &[]),
                say(Msg::HermesChatLater, &[])
            ]
        );
    }

    #[test]
    fn opening_with_a_usable_model_greets() {
        let mut talk = small_talk();
        talk.open();
        assert_eq!(
            first_hermes_texts(&talk),
            vec![say(Msg::HermesChatGreeting, &[])]
        );
    }

    #[test]
    fn opening_with_a_failed_model_says_so() {
        let mut talk = small_talk();
        talk.model.set_status(LocalModelStatus::Failed("no memory"));
        talk.open();
        assert_eq!(
            first_hermes_texts(&talk),
            vec![say(Msg::HermesChatModelFailed, &[("reason", "no memory")])]
        );
        assert_eq!(talk.labels().len(), 4);
    }

    #[test]
    fn opening_with_the_ai_off_offers_to_switch_it_on_then_the_download() {
        let mut talk = small_talk();
        talk.ctx.ai_on = false;
        talk.model.set_status(LocalModelStatus::NotDownloaded);
        talk.open();
        assert_eq!(
            first_hermes_texts(&talk),
            vec![say(Msg::HermesChatAiOff, &[])]
        );
        assert_eq!(
            talk.labels(),
            vec![
                say(Msg::HermesChatTurnOnAi, &[]),
                say(Msg::HermesChatLater, &[])
            ]
        );

        talk.press(&ChipReply::TurnOnAi);
        assert_eq!(talk.actions, vec![Action::TurnOnAi]);

        talk.ctx.ai_on = true;
        talk.send(Message::AiTurnedOn);
        assert_eq!(talk.last_texts(), vec![offer_text()]);

        talk.model.set_status(loaded());
        talk.send(Message::ModelReady);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatGreeting, &[])]);
    }

    #[test]
    fn a_sentence_with_the_ai_off_waits_for_it() {
        let mut talk = small_talk();
        talk.ctx.ai_on = false;
        talk.open();
        talk.type_text("räum die Inbox auf");
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatAiOff, &[])]);
        assert!(talk.model.turns().is_empty());

        talk.ctx.ai_on = true;
        talk.model
            .push_reply(Ok(r#"{"intent":"tidy_folder","source":"Inbox"}"#));
        talk.send(Message::AiTurnedOn);
        assert_eq!(talk.model.turns()[0].message, "räum die Inbox auf");
    }

    #[test]
    fn a_replayed_sentence_settles_the_owed_greeting() {
        let mut talk = small_talk();
        talk.ctx.ai_on = false;
        talk.open();
        talk.type_text("räum die Inbox auf");
        assert!(talk.state.greeting_owed);

        talk.ctx.ai_on = true;
        talk.model
            .push_reply(Ok(r#"{"intent":"tidy_folder","source":"Inbox"}"#));
        talk.send(Message::AiTurnedOn);
        assert!(!talk.state.greeting_owed);
        talk.model.push_reply(Ok(r#"{"intent":"other"}"#));
        talk.send(Message::ModelReady);
        assert!(
            !first_hermes_texts(&talk).contains(&say(Msg::HermesChatGreeting, &[])),
            "the greeting does not come on top of the answer"
        );
    }

    #[test]
    fn after_the_offer_the_download_finishing_brings_the_greeting() {
        let mut talk = small_talk();
        talk.model.set_status(LocalModelStatus::NotDownloaded);
        talk.open();
        talk.press(&ChipReply::DownloadModel);
        talk.model.set_status(loaded());
        talk.send(Message::ModelReady);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatGreeting, &[])]);
        assert_eq!(talk.model.refresh_count(), 1);
    }

    #[test]
    fn the_download_chip_is_no_sentence_and_the_ready_line_comes_before_the_answer() {
        let mut talk = small_talk();
        talk.model.set_status(LocalModelStatus::NotDownloaded);
        talk.type_text("räum die Inbox auf");
        let before = first_hermes_texts(&talk).len();
        talk.press(&ChipReply::DownloadModel);
        assert_eq!(talk.actions, vec![Action::DownloadModel]);
        assert!(talk.model.turns().is_empty());
        assert_eq!(
            first_hermes_texts(&talk).len(),
            before,
            "no answer to a press"
        );
        assert!(!first_hermes_texts(&talk).contains(&say(Msg::HermesChatFallback, &[])));

        talk.model.set_status(loaded());
        talk.model
            .push_reply(Ok(r#"{"intent":"tidy_folder","source":"Inbox"}"#));
        talk.send(Message::ModelReady);
        let texts = first_hermes_texts(&talk);
        let ready = texts
            .iter()
            .position(|text| *text == say(Msg::HermesChatModelReady, &[]))
            .unwrap();
        assert_eq!(ready, texts.len() - 2, "{texts:?}");
        assert_eq!(talk.model.turns()[0].message, "räum die Inbox auf");
    }

    #[test]
    fn the_ready_line_comes_before_the_greeting_too() {
        let mut talk = small_talk();
        talk.model.set_status(LocalModelStatus::NotDownloaded);
        talk.open();
        talk.model.set_status(loaded());
        talk.send(Message::ModelReady);
        let texts = first_hermes_texts(&talk);
        assert_eq!(
            texts[texts.len() - 2..],
            [
                say(Msg::HermesChatModelReady, &[]),
                say(Msg::HermesChatGreeting, &[])
            ]
        );
    }

    #[test]
    fn after_later_a_typed_sentence_offers_the_download_again() {
        let mut talk = small_talk();
        talk.model.set_status(LocalModelStatus::NotDownloaded);
        talk.open();
        talk.press(&ChipReply::Later);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatLaterReply, &[])]);
        talk.type_text("räum die Inbox auf");
        assert_eq!(talk.last_texts(), vec![offer_text()]);
        assert!(talk.model.turns().is_empty());
    }

    #[test]
    fn a_failed_model_an_error_and_a_name_that_climbs_out_end_in_the_fallback() {
        let fallback = |talk: &Talk| {
            assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatFallback, &[])]);
            assert_eq!(talk.labels().len(), 4);
            assert!(talk.state.pending_work().is_none());
        };

        let mut talk = small_talk();
        talk.model.set_status(LocalModelStatus::Failed("no memory"));
        talk.type_text("hallo");
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatModelFailed, &[("reason", "no memory")])]
        );
        assert_eq!(talk.labels().len(), 4);
        assert!(talk.model.turns().is_empty());

        talk.model.set_status(loaded());
        talk.model.push_reply(Err(AiError::Model("broken")));
        talk.type_text("hallo");
        fallback(&talk);

        talk.type_text_understood(
            "räum ../geheim auf",
            r#"{"intent":"tidy_folder","source":"../geheim"}"#,
        );
        fallback(&talk);
        talk.type_text_understood(
            "räum Inbox nach ../x",
            r#"{"intent":"tidy_folder","source":"Inbox","target":"../x"}"#,
        );
        fallback(&talk);
    }

    #[test]
    fn a_build_without_the_model_answers_with_the_fallback() {
        let mut talk = small_talk();
        talk.ctx.model = None;
        talk.type_text("hallo");
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatFallback, &[])]);
    }

    #[test]
    fn a_tidy_sentence_is_confirmed_and_changed_before_anything_is_read() {
        let mut talk = small_talk();
        let before = talk.files();
        talk.type_text_understood(
            "verschieb die Inbox ins Archiv",
            r#"{"intent":"tidy_folder","source":"Inbox","target":"archive","transfer":"move"}"#,
        );
        let inbox = talk.path("inbox");
        let archive = talk.path("archive");
        let confirm = |mode: Msg| {
            say(
                Msg::HermesChatConfirmTidyTo,
                &[
                    ("source", &inbox.to_string_lossy()),
                    ("target", &archive.to_string_lossy()),
                    ("mode", &say(mode, &[])),
                ],
            )
        };
        assert_eq!(talk.last_texts(), vec![confirm(Msg::HermesChatConfirmMove)]);
        assert!(talk.state.pending_work().is_none());
        assert_eq!(
            talk.labels(),
            vec![
                say(Msg::HermesChatConfirmYes, &[]),
                say(Msg::HermesChatOtherTarget, &[]),
                say(Msg::HermesChatCopy, &[]),
                say(Msg::HermesChatCancel, &[]),
            ]
        );

        talk.press(&ChipReply::SwitchToCopy);
        assert_eq!(talk.last_texts(), vec![confirm(Msg::HermesChatConfirmCopy)]);
        assert_eq!(talk.labels()[2], say(Msg::HermesChatMove, &[]));

        talk.press(&ChipReply::OtherTarget);
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatAskOtherTarget, &[])]
        );
        let elsewhere = talk.path("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        talk.type_text(&elsewhere.to_string_lossy());
        talk.press(&ChipReply::ReadAnyway);
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatConfirmTidyTo,
                &[
                    ("source", &talk.path("inbox").to_string_lossy()),
                    ("target", &elsewhere.to_string_lossy()),
                    ("mode", &say(Msg::HermesChatConfirmCopy, &[])),
                ]
            )]
        );
        assert_eq!(talk.files(), before);
    }

    #[test]
    fn yes_reads_and_tries_with_what_was_said_and_files_nothing() {
        let mut talk = small_talk();
        let before = talk.files();
        talk.type_text_understood(
            "räum die Inbox ins Archiv auf, einmal, kopieren",
            r#"{"intent":"tidy_folder","source":"Inbox","target":"archive","schedule":"once","transfer":"copy"}"#,
        );
        talk.press(&ChipReply::ConfirmTidy);
        assert_eq!(talk.plan().filed, 1);
        assert_eq!(talk.files(), before, "the trial moves nothing");
        assert_eq!(
            talk.state.chips()[0].label,
            say(Msg::HermesChatFileAll, &[("count", "1")])
        );
        talk.press(&ChipReply::FileAll);
        assert_eq!(talk.files().len(), before.len() + 1);
    }

    #[test]
    fn an_always_in_the_sentence_goes_straight_on_with_the_trial() {
        let mut talk = small_talk();
        talk.type_text_understood(
            "räum die Inbox immer auf",
            r#"{"intent":"tidy_folder","source":"Inbox","schedule":"always","transfer":"move"}"#,
        );
        talk.press(&ChipReply::ConfirmTidy);
        assert!(talk.plan().filed > 0);
    }

    fn archive_with_years() -> Talk {
        let mut talk = Talk::in_dir(tempfile::tempdir().unwrap(), &["archive"], &["inbox"]);
        for year in [2020, 2021] {
            for (month, day) in [(1, 10), (3, 11), (6, 12)] {
                talk.write(
                    &format!("archive/Alfa/{year}/{year}-{month:02}-{day} Alfa Rechnung.pdf"),
                    &letter(
                        "Alfa AG",
                        &format!("{day}.{month:02}.{year}"),
                        "Rechnung",
                        day + month,
                    ),
                );
            }
        }
        talk.open();
        talk.press(&ChipReply::Folder(talk.path("archive")));
        talk.press(&ChipReply::WantChange);
        talk
    }

    #[test]
    fn a_change_without_a_scope_needs_a_yes_before_it_counts() {
        let mut talk = archive_with_years();
        talk.type_text_understood(
            "Rechnungen lieber nach Monat",
            r#"{"intent":"change_structure","area":"Rechnungen","order":"by_month"}"#,
        );
        assert!(talk.state.session.changes.is_empty());
        assert_eq!(
            talk.labels(),
            vec![
                say(Msg::HermesChatConfirmYes, &[]),
                say(Msg::HermesChatCancel, &[])
            ]
        );
        talk.press(&ChipReply::ConfirmChange);
        assert_eq!(talk.state.session.changes.len(), 1);
        assert_eq!(talk.last_texts()[0], say(Msg::HermesChatChangeDone, &[]));
    }

    fn archive_laid_out(layout: fn(i32, u32, u32) -> String) -> Talk {
        let mut talk = Talk::in_dir(tempfile::tempdir().unwrap(), &["archive"], &["inbox"]);
        for year in [2020, 2021] {
            for (month, day) in [(1, 10), (3, 11), (6, 12)] {
                talk.write(
                    &format!("archive/{}", layout(year, month, day)),
                    &letter(
                        "Alfa AG",
                        &format!("{day}.{month:02}.{year}"),
                        "Rechnung",
                        day + month,
                    ),
                );
            }
        }
        talk.open();
        talk.press(&ChipReply::Folder(talk.path("archive")));
        talk.press(&ChipReply::WantChange);
        talk
    }

    fn asks_to_confirm_the_order(order_word: &str, order: ChangeOrder, mut talk: Talk) {
        talk.type_text_understood(
            "Rechnungen anders ordnen",
            &format!(
                r#"{{"intent":"change_structure","area":"Rechnungen","order":"{order_word}"}}"#
            ),
        );
        assert!(talk.state.session.changes.is_empty());
        assert_eq!(
            talk.labels(),
            vec![
                say(Msg::HermesChatConfirmYes, &[]),
                say(Msg::HermesChatCancel, &[])
            ]
        );
        talk.press(&ChipReply::ConfirmChange);
        let changes = &talk.state.session.changes;
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].order, order);
    }

    #[test]
    fn every_order_shows_an_example_and_counts_after_a_yes() {
        asks_to_confirm_the_order(
            "by_year",
            ChangeOrder::ByYear,
            archive_laid_out(|year, month, day| {
                format!("Alfa/{year}-{month:02}-{day} Alfa Rechnung.pdf")
            }),
        );
        asks_to_confirm_the_order(
            "by_sender",
            ChangeOrder::BySender,
            archive_laid_out(|year, month, day| {
                format!("Alfa/{year}/{year}-{month:02}-{day} Alfa Rechnung.pdf")
            }),
        );
        asks_to_confirm_the_order(
            "by_kind",
            ChangeOrder::ByKind,
            archive_laid_out(|year, month, day| {
                format!("Alfa/{year}/{year}-{month:02}-{day} Alfa Rechnung.pdf")
            }),
        );
        asks_to_confirm_the_order(
            "flat",
            ChangeOrder::Flat,
            archive_laid_out(|year, month, day| {
                format!("Alfa/{year}/{year}-{month:02}-{day} Alfa Rechnung.pdf")
            }),
        );
    }
}
