//! The state changes of Quick Open. What reaches outside it comes back as an
//! action.

use iced::Task;

use super::action::QuickOpenAction;
use super::ctx::QuickOpenCtx;
use super::QuickOpen;
use crate::message::QuickOpenMessage;

type Outcome = (Task<QuickOpenMessage>, Option<QuickOpenAction>);

fn only(action: QuickOpenAction) -> Outcome {
    (Task::none(), Some(action))
}

/// Applies a message to the open box, if any.
pub fn update(
    quick_open: &mut Option<QuickOpen>,
    message: QuickOpenMessage,
    ctx: &QuickOpenCtx,
) -> Outcome {
    match message {
        QuickOpenMessage::Open => return only(QuickOpenAction::Open),
        QuickOpenMessage::Close => return only(QuickOpenAction::Close),
        _ => {}
    }
    let Some(open) = quick_open.as_mut() else {
        return (Task::none(), None);
    };
    match message {
        QuickOpenMessage::QueryChanged(text) => {
            if let Some(rest) = text.strip_prefix('>') {
                return only(QuickOpenAction::SwitchToCommands(rest.to_owned()));
            }
            let ranking = open
                .set_query(text)
                .map(|job| Task::perform(job.run_in_background(), std::convert::identity));
            // Apart from the name ranking: the names never wait for it.
            let documents = request_documents(open, ctx);
            (Task::batch(ranking), documents)
        }
        QuickOpenMessage::DocumentsFound {
            walk_generation,
            generation,
            result,
        } => {
            open.apply_documents(walk_generation, generation, result);
            (Task::none(), None)
        }
        QuickOpenMessage::Events { generation, events } => {
            if generation == open.walk_generation() {
                open.apply_events(events);
                if !open.walking() {
                    return only(QuickOpenAction::WalkFinished);
                }
            }
            (Task::none(), None)
        }
        QuickOpenMessage::Ranked {
            walk_generation,
            generation,
            hits,
            chunks_scored,
        } => {
            open.apply_ranked(walk_generation, generation, hits, chunks_scored);
            (Task::none(), None)
        }
        QuickOpenMessage::Move(delta) => {
            open.move_cursor(delta);
            (Task::none(), None)
        }
        QuickOpenMessage::Activate => activate(open),
        QuickOpenMessage::RowActivated(index) => {
            open.select(index);
            activate(open)
        }
        QuickOpenMessage::Open | QuickOpenMessage::Close => (Task::none(), None),
    }
}

/// Asks for the documents of the typed text. Without an index the section
/// stays away.
fn request_documents(open: &mut QuickOpen, ctx: &QuickOpenCtx) -> Option<QuickOpenAction> {
    let wanted = open.request_documents()?;
    if ctx.documents_available {
        Some(QuickOpenAction::FindDocuments(wanted))
    } else {
        open.apply_documents(
            wanted.walk_generation,
            wanted.generation,
            Err(String::from("no index")),
        );
        None
    }
}

/// Enter on a hit.
fn activate(open: &mut QuickOpen) -> Outcome {
    // Typed fast, Enter before the background ranking is back.
    open.rank_now();
    if open.all_hits_selected() {
        return only(QuickOpenAction::OpenSearch(open.query().to_owned()));
    }
    match open.selected_path() {
        Some(path) => only(QuickOpenAction::Reveal(path)),
        None => (Task::none(), None),
    }
}
