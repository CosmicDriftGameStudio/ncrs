//! The opening question: where the documents are. Folders where new files
//! arrive go to the tidy-up, folders where documents are kept are read for
//! their order; a folder with no role yet is asked about once.

use std::path::PathBuf;

use ncrs_i18n::Msg;
use ncrs_ui_kit::hermes_state::FolderRole;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::state::{Chip, ChipReply, State, Step};
use crate::texts::{fill, folder_name};
use crate::update::{cancel_chip, candidate_folders, chip, chip_labelled, is_target_folder, say};
use crate::{learn, tidy};

/// The folders, each labelled with what it is for when that is known.
pub fn chips(state: &State, ctx: &Ctx) -> Vec<Chip> {
    let mut chips: Vec<Chip> = candidate_folders(ctx, true)
        .into_iter()
        .map(|path| {
            let shown = folder_name(ctx, &path);
            let label = if ctx.sources.contains(&path) {
                fill(
                    ctx.lang,
                    Msg::HermesChatGreetingSourceChip,
                    &[("folder", &shown)],
                )
            } else if is_target_folder(state, ctx, &path) {
                fill(
                    ctx.lang,
                    Msg::HermesChatGreetingTargetChip,
                    &[("folder", &shown)],
                )
            } else {
                shown
            };
            chip_labelled(label, ChipReply::Folder(path), false)
        })
        .collect();
    chips.push(chip(
        ctx,
        Msg::HermesChatOtherFolder,
        ChipReply::PickFolder,
        false,
    ));
    chips
}

pub fn route(state: &mut State, ctx: &Ctx, path: PathBuf) {
    if ctx.sources.contains(&path) {
        tidy::begin(state, ctx, path);
    } else if is_target_folder(state, ctx, &path) {
        if !crate::rule_flow::open(state, ctx, path.clone()) {
            learn::read(state, ctx, path);
        }
    } else {
        state.step = Step::Role;
        let shown = folder_name(ctx, &path);
        state.session.role_asked = Some(path);
        say(
            state,
            fill(ctx.lang, Msg::HermesChatAskRole, &[("folder", &shown)]),
            vec![
                chip(ctx, Msg::HermesChatRoleSource, ChipReply::RoleSource, false),
                chip(ctx, Msg::HermesChatRoleTarget, ChipReply::RoleTarget, false),
                cancel_chip(ctx),
            ],
        );
    }
}

pub fn role_chosen(state: &mut State, ctx: &Ctx, source: bool) -> Option<Action> {
    let path = state.session.role_asked.take()?;
    if source {
        let role = Action::SetFolderRole {
            folder: path.clone(),
            role: FolderRole::Inbox,
        };
        tidy::begin(state, ctx, path);
        Some(role)
    } else {
        state.step = Step::Greeting;
        if !crate::rule_flow::open(state, ctx, path.clone()) {
            learn::read(state, ctx, path);
        }
        None
    }
}
