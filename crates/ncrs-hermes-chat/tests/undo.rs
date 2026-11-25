//! Taking the last run back, the only work of the chat that touches files: it
//! is asked about first and done on the button alone.

mod common;

#[cfg(test)]
mod tests {
    use crate::common::{say, Talk};
    use ncrs_filing::chat_work::{ChatResult, ChatWork, RunSummary};
    use ncrs_hermes_chat::{Action, ChipReply, Message};
    use ncrs_i18n::Msg;
    use ncrs_index::{BatchId, BatchKind, Transfer};

    /// A talk that asks whether to take back a run of three moved files.
    fn asked_to_undo(talk: &mut Talk) -> BatchId {
        let batch = talk
            .index
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .begin_batch(BatchKind::Hermes)
            .unwrap();
        talk.send(Message::Open);
        talk.send(Message::Done(Box::new(ChatResult::LastRun(Ok(Some(
            RunSummary {
                batch,
                name: "inbox".to_owned(),
                transfer: Transfer::Move,
                items: 3,
                finished_at: 1_700_000_000,
                source_folder: Some(talk.path("inbox")),
                automatic: false,
            },
        ))))));
        batch
    }

    fn talk() -> Talk {
        Talk::in_dir(tempfile::tempdir().unwrap(), &["archive"], &["inbox"])
    }

    fn undone(talk: &Talk) -> Vec<BatchId> {
        talk.works
            .iter()
            .filter_map(|work| match work {
                ChatWork::Undo { batch } => Some(*batch),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_run_is_taken_back_on_the_button_and_not_before() {
        let mut talk = talk();
        let batch = asked_to_undo(&mut talk);
        assert_eq!(
            talk.labels(),
            vec![
                say(Msg::HermesChatPutBack, &[]),
                say(Msg::HermesChatCancel, &[])
            ]
        );
        assert!(undone(&talk).is_empty(), "{:?}", talk.works);

        talk.press(&ChipReply::PutBack);

        assert_eq!(undone(&talk), vec![batch]);
    }

    #[test]
    fn cancelling_leaves_the_run_as_it_is() {
        let mut talk = talk();
        asked_to_undo(&mut talk);

        talk.press(&ChipReply::Cancel);

        assert_eq!(talk.last_texts(), vec![say(Msg::HermesChatCancelled, &[])]);
        assert!(undone(&talk).is_empty(), "{:?}", talk.works);
    }

    #[test]
    fn nothing_is_taken_back_while_changes_are_locked() {
        let mut talk = talk();
        asked_to_undo(&mut talk);
        talk.ctx.changes_allowed = false;

        talk.press(&ChipReply::PutBack);

        assert_eq!(talk.actions, vec![Action::ChangesLocked]);
        assert!(undone(&talk).is_empty(), "{:?}", talk.works);
    }
}
