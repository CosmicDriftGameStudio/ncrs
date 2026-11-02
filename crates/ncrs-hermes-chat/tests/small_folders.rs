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

    fn tidy_until_trial(talk: &mut Talk, transfer: &ChipReply) {
        talk.type_text("Räum Inbox auf");
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
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatTrialIntro, &[("count", "10")])]
        );
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
        assert_eq!((first.rows.len(), all.rows.len()), (3, 6));
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
    fn always_says_it_cannot_yet_and_goes_on_with_the_mode() {
        let mut talk = small_talk();
        talk.type_text("Räum Inbox auf");
        talk.press(&ChipReply::Always);
        assert_eq!(
            talk.last_texts(),
            vec![
                say(Msg::HermesChatAlwaysNotYet, &[]),
                say(Msg::HermesChatAskMode, &[("folder", "inbox")]),
            ]
        );
        talk.press(&ChipReply::ModeMove);
        assert!(talk.state.trial_plan().is_some());
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

        talk.type_text("such meine Rechnung");
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatSearchNotYet, &[])]
        );
        talk.press(&ChipReply::OpenSearch);
        assert_eq!(talk.actions, vec![Action::OpenSearch]);

        talk.type_text("rm -rf /tmp/x");
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatShell, &[])]);
        talk.press(&ChipReply::ToCommandLine("rm -rf /tmp/x".to_owned()));
        assert_eq!(
            talk.actions.last(),
            Some(&Action::UseAsCommand("rm -rf /tmp/x".to_owned()))
        );

        talk.type_text("hmmmm wie geht das");
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
        talk.ctx.availability = Availability::NeedsSetup;
        talk.open();
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatNeedsSetup, &[])]);
        talk.press(&ChipReply::Setup);
        assert_eq!(talk.actions, vec![Action::OpenFilingSetup]);

        let mut waiting = small_talk();
        waiting.ctx.availability = Availability::Starting;
        waiting.open();
        assert_eq!(
            waiting.last_texts(),
            vec![say(Msg::HermesChatStarting, &[])]
        );
        assert!(waiting.state.chips().is_empty());
    }

    #[test]
    fn a_source_inside_a_target_is_not_tidied_into_itself() {
        let mut talk = small_talk();
        let archive = talk.path("archive");
        talk.type_text(&format!("Räum {} auf", archive.display()));
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatSourceIsTarget, &[("folder", "archive")])]
        );

        let inside = archive.join("Alfa");
        talk.type_text(&format!("Räum {} auf", inside.display()));
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatUnknownPathConfirm,
                &[("path", &inside.to_string_lossy())]
            )]
        );
        talk.press(&ChipReply::ReadAnyway);
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
        talk.type_text(&format!("Lies {} ein", elsewhere.display()));
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatUnknownPathConfirm,
                &[("path", &elsewhere.to_string_lossy())]
            )]
        );
        assert!(talk.state.pending_work().is_none());
        talk.press(&ChipReply::Cancel);
        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatCancelled, &[])]);
    }
}
