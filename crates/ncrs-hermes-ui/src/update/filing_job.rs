use iced::Task;
use ncrs_filing::filing::{FilingMessage, Work, WorkOutcome};

use crate::message::Message;

/// Runs one blocking step of the filing module off the UI thread.
pub fn run_filing_work(work: Work) -> Task<Message> {
    run_filing_work_after(None, work)
}

/// Like [`run_filing_work`], with the pipeline dropped first on the same
/// blocking thread: joining its threads blocks, and the work needs the index
/// free of them.
pub fn run_filing_work_after(pipeline: Option<ncrs_pipeline::Runner>, work: Work) -> Task<Message> {
    let generation = work.generation();
    Task::perform(
        tokio::task::spawn_blocking(move || {
            drop(pipeline);
            work.run()
        }),
        move |joined| {
            Message::Filing(joined.unwrap_or_else(|join| FilingMessage::WorkDone {
                generation,
                outcome: WorkOutcome::Crashed(join.to_string()),
            }))
        },
    )
}
