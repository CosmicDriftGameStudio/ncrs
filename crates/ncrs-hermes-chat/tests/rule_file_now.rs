//! A rule made in the chat files the waiting documents: the click on
//! "Rearrange now" is the confirmation and files at once; undo puts it back.

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

    use crate::common::{letter, say, Talk};
    use ncrs_hermes_chat::{ChipReply, Message};
    use ncrs_i18n::Msg;
    use ncrs_index::{Condition, ConditionKind, RuleConditions, RuleSpec};

    const WAITING: &str = "inbox/scan.pdf";

    fn spec() -> RuleSpec {
        spec_for("Rechnung", "Telefon/{year}")
    }

    fn spec_for(word: &str, pattern: &str) -> RuleSpec {
        RuleSpec {
            name: None,
            conditions: RuleConditions::all_of(vec![Condition::new(ConditionKind::Word, word)]),
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: None,
            tags: BTreeSet::new(),
            filing_pattern: pattern.to_owned(),
            confirmed: false,
            enabled: false,
        }
    }

    /// Four invoices of `Alfa AG` in the archive, one more waits in the inbox,
    /// and the empty folder `Telefon` has just got its rule.
    fn talk_with_stored_rule() -> Talk {
        stored_rule(&[(
            WAITING,
            letter("Alfa AG", "05.07.2021", "Rechnung", 77),
            (2021, 7, 5),
        )])
    }

    fn stored_rule(waiting: &[(&str, String, (i32, u32, u32))]) -> Talk {
        stored_rule_after(waiting, None)
    }

    fn stored_rule_after(
        waiting: &[(&str, String, (i32, u32, u32))],
        other: Option<(&str, &str)>,
    ) -> Talk {
        stored_rule_in(&["inbox"], waiting, other)
    }

    /// `other`: an area and a word of a rule made before the one under test.
    fn stored_rule_in(
        sources: &[&str],
        waiting: &[(&str, String, (i32, u32, u32))],
        other: Option<(&str, &str)>,
    ) -> Talk {
        let mut talk = Talk::in_dir(tempfile::tempdir().unwrap(), &["archive"], sources);
        for source in sources {
            std::fs::create_dir_all(talk.path(source)).unwrap();
        }
        for (month, day) in [(1, 10), (3, 11), (6, 12), (9, 13)] {
            let id = talk.index_document(
                &format!("archive/Alfa/2020/2020-{month:02}-{day}.pdf"),
                &letter(
                    "Alfa AG",
                    &format!("{day}.{month:02}.2020"),
                    "Rechnung",
                    day,
                ),
            );
            talk.set_fields(
                id,
                &ncrs_index::DocumentFields {
                    sender: Some("Alfa AG".to_owned()),
                    kind: Some("invoice".to_owned()),
                    date: chrono::NaiveDate::from_ymd_opt(2020, month, day),
                    ..ncrs_index::DocumentFields::default()
                },
            );
        }
        std::fs::create_dir_all(talk.path("archive/Telefon")).unwrap();
        for (name, text, (year, month, day)) in waiting {
            let id = talk.index_inbox_document(name, text);
            talk.set_fields(
                id,
                &ncrs_index::DocumentFields {
                    sender: Some("Alfa AG".to_owned()),
                    kind: Some("invoice".to_owned()),
                    date: chrono::NaiveDate::from_ymd_opt(*year, *month, *day),
                    ..ncrs_index::DocumentFields::default()
                },
            );
        }
        talk = talk.with_rules();
        if let Some((area, word)) = other {
            std::fs::create_dir_all(talk.path(&format!("archive/{area}"))).unwrap();
            let folder = talk.path("archive");
            talk.send(Message::ChangeArea {
                folder,
                area: area.to_owned(),
            });
            talk.press(&ChipReply::LevelsKeep);
            talk.model
                .push_rule(Ok(spec_for(word, &format!("{area}/{{year}}"))));
            talk.type_text("Das kommt woanders hin");
            talk.press(&ChipReply::RuleCreate);
            talk.press(&ChipReply::RuleOnlyNew);
        }
        let folder = talk.path("archive");
        talk.send(Message::ChangeArea {
            folder,
            area: "Telefon".to_owned(),
        });
        talk.press(&ChipReply::LevelsKeep);
        talk.model.push_rule(Ok(spec()));
        talk.type_text("Rechnungen kommen hierhin");
        talk.press(&ChipReply::RuleCreate);
        assert_eq!(talk.rule_count(), 1 + usize::from(other.is_some()));
        talk
    }

    const FILED_AT: &str = "archive/Telefon/2021/2021-07-05_Alfa AG_Invoice.pdf";

    #[test]
    fn file_now_moves_the_waiting_file_where_the_rule_says_and_undo_puts_it_back() {
        let mut talk = talk_with_stored_rule();
        let before = talk.files();

        talk.press(&ChipReply::RuleNow);
        assert!(
            talk.actions.is_empty(),
            "filing now needs nothing of the app"
        );
        let filed = talk.files();
        assert!(!filed.contains_key(WAITING), "{:?}", filed.keys());
        assert_eq!(filed[FILED_AT], before[WAITING], "same content, new place");

        talk.press(&ChipReply::Undo);
        talk.press(&ChipReply::PutBack);
        assert_eq!(talk.files(), before);
    }

    #[test]
    fn rearrange_now_asks_nothing_and_files_inbox_and_archive_in_one_run() {
        let mut talk = talk_with_stored_rule();
        let before = talk.files();

        talk.press(&ChipReply::RuleNow);

        let asked: Vec<_> = talk
            .state
            .chips()
            .iter()
            .map(|chip| chip.reply.clone())
            .filter(|reply| {
                matches!(
                    reply,
                    ChipReply::Folder(_)
                        | ChipReply::Once
                        | ChipReply::Always
                        | ChipReply::ModeMove
                        | ChipReply::ModeCopy
                        | ChipReply::FileAll
                )
            })
            .collect();
        assert!(asked.is_empty(), "{asked:?}");
        assert!(talk.files().contains_key(FILED_AT));
        assert!(names_below(&talk, "archive/Alfa").is_empty());
        assert_eq!(names_below(&talk, "archive/Telefon/2020/").len(), 4);

        talk.press(&ChipReply::Undo);
        talk.press(&ChipReply::PutBack);
        assert_eq!(talk.files(), before, "one undo restores both");
    }

    #[test]
    fn rearrange_now_leaves_documents_other_rules_would_file() {
        let strom = letter("Alfa AG", "06.07.2021", "Strom", 78);
        let mut talk = stored_rule_after(
            &[
                (
                    WAITING,
                    letter("Alfa AG", "05.07.2021", "Rechnung", 77),
                    (2021, 7, 5),
                ),
                ("inbox/strom.pdf", strom, (2021, 7, 6)),
            ],
            Some(("Strom", "Strom")),
        );

        talk.press(&ChipReply::RuleNow);

        let filed = talk.files();
        assert!(filed.contains_key(FILED_AT), "{:?}", filed.keys());
        assert!(!filed.contains_key(WAITING));
        assert!(
            filed.contains_key("inbox/strom.pdf"),
            "the other rule's document stays: {:?}",
            filed.keys()
        );
    }

    #[test]
    fn an_empty_first_source_hands_the_archive_moves_to_the_next() {
        let mut talk = stored_rule_in(
            &["inbox_a", "inbox"],
            &[(
                WAITING,
                letter("Alfa AG", "05.07.2021", "Rechnung", 77),
                (2021, 7, 5),
            )],
            None,
        );
        let before = talk.files();

        talk.press(&ChipReply::RuleNow);

        let filed = talk.files();
        assert!(filed.contains_key(FILED_AT), "{:?}", filed.keys());
        assert!(names_below(&talk, "archive/Alfa").is_empty());
        assert_eq!(names_below(&talk, "archive/Telefon/2020/").len(), 4);
        assert!(talk.state.session.rule_queue.is_none());

        talk.press(&ChipReply::Undo);
        talk.press(&ChipReply::PutBack);
        assert_eq!(talk.files(), before);
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

    #[test]
    fn the_stored_rule_is_named_and_asks_about_the_existing_files_in_its_own_bubble() {
        let talk = talk_with_stored_rule();

        let rule = &talk.rules()[0];
        assert_eq!(rule.spec.name.as_deref(), Some("Telefon"));
        assert_eq!(rule.origin, ncrs_index::RuleOrigin::Chat);
        // Four archive documents and the waiting file.
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatRuleMoveQuestionOther, &[("count", "5")])]
        );
        assert_eq!(
            talk.labels()[..2],
            [
                say(Msg::HermesChatRuleRearrangeNow, &[("count", "5")]),
                say(Msg::HermesChatRuleOnlyNew, &[]),
            ]
        );
    }

    #[test]
    fn only_new_moves_nothing_and_offers_the_usual_buttons() {
        let mut talk = talk_with_stored_rule();
        let before = talk.files();

        talk.press(&ChipReply::RuleOnlyNew);

        assert_eq!(talk.files(), before);
        assert_eq!(
            talk.last_texts(),
            vec![say(Msg::HermesChatRuleOnlyNewDone, &[])]
        );
        let replies: Vec<_> = talk
            .state
            .chips()
            .iter()
            .map(|chip| chip.reply.clone())
            .collect();
        assert!(!replies.contains(&ChipReply::RuleNow));
        assert!(!replies.contains(&ChipReply::RuleOnlyNew));
        assert!(!replies.is_empty(), "the usual buttons");
        let rule = &talk.rules()[0];
        assert_eq!(rule.spec.name.as_deref(), Some("Telefon"));
        assert_eq!(rule.origin, ncrs_index::RuleOrigin::Chat);
    }

    fn names_below(talk: &Talk, folder: &str) -> Vec<String> {
        talk.files()
            .keys()
            .filter(|path| path.starts_with(folder))
            .cloned()
            .collect()
    }

    #[test]
    fn file_now_with_only_archive_documents_moves_them_at_once_and_undo_puts_them_back() {
        let mut talk = stored_rule(&[]);
        let before = talk.files();

        talk.press(&ChipReply::RuleNow);
        assert!(names_below(&talk, "archive/Alfa").is_empty());
        assert_eq!(names_below(&talk, "archive/Telefon/2020/").len(), 4);
        talk.press(&ChipReply::Undo);
        talk.press(&ChipReply::PutBack);
        assert_eq!(talk.files(), before);
    }

    #[test]
    fn archive_renames_run_even_when_the_waiting_file_cannot_be_filed() {
        let copy = letter("Alfa AG", "10.01.2020", "Rechnung", 10);
        let mut talk = stored_rule(&[("inbox/copy.pdf", copy, (2020, 1, 10))]);
        let before = talk.files();

        talk.press(&ChipReply::RuleNow);

        assert!(names_below(&talk, "archive/Alfa").is_empty());
        assert_eq!(names_below(&talk, "archive/Telefon/2020/").len(), 4);
        assert!(talk.files().contains_key("inbox/copy.pdf"));
        assert!(
            !talk.path("archive/Alfa").exists(),
            "the emptied folders go"
        );
        talk.press(&ChipReply::Undo);
        talk.press(&ChipReply::PutBack);
        assert_eq!(talk.files(), before);
        assert!(talk.path("archive/Alfa/2020").is_dir());
    }

    #[test]
    fn archive_renames_in_two_archives_each_run_in_its_own_archive() {
        let mut talk = Talk::in_dir(
            tempfile::tempdir().unwrap(),
            &["archive", "second"],
            &["inbox"],
        );
        for (root, day) in [
            ("archive", 10),
            ("archive", 11),
            ("second", 12),
            ("second", 13),
        ] {
            let id = talk.index_document(
                &format!("{root}/Alfa/2020/2020-01-{day}.pdf"),
                &letter("Alfa AG", &format!("{day}.01.2020"), "Rechnung", day),
            );
            talk.set_fields(
                id,
                &ncrs_index::DocumentFields {
                    sender: Some("Alfa AG".to_owned()),
                    kind: Some("invoice".to_owned()),
                    date: chrono::NaiveDate::from_ymd_opt(2020, 1, day),
                    ..ncrs_index::DocumentFields::default()
                },
            );
        }
        talk = talk.with_rules();
        std::fs::create_dir_all(talk.path("archive/Telefon")).unwrap();
        let before = talk.files();
        talk.send(Message::ChangeArea {
            folder: talk.path("archive"),
            area: "Telefon".to_owned(),
        });
        talk.press(&ChipReply::LevelsKeep);
        talk.model.push_rule(Ok(spec()));
        talk.type_text("Rechnungen kommen hierhin");
        talk.press(&ChipReply::RuleCreate);
        talk.press(&ChipReply::RuleNow);

        for root in ["archive", "second"] {
            assert!(
                names_below(&talk, &format!("{root}/Alfa")).is_empty(),
                "{:?}",
                talk.files().keys()
            );
            assert_eq!(
                names_below(&talk, &format!("{root}/Telefon/2020/")).len(),
                2
            );
        }
        talk.press(&ChipReply::Undo);
        talk.press(&ChipReply::PutBack);
        talk.press(&ChipReply::IntentUndo);
        talk.press(&ChipReply::PutBack);
        assert_eq!(talk.files(), before);
    }
}
