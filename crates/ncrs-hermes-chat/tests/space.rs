//! The Platz answer in the chat: a sentence about the full drive leads to a
//! measure, the card with its buttons, and runs that go through the Platz tab.

mod common;

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    use crate::common::{say, Talk, LANG};
    use iced::keyboard::{key::Named, Key, Modifiers};
    use ncrs_core::FolderSize;
    use ncrs_hermes_chat::{
        card_key_message, chat_key, Action, ChatKey, ChipReply, Entry, Message, Part,
        SpaceDecideTarget, SpaceRunKind,
    };
    use ncrs_i18n::Msg;
    use ncrs_space::decide::DecideKind;
    use ncrs_space::{
        CatalogId, Entry as SpaceEntry, EntryKind, Group, LogRow, RunLog, Scan, Skip, VolumeSpace,
        WorktreeSpace,
    };
    use ncrs_ui_kit::format;

    const GB: u64 = 1_000_000_000;

    fn talk() -> Talk {
        Talk::in_dir(tempfile::tempdir().unwrap(), &["archive"], &["inbox"])
    }

    fn gb(bytes: u64) -> String {
        format::size_shown(bytes * GB, LANG)
    }

    fn scan(talk: &Talk) -> Arc<Scan> {
        let home = &talk.ctx.home;
        let size = |gigabytes: u64| FolderSize {
            bytes: gigabytes * GB,
            partial: false,
        };
        let entry = |name: &str, group, kind, gigabytes| SpaceEntry {
            path: home.join(name),
            group,
            kind,
            size: size(gigabytes),
            modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
            device: None,
            held_by: Vec::new(),
            facts: Default::default(),
        };
        Arc::new(Scan {
            volume: VolumeSpace {
                total: 1000 * GB,
                available: 100 * GB,
            },
            entries: vec![
                entry(
                    "target",
                    Group::Safe,
                    EntryKind::Catalog(CatalogId::CargoTarget),
                    40,
                ),
                entry(
                    "wt",
                    Group::Worktrees,
                    EntryKind::Worktree(WorktreeSpace {
                        repo: home.join("repo"),
                        path: home.join("wt"),
                        branch: "feat/old".to_owned(),
                        size: size(20),
                        modified: None,
                        ignored_files: Vec::new(),
                    }),
                    20,
                ),
                entry(
                    "Backups",
                    Group::Decide,
                    EntryKind::Decide(DecideKind::BigFolder),
                    30,
                ),
            ],
            partial: false,
            processes_known: true,
            volume_name: String::new(),
            worktree_roots: Vec::new(),
            measured_at: SystemTime::UNIX_EPOCH,
            took: Duration::from_secs(9),
        })
    }

    /// Downloads (when `downloads`) and session logs to decide on, no worktrees.
    fn scan_without_worktrees(talk: &Talk, downloads: bool) -> Arc<Scan> {
        let home = &talk.ctx.home;
        let entry = |name: &str, kind| SpaceEntry {
            path: home.join(name),
            group: Group::Decide,
            kind: EntryKind::Decide(kind),
            size: FolderSize {
                bytes: 20 * GB,
                partial: false,
            },
            modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
            device: None,
            held_by: Vec::new(),
            facts: Default::default(),
        };
        let mut entries = vec![entry(".claude/projects", DecideKind::SessionLogs)];
        if downloads {
            entries.push(entry("Downloads/old.dmg", DecideKind::BigFolder));
        }
        Arc::new(Scan {
            volume: VolumeSpace {
                total: 1000 * GB,
                available: 100 * GB,
            },
            entries,
            partial: false,
            processes_known: true,
            volume_name: String::new(),
            worktree_roots: Vec::new(),
            measured_at: SystemTime::UNIX_EPOCH,
            took: Duration::from_secs(9),
        })
    }

    fn empty_run() -> Message {
        Message::SpaceRunDone(Arc::new(RunLog {
            started: SystemTime::UNIX_EPOCH,
            finished: SystemTime::UNIX_EPOCH,
            rows: Vec::new(),
        }))
    }

    #[test]
    fn after_a_run_the_untouched_folders_are_listed_and_downloads_opens_its_decision() {
        let mut talk = asks_to_measure(
            "warum ist die platte voll",
            r#"{"intent":"space","ask":"why"}"#,
        );
        let measured = scan_without_worktrees(&talk, true);
        talk.send(Message::SpaceMeasured(measured));
        talk.send(empty_run());

        let texts = talk.last_texts();
        assert!(texts.contains(&say(Msg::HermesChatSpaceNoWorktrees, &[])));
        assert!(texts.contains(&say(Msg::HermesChatSpaceUntouched, &[])));
        let list = texts.last().unwrap();
        assert!(list.contains("Downloads"), "{list}");
        assert!(
            list.contains(&say(Msg::SpaceDecideSessionLogs, &[])),
            "{list}"
        );
        assert!(talk
            .labels()
            .contains(&say(Msg::HermesChatSpaceShowOldLogs, &[])));
        talk.press(&ChipReply::SpaceShowDownloads);
        assert_eq!(
            talk.actions.last(),
            Some(&Action::SpaceDecide(SpaceDecideTarget::Downloads))
        );
    }

    #[test]
    fn without_downloads_in_the_scan_there_is_no_downloads_button() {
        let mut talk = asks_to_measure(
            "warum ist die platte voll",
            r#"{"intent":"space","ask":"why"}"#,
        );
        let measured = scan_without_worktrees(&talk, false);
        talk.send(Message::SpaceMeasured(measured));
        talk.send(empty_run());

        let labels = talk.labels();
        assert!(!labels.contains(&say(Msg::HermesChatSpaceShowDownloads, &[])));
        assert!(labels.contains(&say(Msg::HermesChatSpaceShowOldLogs, &[])));
    }

    fn asks_to_measure(sentence: &str, reply: &str) -> Talk {
        let mut talk = talk();
        talk.open();
        talk.type_text_understood(sentence, reply);
        assert!(
            talk.actions.contains(&Action::MeasureSpace),
            "{:?}",
            talk.actions
        );
        talk
    }

    fn the_card(talk: &Talk) -> &ncrs_hermes_chat::SpaceCard {
        talk.last_parts()
            .iter()
            .find_map(|part| match part {
                Part::Space(card) => Some(card),
                _ => None,
            })
            .expect("a card")
    }

    #[test]
    fn why_is_the_drive_full_measures_and_then_shows_the_card_with_the_sums() {
        let mut talk = asks_to_measure(
            "warum ist die platte voll",
            r#"{"intent":"space","ask":"why"}"#,
        );
        talk.model
            .push_notes(Ok(vec!["Vermutlich alte Sicherungen.".to_owned()]));
        let measured = scan(&talk);
        talk.send(Message::SpaceMeasured(measured));

        let card = the_card(&talk);
        assert_eq!(card.rows.len(), 3);
        assert_eq!(card.rows[0].size, gb(40));
        let decide = card.rows.iter().find(|row| row.described).unwrap();
        assert_eq!(decide.hint, "Vermutlich alte Sicherungen.");
        assert!(card
            .after
            .as_deref()
            .is_some_and(|after| after.contains(&gb(140))));
        assert_eq!(
            talk.labels(),
            [
                say(Msg::SpaceDeleteAllSafe, &[("size", &gb(40))]),
                say(Msg::SpaceRemoveWorktrees, &[("size", &gb(20))]),
                say(Msg::SpaceOpenOverview, &[]),
            ]
        );
        let asked = talk.model.noted();
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0][0].size_bytes, 30 * GB);
    }

    #[test]
    fn a_failed_description_keeps_the_reason_of_the_code_and_says_so() {
        let mut talk = asks_to_measure(
            "warum ist die platte voll",
            r#"{"intent":"space","ask":"why"}"#,
        );
        let measured = scan(&talk);
        talk.send(Message::SpaceMeasured(measured));

        let decide = the_card(&talk)
            .rows
            .iter()
            .find(|row| row.described)
            .unwrap()
            .clone();
        assert_eq!(decide.hint, say(Msg::SpaceDecideOtherCheck, &[]));
        assert!(talk
            .last_texts()
            .contains(&say(Msg::HermesChatSpaceDescribeFailed, &[])));
    }

    #[test]
    fn a_store_build_offers_no_worktree_button() {
        let mut talk = asks_to_measure(
            "warum ist die platte voll",
            r#"{"intent":"space","ask":"why"}"#,
        );
        talk.ctx.store_build = true;
        let measured = scan(&talk);
        talk.send(Message::SpaceMeasured(measured));

        assert!(talk
            .state
            .chips()
            .iter()
            .all(|chip| chip.reply != ChipReply::SpaceRemoveWorktrees));
        assert_eq!(talk.labels().len(), 2);
    }

    #[test]
    fn deleting_the_safe_ones_by_sentence_measures_and_never_runs() {
        let mut talk = asks_to_measure(
            "mach platz frei",
            r#"{"intent":"space","ask":"delete_safe"}"#,
        );
        let measured = scan(&talk);
        talk.send(Message::SpaceMeasured(measured));

        assert!(!talk
            .actions
            .iter()
            .any(|action| matches!(action, Action::SpaceRun(_))));
        talk.press(&ChipReply::SpaceDeleteSafe);
        assert_eq!(
            talk.actions.last(),
            Some(&Action::SpaceRun(SpaceRunKind::DeleteSafe))
        );
    }

    #[test]
    fn removing_worktrees_by_sentence_leads_to_the_card_and_its_button() {
        let mut talk = asks_to_measure(
            "räum die worktrees weg",
            r#"{"intent":"space","ask":"remove_worktrees"}"#,
        );
        let measured = scan(&talk);
        talk.send(Message::SpaceMeasured(measured));
        talk.press(&ChipReply::SpaceRemoveWorktrees);
        assert_eq!(
            talk.actions.last(),
            Some(&Action::SpaceRun(SpaceRunKind::RemoveWorktrees))
        );
    }

    #[test]
    fn the_overview_sentence_opens_the_platz_tab() {
        let mut talk = talk();
        talk.open();
        talk.type_text_understood(
            "zeig die platzübersicht",
            r#"{"intent":"space","ask":"overview"}"#,
        );
        assert_eq!(talk.actions.last(), Some(&Action::OpenSpace));
    }

    #[test]
    fn cmd_enter_on_the_card_is_the_delete_button_and_not_a_submit() {
        let mut talk = asks_to_measure(
            "warum ist die platte voll",
            r#"{"intent":"space","ask":"why"}"#,
        );
        let measured = scan(&talk);
        talk.send(Message::SpaceMeasured(measured));

        let key = chat_key(&Key::Named(Named::Enter), Modifiers::COMMAND);
        assert_eq!(key, ChatKey::SpaceDeleteSafe);
        let message = card_key_message(&talk.state, key).expect("the button is shown");
        assert!(matches!(
            &message,
            Message::ChipPressed {
                reply: ChipReply::SpaceDeleteSafe,
                ..
            }
        ));
        talk.send(message);
        assert_eq!(
            talk.actions.last(),
            Some(&Action::SpaceRun(SpaceRunKind::DeleteSafe))
        );
    }

    #[test]
    fn a_card_key_without_its_button_does_nothing() {
        let mut talk = talk();
        talk.open();
        assert_eq!(
            card_key_message(&talk.state, ChatKey::SpaceDeleteSafe),
            None
        );
    }

    #[test]
    fn the_result_of_a_run_is_told_with_what_was_left_standing() {
        let mut talk = asks_to_measure(
            "warum ist die platte voll",
            r#"{"intent":"space","ask":"why"}"#,
        );
        let measured = scan(&talk);
        talk.send(Message::SpaceMeasured(measured));
        let log = RunLog {
            started: SystemTime::UNIX_EPOCH,
            finished: SystemTime::UNIX_EPOCH,
            rows: vec![
                LogRow {
                    path: talk.ctx.home.join("target"),
                    size: 40 * GB,
                    outcome: Ok(40 * GB),
                    last_used: None,
                    use_checked: false,
                },
                LogRow {
                    path: talk.ctx.home.join("wt"),
                    size: 20 * GB,
                    outcome: Err(Skip::Changed),
                    last_used: None,
                    use_checked: false,
                },
            ],
        };
        talk.send(Message::SpaceRunDone(Arc::new(log)));

        let texts = talk.last_texts();
        assert_eq!(texts[0], say(Msg::SpaceResultFreed, &[("size", &gb(40))]));
        assert_eq!(
            texts[1],
            say(
                Msg::HermesChatSpaceLeftStanding,
                &[("name", "wt"), ("reason", &say(Msg::SpaceSkipChanged, &[]))]
            )
        );
        assert!(matches!(
            talk.state.thread().last(),
            Some(Entry::Hermes { .. })
        ));
    }
}
