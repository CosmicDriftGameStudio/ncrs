//! What the chat says when the "Filing" tab finished a run: the report and the
//! button that puts that very run back.

mod common;

#[cfg(test)]
mod tests {
    use crate::common::{say, Talk};
    use ncrs_filing::chat_work::ChatWork;
    use ncrs_hermes_chat::{filed_in_tab, update, ChipReply, FiledInTab, Message};
    use ncrs_i18n::Msg;
    use ncrs_index::{BatchId, BatchKind, Transfer};

    fn new_batch(talk: &Talk) -> BatchId {
        let guard = talk.index.lock().unwrap();
        guard
            .as_ref()
            .unwrap()
            .begin_batch(BatchKind::Hermes)
            .unwrap()
    }

    fn finished(talk: &mut Talk, transfer: Transfer, batch: BatchId) {
        let source = talk.path("inbox");
        filed_in_tab(
            &mut talk.state,
            &talk.ctx,
            FiledInTab {
                source,
                transfer,
                filed: 7,
                left: 3,
                batch,
            },
        );
    }

    #[test]
    fn a_run_of_the_tab_is_reported_with_what_is_left() {
        let mut talk = Talk::in_dir(tempfile::tempdir().unwrap(), &["archive"], &["inbox"]);
        let move_batch = new_batch(&talk);
        finished(&mut talk, Transfer::Move, move_batch);
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatFiledInTabMove,
                &[("filed", "7"), ("left", "3"), ("folder", "inbox")]
            )]
        );
        let copy_batch = new_batch(&talk);
        finished(&mut talk, Transfer::Copy, copy_batch);
        assert_eq!(
            talk.last_texts(),
            vec![say(
                Msg::HermesChatFiledInTabCopy,
                &[("filed", "7"), ("left", "3"), ("folder", "inbox")]
            )]
        );
    }

    #[test]
    fn the_undo_chip_puts_back_the_batch_of_the_tab_run() {
        let mut talk = Talk::in_dir(tempfile::tempdir().unwrap(), &["archive"], &["inbox"]);
        let batch = new_batch(&talk);
        finished(&mut talk, Transfer::Move, batch);
        let chip = talk.state.chips()[0].clone();
        assert_eq!(chip.reply, ChipReply::PutBack);
        let (_task, action) = update(
            &mut talk.state,
            Message::ChipPressed {
                label: chip.label,
                reply: chip.reply,
            },
            &talk.ctx,
        );
        assert_eq!(action, None);
        assert!(matches!(
            talk.state.pending_work(),
            Some(ChatWork::Undo { batch: undone }) if *undone == batch
        ));
    }
}
