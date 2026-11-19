//! The lookup of document texts, which Quick Open runs off the UI thread.

use iced::Task;
use ncrs_core::FoundDocument;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ncrs_fs::search::{all_files, search_events, stays_on_file_system, SearchEvent};

use super::{DocumentRequest, QuickOpen, WalkEvent, MAX_DOCUMENT_HITS, QUICK_OPEN_FILE_LIMIT};
use crate::message::QuickOpenMessage;

/// Counts the walks started, so a batch of a box that has been closed cannot
/// be taken for one of the box opened next.
static WALKS_STARTED: AtomicU64 = AtomicU64::new(0);

fn walk_batch(generation: u64, events: Vec<SearchEvent>) -> QuickOpenMessage {
    let walk_events = events
        .into_iter()
        .filter_map(|event| match event {
            SearchEvent::File { path, .. } => Some(WalkEvent::File { path }),
            SearchEvent::Folder { .. } | SearchEvent::Progress { .. } => None,
            SearchEvent::Finished(totals) => Some(WalkEvent::Finished {
                truncated: totals.truncated,
            }),
        })
        .collect();
    QuickOpenMessage::Events {
        generation,
        events: walk_events,
    }
}

/// A box over the files below `root`, and the task that walks them. The walk
/// stops when the box is dropped.
pub fn open(root: PathBuf, documents_enabled: bool) -> (QuickOpen, Task<QuickOpenMessage>) {
    let generation = WALKS_STARTED.fetch_add(1, Ordering::Relaxed) + 1;
    let walk = all_files(
        root.clone(),
        stays_on_file_system(&root),
        QUICK_OPEN_FILE_LIMIT,
    );
    let (task, handle) = Task::run(
        search_events(walk, generation, walk_batch),
        std::convert::identity,
    )
    .abortable();
    let mut quick_open = QuickOpen::new(root, generation).with_documents(documents_enabled);
    quick_open.set_walk(handle.abort_on_drop());
    (quick_open, task)
}

/// Looks the documents of `request` up with `find`, on a blocking thread.
/// `find` gets the words and the most hits to return.
pub fn find_documents(
    request: DocumentRequest,
    find: impl FnOnce(&str, usize) -> Result<Vec<FoundDocument>, String> + Send + 'static,
) -> Task<QuickOpenMessage> {
    let (walk_generation, generation) = (request.walk_generation, request.generation);
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                if request.is_current() {
                    find(&request.words, MAX_DOCUMENT_HITS)
                } else {
                    Ok(Vec::new())
                }
            })
            .await
            .unwrap_or_else(|failure| Err(failure.to_string()))
        },
        move |result| QuickOpenMessage::DocumentsFound {
            walk_generation,
            generation,
            result,
        },
    )
}
