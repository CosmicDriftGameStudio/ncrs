//! Conversation 1: reading a folder, keeping its order, changing it for the
//! session.

use std::path::PathBuf;
use std::sync::Arc;

use ncrs_filing::chat_work::{ChatAccess, ChatWork, FolderReading, ReadError, ReadProgress};
use ncrs_i18n::Msg;
use ncrs_index::{KindCatalog, KindLanguage, Kinds};
use ncrs_ui_kit::hermes_state::FolderRole;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::state::{Chip, ChipReply, Offer, Part, ReadFor, Retry, State, Step};
use crate::texts::{areas_card, fill, folder_name, number, text, SHOWN_AREAS};
use crate::tidy;
use crate::update::{
    access, chip, chip_labelled, hermes, lost_folder_chips, lost_folder_retry_chips, say,
    source_chips, work,
};

pub fn read(state: &mut State, ctx: &Ctx, folder: PathBuf) {
    read_for(state, ctx, folder, ReadFor::Learn);
}

pub fn read_for(state: &mut State, ctx: &Ctx, folder: PathBuf, purpose: ReadFor) {
    if let Some(reading) = state.session.readings.get(&folder).cloned() {
        deliver(state, ctx, purpose, &reading);
        return;
    }
    state.session.read_for = Some(purpose);
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
    let purpose = state.session.read_for.take().unwrap_or(ReadFor::Learn);
    state.session.progress = None;
    match result {
        Ok(reading) => {
            state
                .session
                .readings
                .insert(reading.root.clone(), Arc::clone(&reading));
            deliver(state, ctx, purpose, &reading);
        }
        Err(error) => {
            state.session.tidy = None;
            state.session.resume_tidy = false;
            state.step = Step::Idle;
            let requested = state.session.requested.clone();
            let can_retry = matches!(error, ReadError::Gone | ReadError::NotADirectory)
                && matches!(purpose, ReadFor::Learn | ReadFor::Source | ReadFor::Open);
            state.session.retry = can_retry.then(|| Retry {
                folder: requested.clone(),
                purpose,
                unresolved_in: None,
            });
            let sentence = match error {
                ReadError::Gone => fill(
                    ctx.lang,
                    Msg::HermesChatFolderGone,
                    &[("path", &requested.to_string_lossy())],
                ),
                ReadError::NotADirectory => fill(
                    ctx.lang,
                    Msg::HermesChatNotAFolder,
                    &[("name", &folder_name(ctx, &requested))],
                ),
                ReadError::TooMany { limit } => fill(
                    ctx.lang,
                    Msg::HermesChatTooMany,
                    &[
                        ("folder", &folder_name(ctx, &requested)),
                        ("limit", &number(limit)),
                    ],
                ),
                ReadError::Io(reason) => {
                    fill(ctx.lang, Msg::HermesChatReadFailed, &[("reason", &reason)])
                }
            };
            let chips = lost_folder_retry_chips(state, ctx);
            say(state, sentence, chips);
        }
    }
}

/// Reads the folder again that could not be read.
pub fn retry(state: &mut State, ctx: &Ctx) {
    let Some(failed) = state.session.retry.take() else {
        return;
    };
    if let Some(step) = failed.unresolved_in {
        state.resolving = Some((failed.folder, step));
        state.fresh_resolve = true;
        return;
    }
    match failed.purpose {
        ReadFor::Source => tidy::begin(state, ctx, failed.folder),
        ReadFor::Open => {
            crate::rule_flow::open(state, ctx, failed.folder);
        }
        ReadFor::Learn | ReadFor::Target | ReadFor::NewTarget => read(state, ctx, failed.folder),
    }
}

fn deliver(state: &mut State, ctx: &Ctx, purpose: ReadFor, reading: &Arc<FolderReading>) {
    match purpose {
        ReadFor::Open => crate::rule_flow::folder_read(state, ctx, reading),
        ReadFor::Learn => summary(state, ctx, reading),
        ReadFor::Source => tidy::source_read(state, ctx, reading),
        ReadFor::Target => tidy::target_read(state, ctx),
        ReadFor::NewTarget => crate::decide::new_target_read(state, ctx, reading),
    }
}

pub fn summary(state: &mut State, ctx: &Ctx, reading: &Arc<FolderReading>) {
    let folder = folder_name(ctx, &reading.root);
    if reading.files.is_empty() {
        state.step = Step::Idle;
        state.session.resume_tidy = false;
        say(
            state,
            fill(ctx.lang, Msg::HermesChatReadEmpty, &[("folder", &folder)]),
            lost_folder_chips(ctx),
        );
        return;
    }
    state.session.learning = Some(Arc::clone(reading));
    let count = number(reading.files.len());
    let areas = reading.structure.areas.len();
    if areas == 0 {
        state.step = Step::Idle;
        state.session.resume_tidy = false;
        say(
            state,
            fill(
                ctx.lang,
                Msg::HermesChatReadSummaryNoOrder,
                &[("count", &count)],
            ),
            lost_folder_chips(ctx),
        );
        return;
    }
    let intro = if areas == 1 {
        fill(
            ctx.lang,
            Msg::HermesChatReadSummaryOneArea,
            &[("count", &count)],
        )
    } else {
        fill(
            ctx.lang,
            Msg::HermesChatReadSummary,
            &[("count", &count), ("areas", &number(areas))],
        )
    };
    state.step = Step::Keep;
    let chips = keep_chips(ctx, areas > SHOWN_AREAS, areas);
    let mut parts = vec![
        Part::Text(intro),
        Part::Areas(areas_card(
            ctx.lang,
            &folder_name(ctx, &reading.root),
            reading,
            false,
        )),
    ];
    parts.extend(ai_progress(ctx, reading));
    parts.push(Part::Text(text(ctx.lang, Msg::HermesChatAskKeep)));
    hermes(state, parts, chips);
}

fn keep_chips(ctx: &Ctx, with_all: bool, areas: usize) -> Vec<Chip> {
    let mut chips = vec![chip(ctx, Msg::HermesChatKeepYes, ChipReply::KeepYes, true)];
    if with_all {
        chips.push(chip_labelled(
            fill(
                ctx.lang,
                Msg::HermesChatShowAll,
                &[("areas", &number(areas))],
            ),
            ChipReply::ShowAll,
            false,
        ));
    }
    chips.push(chip(
        ctx,
        Msg::HermesChatOtherFolder,
        ChipReply::PickFolder,
        false,
    ));
    chips
}

pub fn show_all(state: &mut State, ctx: &Ctx) {
    let Some(reading) = state.session.learning.clone() else {
        return;
    };
    state.step = Step::Keep;
    let chips = keep_chips(ctx, false, reading.structure.areas.len());
    hermes(
        state,
        vec![
            Part::Areas(areas_card(
                ctx.lang,
                &folder_name(ctx, &reading.root),
                &reading,
                true,
            )),
            Part::Text(text(ctx.lang, Msg::HermesChatAskKeep)),
        ],
        chips,
    );
}

/// The order is kept: the folder becomes a filing location, its duplicates
/// wait for the first filing, and the talk moves on.
pub fn keep(state: &mut State, ctx: &Ctx) -> Option<Action> {
    let reading = state.session.learning.clone()?;
    crate::follow_up::forget(state);
    let folder = reading.root.clone();
    let known = ctx.targets.contains(&folder);
    let role = (!known).then(|| Action::SetFolderRole {
        folder: folder.clone(),
        role: FolderRole::FilingLocation,
    });
    if !known && !state.session.learned.contains(&folder) {
        state.session.learned.push(folder.clone());
    }
    let shown = folder_name(ctx, &folder);
    let mut parts = vec![Part::Text(fill(
        ctx.lang,
        Msg::HermesChatKept,
        &[("folder", &shown)],
    ))];
    offer_duplicates(state, ctx, &reading);
    if state.session.resume_tidy {
        hermes(state, parts, Vec::new());
        tidy::resume(state, ctx);
    } else {
        parts.push(Part::Text(text(ctx.lang, Msg::HermesChatAskSource)));
        state.step = Step::Source;
        hermes(state, parts, source_chips(state, ctx));
    }
    role
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

/// The duplicates of the first archive read wait for the first filing.
pub fn offer_duplicates(state: &mut State, ctx: &Ctx, reading: &FolderReading) {
    let can_trash = access(ctx).is_some_and(|access| access.trash_can_restore());
    if can_trash
        && !reading.duplicates.is_empty()
        && !state.session.archive_offered
        && state.session.archive_duplicates.is_none()
    {
        state.session.archive_duplicates = Some(Offer {
            name: fill(
                ctx.lang,
                Msg::HermesChatTrashRunName,
                &[("folder", &folder_name(ctx, &reading.root))],
            ),
            root: reading.root.clone(),
            items: reading.duplicates.clone(),
        });
    }
}

/// The kinds a typed word is compared with, in the language the reading uses.
pub fn word_kinds(ctx: &Ctx) -> Kinds {
    let language = access(ctx).map_or_else(KindLanguage::default, ChatAccess::kind_language);
    Kinds::new(Arc::new(KindCatalog::built_in()), language)
}
