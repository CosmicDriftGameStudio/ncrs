//! Conversation 1: reading a folder, keeping its order, changing it for the
//! session.

use std::path::PathBuf;
use std::sync::Arc;

use ncrs_filing::chat_work::{
    same_word, ChangeOrder, ChangePreview, ChatWork, FolderReading, OrderChange, ReadError,
};
use ncrs_i18n::Msg;
use ncrs_ui_kit::hermes_state::FolderRole;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::state::{Chip, ChipReply, FileCard, Offer, Part, ReadFor, State, Step};
use crate::texts::{areas_card, fill, folder_name, number, plural_text, text};
use crate::update::{
    access, cancel_chip, chip, chip_labelled, hermes, say, source_chips, target_chips, work,
};
use crate::{patterns, tidy};

const SCOPE_VALUES: usize = 3;
const AREA_CHOICES: usize = 6;

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
    work(
        state,
        ChatWork::Read { folder },
        Some(fill(
            ctx.lang,
            Msg::HermesChatWorkingReading,
            &[("folder", &shown)],
        )),
    );
}

pub fn read_done(state: &mut State, ctx: &Ctx, result: Result<Arc<FolderReading>, ReadError>) {
    let purpose = state.session.read_for.take().unwrap_or(ReadFor::Learn);
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
            let sentence = match error {
                ReadError::NotADirectory => fill(
                    ctx.lang,
                    Msg::HermesChatNotAFolder,
                    &[("path", &requested.to_string_lossy())],
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
            say(state, sentence, Vec::new());
        }
    }
}

fn deliver(state: &mut State, ctx: &Ctx, purpose: ReadFor, reading: &Arc<FolderReading>) {
    match purpose {
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
            Vec::new(),
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
            Vec::new(),
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
    let chips = keep_chips(state, ctx, true);
    hermes(
        state,
        vec![
            Part::Text(intro),
            Part::Areas(areas_card(
                ctx.lang,
                &folder_name(ctx, &reading.root),
                &reading,
                false,
            )),
            Part::Text(text(ctx.lang, Msg::HermesChatAskKeep)),
        ],
        chips,
    );
}

fn keep_chips(state: &mut State, ctx: &Ctx, with_all: bool) -> Vec<Chip> {
    let mut chips = vec![
        chip(ctx, Msg::HermesChatKeepYes, ChipReply::KeepYes, true),
        chip(ctx, Msg::HermesChatWantChange, ChipReply::WantChange, false),
    ];
    if with_all {
        chips.push(chip(ctx, Msg::HermesChatShowAll, ChipReply::ShowAll, false));
    }
    chips.extend(patterns::chips(state, ctx));
    chips
}

pub fn show_all(state: &mut State, ctx: &Ctx) {
    let Some(reading) = state.session.learning.clone() else {
        return;
    };
    state.step = Step::Keep;
    let chips = keep_chips(state, ctx, false);
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

pub fn want_change(state: &mut State, ctx: &Ctx) {
    state.step = Step::Change;
    say(state, text(ctx.lang, Msg::HermesChatAskChange), Vec::new());
}

/// The order is kept: the folder becomes a filing location, its duplicates
/// wait for the first filing, and the talk moves on.
pub fn keep(state: &mut State, ctx: &Ctx) -> Option<Action> {
    let reading = state.session.learning.clone()?;
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
    let can_trash = access(ctx).is_some_and(|access| access.trash_can_restore());
    if can_trash
        && !reading.duplicates.is_empty()
        && !state.session.archive_offered
        && state.session.archive_duplicates.is_none()
    {
        state.session.archive_duplicates = Some(Offer {
            name: fill(ctx.lang, Msg::HermesChatTrashRunName, &[("folder", &shown)]),
            root: reading.root.clone(),
            items: reading.duplicates.clone(),
        });
    }
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

pub fn change_order(state: &mut State, ctx: &Ctx, kind_word: String, order: ChangeOrder) {
    state.session.change_order = Some(order);
    let Some(reading) = state.session.learning.clone() else {
        state.step = Step::ReadFolder;
        state.session.read_for = Some(ReadFor::Learn);
        say(
            state,
            text(ctx.lang, Msg::HermesChatChangeNeedsReading),
            target_chips(state, ctx),
        );
        return;
    };
    state.session.change_word = kind_word.clone();
    work(
        state,
        ChatWork::PreviewChange {
            reading,
            kind_word,
            order,
        },
        None,
    );
}

pub fn preview_done(state: &mut State, ctx: &Ctx, computed: Option<ChangePreview>) {
    let wanted = state.session.change_scope.take();
    let Some(mut preview) = computed else {
        if ask_area(state, ctx) {
            return;
        }
        state.step = Step::AfterChange;
        let word = state.session.change_word.clone();
        say(
            state,
            fill(ctx.lang, Msg::HermesChatChangeNotFound, &[("word", &word)]),
            vec![chip(
                ctx,
                Msg::HermesChatFineAsIs,
                ChipReply::FineAsIs,
                true,
            )],
        );
        return;
    };
    let mut parts = vec![
        Part::Text(fill(
            ctx.lang,
            Msg::HermesChatChangePreview,
            &[("kind", &preview.kind_label)],
        )),
        Part::File(FileCard {
            segments: preview.example.clone(),
            highlighted: Some(preview.highlighted),
        }),
    ];
    let Some(scope) = preview.scope.clone() else {
        confirm(state, ctx, parts, preview);
        return;
    };
    let said = wanted.and_then(|word| {
        scope
            .values
            .iter()
            .find(|value| same_word(&word, value))
            .cloned()
    });
    if let Some(value) = said {
        preview.change.only = Some((scope.depth, value));
        confirm(state, ctx, parts, preview);
        return;
    }
    let plural = plural_text(ctx.lang, scope.label);
    parts.push(Part::Text(fill(
        ctx.lang,
        Msg::HermesChatChangeScope,
        &[("plural", plural)],
    )));
    let mut chips = vec![chip_labelled(
        fill(ctx.lang, Msg::HermesChatScopeAll, &[("plural", plural)]),
        ChipReply::ScopeAll,
        true,
    )];
    for value in scope.values.iter().take(SCOPE_VALUES) {
        chips.push(chip_labelled(
            fill(ctx.lang, Msg::HermesChatScopeOnly, &[("value", value)]),
            ChipReply::ScopeOnly(value.clone()),
            false,
        ));
    }
    chips.push(cancel_chip(ctx));
    state.session.pending_change = Some(preview);
    state.step = Step::Scope;
    hermes(state, parts, chips);
}

/// No folder fits the word: the areas to choose from, so the order asked for
/// is not lost. False when it was asked already or there is no area.
fn ask_area(state: &mut State, ctx: &Ctx) -> bool {
    let (Some(reading), Some(_)) = (state.session.learning.clone(), state.session.change_order)
    else {
        return false;
    };
    if state.session.area_chosen || reading.structure.areas.is_empty() {
        return false;
    }
    let mut areas: Vec<&ncrs_intake::Area> = reading.structure.areas.iter().collect();
    areas.sort_by(|left, right| right.documents.cmp(&left.documents));
    let mut chips: Vec<Chip> = areas
        .into_iter()
        .take(AREA_CHOICES)
        .map(|area| {
            chip_labelled(
                area.folder.clone(),
                ChipReply::ChangeArea(area.folder.clone()),
                false,
            )
        })
        .collect();
    chips.push(chip(
        ctx,
        Msg::HermesChatOtherFolder,
        ChipReply::PickFolder,
        false,
    ));
    chips.push(cancel_chip(ctx));
    state.step = Step::AfterChange;
    say(state, text(ctx.lang, Msg::HermesChatChangeAreaAsk), chips);
    true
}

/// The area the user picked is the word the change is looked up with.
pub fn area_chosen(state: &mut State, ctx: &Ctx, area: String) {
    let Some(order) = state.session.change_order else {
        return;
    };
    state.session.area_chosen = true;
    change_order(state, ctx, area, order);
}

/// The preview is the question; the yes applies the change.
fn confirm(state: &mut State, ctx: &Ctx, parts: Vec<Part>, preview: ChangePreview) {
    state.session.pending_change = Some(preview);
    state.step = Step::ConfirmChange;
    hermes(
        state,
        parts,
        vec![
            chip(
                ctx,
                Msg::HermesChatConfirmYes,
                ChipReply::ConfirmChange,
                true,
            ),
            cancel_chip(ctx),
        ],
    );
}

pub fn confirm_change(state: &mut State, ctx: &Ctx) {
    if let Some(change) = state.session.pending_pattern.take() {
        apply(state, ctx, Vec::new(), change);
        return;
    }
    let Some(preview) = state.session.pending_change.take() else {
        return;
    };
    apply(state, ctx, Vec::new(), preview.change);
}

pub fn scope(state: &mut State, ctx: &Ctx, only: Option<String>) {
    let Some(preview) = state.session.pending_change.take() else {
        return;
    };
    let mut change = preview.change;
    if let (Some(value), Some(scope)) = (only, preview.scope) {
        change.only = Some((scope.depth, value));
    }
    apply(state, ctx, Vec::new(), change);
}

fn apply(state: &mut State, ctx: &Ctx, mut parts: Vec<Part>, change: OrderChange) {
    state.session.changes.push(change);
    parts.push(Part::Text(text(ctx.lang, Msg::HermesChatChangeDone)));
    parts.push(Part::Text(text(ctx.lang, Msg::HermesChatAnythingElse)));
    state.step = Step::AfterChange;
    hermes(
        state,
        parts,
        vec![chip(
            ctx,
            Msg::HermesChatFineAsIs,
            ChipReply::FineAsIs,
            true,
        )],
    );
}
