//! The chips and the small speaking helpers of the Hermes chat: what Hermes says, which folders it offers.

use std::path::{Path, PathBuf};

use ncrs_filing::chat_work::ChatWork;
use ncrs_i18n::Msg;

use crate::ctx::Ctx;
use crate::state::{Chip, ChipReply, Entry, FolderMention, Part, State};
use crate::texts::{folder_name, shown_folder, text};

pub fn chip(ctx: &Ctx, msg: Msg, reply: ChipReply, primary: bool) -> Chip {
    chip_labelled(text(ctx.lang, msg), reply, primary)
}

pub fn chip_labelled(label: String, reply: ChipReply, primary: bool) -> Chip {
    Chip {
        label,
        reply,
        primary,
    }
}

pub fn cancel_chip(ctx: &Ctx) -> Chip {
    chip(ctx, Msg::HermesChatCancel, ChipReply::Cancel, false)
}

/// A bubble of the user clears the chips Hermes offered before it.
pub fn user(state: &mut State, typed: String) {
    clear_chips(state);
    state.thread.push(Entry::User(typed));
}

pub fn clear_chips(state: &mut State) {
    for entry in &mut state.thread {
        if let Entry::Hermes { chips, .. } = entry {
            chips.clear();
        }
    }
}

pub fn mention_of(ctx: &Ctx, path: &Path) -> FolderMention {
    FolderMention {
        name: folder_name(ctx, path),
        path: shown_folder(ctx, path),
    }
}

/// A folder the user picked or typed: its name, and where it lies.
pub fn user_folder(state: &mut State, ctx: &Ctx, path: &Path) {
    clear_chips(state);
    state.thread.push(Entry::UserFolder(mention_of(ctx, path)));
}

pub fn hermes(state: &mut State, parts: Vec<Part>, chips: Vec<Chip>) {
    state.thread.push(Entry::Hermes { parts, chips });
}

pub fn say(state: &mut State, sentence: String, chips: Vec<Chip>) {
    hermes(state, vec![Part::Text(sentence)], chips);
}

/// Sets work for the app to run. `working` is the line shown meanwhile;
/// `None` for work too quick to show one.
pub fn work(state: &mut State, work: ChatWork, working: Option<String>) {
    if let Some(line) = working {
        state.thread.push(Entry::Working(line));
    }
    state.pending = Some(work);
    state.fresh = true;
}

// Folders.

pub fn well_known(ctx: &Ctx) -> [PathBuf; 4] {
    [
        ctx.home.clone(),
        ctx.home.join("Documents"),
        ctx.home.join("Downloads"),
        ctx.home.join("Desktop"),
    ]
}

pub fn is_target_folder(state: &State, ctx: &Ctx, path: &Path) -> bool {
    ctx.targets
        .iter()
        .chain(&state.session.learned)
        .any(|target| path.starts_with(target))
}

pub fn is_known(state: &State, ctx: &Ctx, path: &Path) -> bool {
    ctx.targets
        .iter()
        .chain(&ctx.sources)
        .chain(&state.session.learned)
        .chain(&well_known(ctx))
        .any(|known| known == path)
}

/// Targets and sources first, then Documents and Downloads when they exist.
pub fn candidate_folders(ctx: &Ctx, with_targets: bool) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    let own = if with_targets {
        ctx.targets.iter().chain(&ctx.sources)
    } else {
        [].iter().chain(&ctx.sources)
    };
    let usual = [ctx.home.join("Documents"), ctx.home.join("Downloads")]
        .into_iter()
        .filter(|path| path.is_dir());
    for path in own.cloned().chain(usual) {
        if !found.contains(&path) {
            found.push(path);
        }
    }
    found
}

pub fn folder_chips(ctx: &Ctx, folders: Vec<PathBuf>) -> Vec<Chip> {
    folders
        .into_iter()
        .map(|path| chip_labelled(folder_name(ctx, &path), ChipReply::Folder(path), false))
        .collect()
}

/// Where structure is read: the filing locations only, and the picker.
pub fn target_chips(state: &State, ctx: &Ctx) -> Vec<Chip> {
    let mut targets: Vec<PathBuf> = ctx.targets.clone();
    for learned in &state.session.learned {
        if !targets.contains(learned) {
            targets.push(learned.clone());
        }
    }
    if targets.is_empty() {
        targets.extend(Some(ctx.home.join("Documents")).filter(|path| path.is_dir()));
    }
    let mut chips = folder_chips(ctx, targets);
    chips.push(chip(
        ctx,
        Msg::HermesChatOtherFolder,
        ChipReply::PickFolder,
        false,
    ));
    chips
}

/// The folders to offer when a folder is asked for, and the picker.
pub fn ask_chips(ctx: &Ctx) -> Vec<Chip> {
    let mut chips = folder_chips(ctx, candidate_folders(ctx, true));
    chips.push(chip(
        ctx,
        Msg::HermesChatOtherFolder,
        ChipReply::PickFolder,
        false,
    ));
    chips
}

pub fn source_chips(ctx: &Ctx) -> Vec<Chip> {
    let mut chips = folder_chips(ctx, candidate_folders(ctx, false));
    chips.push(chip(ctx, Msg::HermesChatLater, ChipReply::Later, false));
    chips
}
