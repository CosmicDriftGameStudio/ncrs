use std::path::PathBuf;

use iced::Task;

use crate::ctx::Ctx;
use crate::message::Message;

/// Looks up `folder` on a blocking thread and answers with what it found.
pub(in crate::update) fn resolve_in_background(
    ctx: &Ctx<'_>,
    folder: PathBuf,
    answer: impl FnOnce(PathBuf, Result<PathBuf, String>) -> Message + Send + 'static,
) -> Task<Message> {
    let resolve_folder = ctx.resolve_folder;
    Task::perform(
        tokio::task::spawn_blocking({
            let looked_up = folder.clone();
            move || resolve_folder(&looked_up)
        }),
        move |joined| answer(folder, joined.unwrap_or_else(|join| Err(join.to_string()))),
    )
}
