//! The lookup of document texts, which Quick Open runs off the UI thread.

use iced::Task;
use ncrs_core::FoundDocument;

use super::{DocumentRequest, MAX_DOCUMENT_HITS};
use crate::message::QuickOpenMessage;

/// Looks the documents of `request` up with `find`, on a blocking thread.
/// `find` gets the words and the most hits to return.
pub fn find_documents(
    request: DocumentRequest,
    find: impl FnOnce(&str, usize) -> Result<Vec<FoundDocument>, String> + Send + 'static,
) -> Task<QuickOpenMessage> {
    let DocumentRequest {
        walk_generation,
        generation,
        words,
    } = request;
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || find(&words, MAX_DOCUMENT_HITS))
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
