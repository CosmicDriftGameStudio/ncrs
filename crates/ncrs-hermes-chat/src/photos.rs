//! Photos and scans of a source that need text recognition: asked about
//! before the files are filed, once the models are there they are read too.

use std::sync::Arc;

use ncrs_filing::chat_work::FolderReading;
use ncrs_i18n::Msg;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::state::{ChipReply, ReadFor, State, Step};
use crate::texts::{fill, folder_name, number, text};
use crate::update::{chip, say};
use crate::{learn, tidy};

/// Asks about the photos of a source when text recognition could read them
/// but its models are missing; else goes on with the target.
pub fn ask_or_go_on(state: &mut State, ctx: &Ctx, reading: &Arc<FolderReading>) {
    if reading.photos == 0 || ctx.ocr_models_present {
        tidy::choose_where(state, ctx, reading);
        return;
    }
    state.session.photos_of = Some(Arc::clone(reading));
    state.step = Step::Photos;
    say(
        state,
        fill(
            ctx.lang,
            Msg::HermesChatAskPhotos,
            &[
                ("folder", &folder_name(ctx, &reading.root)),
                ("count", &number(reading.photos)),
            ],
        ),
        vec![
            chip(ctx, Msg::HermesChatPhotosYes, ChipReply::PhotosYes, true),
            chip(ctx, Msg::HermesChatPhotosNo, ChipReply::PhotosNo, false),
        ],
    );
}

pub fn yes(state: &mut State, ctx: &Ctx) -> Option<Action> {
    let reading = state.session.photos_of.clone()?;
    if ctx.ocr_models_present {
        state.session.photos_of = None;
        state.session.readings.remove(&reading.root);
        state.step = Step::Idle;
        learn::read_for(state, ctx, reading.root.clone(), ReadFor::Source);
        return None;
    }
    say(
        state,
        text(ctx.lang, Msg::HermesChatPhotosModels),
        vec![
            chip(
                ctx,
                Msg::HermesChatPhotosModelsThere,
                ChipReply::PhotosYes,
                true,
            ),
            chip(ctx, Msg::HermesChatPhotosNo, ChipReply::PhotosNo, false),
        ],
    );
    Some(Action::OpenOcrModels)
}

pub fn no(state: &mut State, ctx: &Ctx) {
    let Some(reading) = state.session.photos_of.take() else {
        return;
    };
    say(
        state,
        fill(
            ctx.lang,
            Msg::HermesChatPhotosSkipped,
            &[("count", &number(reading.photos))],
        ),
        Vec::new(),
    );
    tidy::choose_where(state, ctx, &reading);
}
