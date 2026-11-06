//! A rule made in the chat files the waiting documents: the click on "File
//! now" leads to the usual trial, the file moves only on the click on "File
//! all" and goes back with undo.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::common::{letter, Talk};
    use ncrs_hermes_chat::{ChipReply, Message};
    use ncrs_index::{Condition, ConditionKind, RuleConditions, RuleSpec};

    const WAITING: &str = "inbox/scan.pdf";

    fn spec() -> RuleSpec {
        RuleSpec {
            name: None,
            conditions: RuleConditions::all_of(vec![Condition::new(
                ConditionKind::Word,
                "Rechnung",
            )]),
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: None,
            tags: BTreeSet::new(),
            filing_pattern: "Telefon/{year}".to_owned(),
            confirmed: false,
            enabled: false,
        }
    }

    /// Four invoices of `Alfa AG` in the archive, one more waits in the inbox,
    /// and the empty folder `Telefon` has just got its rule.
    fn talk_with_stored_rule() -> Talk {
        let mut talk = Talk::in_dir(tempfile::tempdir().unwrap(), &["archive"], &["inbox"]);
        for (month, day) in [(1, 10), (3, 11), (6, 12), (9, 13)] {
            talk.index_document(
                &format!("archive/Alfa/2020/2020-{month:02}-{day}.pdf"),
                &letter(
                    "Alfa AG",
                    &format!("{day}.{month:02}.2020"),
                    "Rechnung",
                    day,
                ),
            );
        }
        std::fs::create_dir_all(talk.path("archive/Telefon")).unwrap();
        talk.index_inbox_document(WAITING, &letter("Alfa AG", "05.07.2021", "Rechnung", 77));
        talk = talk.with_rules();
        let folder = talk.path("archive");
        talk.send(Message::ChangeArea {
            folder,
            area: "Telefon".to_owned(),
        });
        talk.press(&ChipReply::LevelsKeep);
        talk.model.push_rule(Ok(spec()));
        talk.type_text("Rechnungen kommen hierhin");
        talk.press(&ChipReply::RuleCreate);
        assert_eq!(talk.rule_count(), 1);
        talk
    }

    const FILED_AT: &str = "archive/Telefon/2021/2021-07-05_Alfa AG_Invoice.pdf";

    fn trial_of_the_waiting_file(talk: &mut Talk, mode: &ChipReply) {
        talk.press(&ChipReply::RuleNow);
        assert!(
            talk.actions.is_empty(),
            "filing now needs nothing of the app"
        );
        talk.press(&ChipReply::Folder(talk.path("archive")));
        talk.press(&ChipReply::Once);
        talk.press(mode);
        assert!(talk
            .labels()
            .iter()
            .any(|label| label.contains("1 ablegen")));
    }

    #[test]
    fn file_now_moves_the_waiting_file_where_the_rule_says_and_undo_puts_it_back() {
        let mut talk = talk_with_stored_rule();
        let before = talk.files();

        trial_of_the_waiting_file(&mut talk, &ChipReply::ModeMove);
        assert_eq!(talk.files(), before, "the trial moves nothing");

        talk.press(&ChipReply::FileAll);
        let filed = talk.files();
        assert!(!filed.contains_key(WAITING), "{:?}", filed.keys());
        assert_eq!(filed[FILED_AT], before[WAITING], "same content, new place");

        talk.press(&ChipReply::Undo);
        talk.press(&ChipReply::PutBack);
        assert_eq!(talk.files(), before);
    }

    #[test]
    fn a_typed_yes_to_the_trial_moves_nothing() {
        let mut talk = talk_with_stored_rule();
        let before = talk.files();
        trial_of_the_waiting_file(&mut talk, &ChipReply::ModeMove);
        let file_all = talk
            .state
            .chips()
            .iter()
            .find(|chip| chip.reply == ChipReply::FileAll)
            .unwrap()
            .label
            .clone();

        talk.type_text_understood(
            "ja, ablegen",
            &format!(r#"{{"intent":"answer","choice":"{file_all}"}}"#),
        );

        assert_eq!(talk.files(), before, "a typed answer files nothing");
        talk.press(&ChipReply::FileAll);
        assert!(talk.files().contains_key(FILED_AT));
    }

    #[test]
    fn a_typed_yes_to_file_now_files_nothing_either() {
        let mut talk = talk_with_stored_rule();
        let before = talk.files();
        let file_now = talk
            .state
            .chips()
            .iter()
            .find(|chip| chip.reply == ChipReply::RuleNow)
            .unwrap()
            .label
            .clone();

        talk.type_text_understood(
            "ja, ablegen",
            &format!(r#"{{"intent":"answer","choice":"{file_now}"}}"#),
        );

        assert_eq!(talk.files(), before, "nothing moved");
        assert!(
            !talk
                .state
                .chips()
                .iter()
                .any(|chip| chip.reply == ChipReply::FileAll),
            "no trial filed anything"
        );
        assert!(talk.actions.is_empty());
    }
}
