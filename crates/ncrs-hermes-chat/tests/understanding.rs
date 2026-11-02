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
    use ncrs_hermes_chat::{Action, ChipReply, Entry, Message};
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
        let inbox = talk.path("inbox");
        let label = talk
            .state
            .chips()
            .iter()
            .find(|chip| chip.reply == ChipReply::Folder(inbox.clone()))
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
            say(Msg::HermesChatReadSummaryNoOrder, &[("count", "1")])
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
                &[("source", "inbox"), ("mode", "")]
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
        let confirm = |mode: Msg| {
            say(
                Msg::HermesChatConfirmTidyTo,
                &[
                    ("source", "inbox"),
                    ("target", "archive"),
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
        talk.type_text(&elsewhere.to_string_lossy());
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatConfirmTidyTo,
                &[
                    ("source", "inbox"),
                    ("target", "elsewhere"),
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
    fn an_always_in_the_sentence_says_it_is_not_there_yet() {
        let mut talk = small_talk();
        talk.type_text_understood(
            "räum die Inbox immer auf",
            r#"{"intent":"tidy_folder","source":"Inbox","schedule":"always","transfer":"move"}"#,
        );
        talk.press(&ChipReply::ConfirmTidy);
        assert!(talk.plan().filed > 0);
        let texts: Vec<String> = talk
            .state
            .thread()
            .iter()
            .filter_map(|entry| match entry {
                Entry::Hermes { parts, .. } => Some(parts.clone()),
                _ => None,
            })
            .flatten()
            .filter_map(|part| match part {
                ncrs_hermes_chat::Part::Text(text) => Some(text),
                _ => None,
            })
            .collect();
        assert!(texts.contains(&say(Msg::HermesChatAlwaysNotYet, &[])));
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

    #[test]
    fn an_order_hermes_cannot_do_yet_changes_nothing() {
        let mut talk = archive_with_years();
        talk.type_text_understood(
            "Rechnungen lieber nach Jahr",
            r#"{"intent":"change_structure","area":"Rechnungen","order":"by_year"}"#,
        );
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatOrderNotYet,
                &[("order", &say(Msg::HermesChatOrderYear, &[]))]
            )]
        );
        assert!(talk.state.session.changes.is_empty());
        assert!(talk.state.pending_work().is_none());
    }
}
