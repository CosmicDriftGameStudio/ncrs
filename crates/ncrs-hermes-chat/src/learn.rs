//! Reading a folder.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ncrs_filing::chat_work::{ChatAccess, ChatWork, FolderReading, ReadError, ReadProgress};
use ncrs_i18n::Msg;
use ncrs_index::{KindCatalog, KindLanguage, Kinds};

use crate::ctx::Ctx;
use crate::rule_flow;
use crate::state::{Part, Retry, State, Step};
use crate::texts::{fill, folder_name, number};
use crate::update::{access, lost_folder_retry_chips, say, work};

pub fn read(state: &mut State, ctx: &Ctx, folder: PathBuf) {
    state.session.requested = folder.clone();
    let shown = folder_name(ctx, &folder);
    let progress = Arc::new(ReadProgress::default());
    state.session.progress = Some(Arc::clone(&progress));
    work(
        state,
        ChatWork::Read { folder, progress },
        Some(fill(
            ctx.lang,
            Msg::HermesChatWorkingReading,
            &[("folder", &shown)],
        )),
    );
}

pub fn read_done(state: &mut State, ctx: &Ctx, result: Result<Arc<FolderReading>, ReadError>) {
    state.session.progress = None;
    match result {
        Ok(reading) => rule_flow::folder_read(state, ctx, &reading),
        Err(error) => {
            let requested = state.session.requested.clone();
            state.session.rule = None;
            state.step = Step::Idle;
            let can_retry = matches!(error, ReadError::Gone | ReadError::NotADirectory);
            let sentence = read_error_text(ctx, &error, &requested);
            state.session.retry = can_retry.then_some(Retry {
                folder: requested,
                unresolved_in: None,
            });
            let chips = lost_folder_retry_chips(state, ctx);
            say(state, sentence, chips);
        }
    }
}

fn read_error_text(ctx: &Ctx, error: &ReadError, requested: &Path) -> String {
    match error {
        ReadError::Gone => fill(
            ctx.lang,
            Msg::HermesChatFolderGone,
            &[("path", &requested.to_string_lossy())],
        ),
        ReadError::NotADirectory => fill(
            ctx.lang,
            Msg::HermesChatNotAFolder,
            &[("name", &folder_name(ctx, requested))],
        ),
        ReadError::TooMany { limit } => fill(
            ctx.lang,
            Msg::HermesChatTooMany,
            &[
                ("folder", &folder_name(ctx, requested)),
                ("limit", &number(*limit)),
            ],
        ),
        ReadError::Io(reason) => fill(ctx.lang, Msg::HermesChatReadFailed, &[("reason", reason)]),
    }
}

/// Reads the folder again that could not be read.
pub fn retry(state: &mut State, ctx: &Ctx) {
    let Some(failed) = state.session.retry.take() else {
        return;
    };
    if let Some(step) = failed.unresolved_in {
        state.step = step;
        state.resolving = Some((failed.folder, step));
        state.fresh_resolve = true;
        return;
    }
    rule_flow::open(state, ctx, failed.folder);
}

/// How far the AI is with the documents of the reading, while some still wait.
pub fn ai_progress(ctx: &Ctx, reading: &FolderReading) -> Option<Part> {
    (!reading.ai.is_complete()).then(|| {
        Part::Text(fill(
            ctx.lang,
            Msg::HermesChatReadingAiProgress,
            &[
                ("done", &number(reading.ai.understood)),
                ("total", &number(reading.ai.with_text)),
            ],
        ))
    })
}

/// The kinds a typed word is compared with, in the language the reading uses.
pub fn word_kinds(ctx: &Ctx) -> Kinds {
    let language = access(ctx).map_or_else(KindLanguage::default, ChatAccess::kind_language);
    Kinds::new(Arc::new(KindCatalog::built_in()), language)
}
