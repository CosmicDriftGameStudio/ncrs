//! Folders the model named in a sentence: each name is mapped to every
//! folder it can mean, checked with the filing setup's check off the UI
//! thread, and the user confirms the full paths before anything is read.

use std::path::{Component, Path, PathBuf};

use iced::Task;
use ncrs_ai::Schedule;
use ncrs_filing::chat_work::same_word;
use ncrs_i18n::Msg;
use ncrs_index::Transfer;

use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::{ChipReply, Proposal, State};
use crate::texts::{fill, folder_name};
use crate::update::{
    cancel_chip, chip_labelled, expand_home, looks_like_typed_path, say, well_known,
};

/// A name as the model said it and the folders it can mean: unchecked first,
/// then only those that exist.
#[derive(Debug, Clone)]
pub struct Named {
    pub said: String,
    pub folders: Vec<PathBuf>,
}

/// The folders of one sentence that are being checked or chosen from.
#[derive(Debug, Clone)]
pub struct Naming {
    pub source: Named,
    pub target: Option<Named>,
    pub schedule: Option<Schedule>,
    pub transfer: Option<Transfer>,
}

impl Naming {
    fn unsettled_mut(&mut self) -> Option<&mut Named> {
        std::iter::once(&mut self.source)
            .chain(self.target.as_mut())
            .find(|named| named.folders.len() != 1)
    }

    /// The check of every candidate, as the message the app sends back.
    pub fn resolve(&self, resolve: fn(&Path) -> Result<PathBuf, String>) -> Message {
        Message::NamedFoldersResolved {
            source: existing(resolve, &self.source.folders),
            target: self
                .target
                .as_ref()
                .map(|named| existing(resolve, &named.folders)),
        }
    }
}

fn existing(resolve: fn(&Path) -> Result<PathBuf, String>, candidates: &[PathBuf]) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for candidate in candidates {
        if let Ok(real) = resolve(candidate) {
            if !found.contains(&real) {
                found.push(real);
            }
        }
    }
    found
}

/// Configured, favourite, learned or well-known folders the word names.
fn known_folders(state: &State, ctx: &Ctx, word: &str) -> Vec<PathBuf> {
    let usual = well_known(ctx);
    ctx.targets
        .iter()
        .chain(&ctx.sources)
        .chain(&ctx.favourites)
        .chain(&state.session.learned)
        .chain(&usual[1..])
        .filter(|path| {
            same_word(word, &folder_name(ctx, path))
                || path
                    .file_name()
                    .is_some_and(|last| same_word(word, &last.to_string_lossy()))
        })
        .cloned()
        .collect()
}

/// Every folder a name can stand for: a typed path, the known folders, a path
/// below a known folder, and a folder in the home folder. `None` for names
/// that climb out with `..`.
fn candidates(state: &State, ctx: &Ctx, name: &str) -> Option<Vec<PathBuf>> {
    let path = Path::new(name);
    if path.components().any(|part| part == Component::ParentDir) {
        return None;
    }
    if looks_like_typed_path(name) {
        return Some(vec![expand_home(ctx, path)]);
    }
    let mut found = known_folders(state, ctx, name);
    if let Some((first, rest)) = name.split_once('/') {
        found.extend(
            known_folders(state, ctx, first)
                .into_iter()
                .map(|known| known.join(rest)),
        );
    }
    found.push(ctx.home.join(name));
    let mut unique: Vec<PathBuf> = Vec::new();
    for folder in found {
        if !unique.contains(&folder) {
            unique.push(folder);
        }
    }
    Some(unique)
}

fn named(state: &State, ctx: &Ctx, said: &str) -> Option<Named> {
    candidates(state, ctx, said).map(|folders| Named {
        said: said.to_owned(),
        folders,
    })
}

/// Starts the check of the folders a sentence named. `false` when a name
/// climbs out of its folder, which the caller answers with the fallback.
pub fn start(
    state: &mut State,
    ctx: &Ctx,
    source_said: &str,
    target_said: Option<&str>,
    schedule: Option<Schedule>,
    transfer: Option<Transfer>,
) -> bool {
    let Some(source) = named(state, ctx, source_said) else {
        return false;
    };
    let target = match target_said.map(|name| named(state, ctx, name)) {
        Some(None) => return false,
        Some(Some(found)) => Some(found),
        None => None,
    };
    state.session.proposal = None;
    state.session.choosing = None;
    state.naming = Some(Naming {
        source,
        target,
        schedule,
        transfer,
    });
    state.fresh_naming = true;
    true
}

/// The task that checks the candidates on a blocking thread.
pub fn launch(state: &mut State, ctx: &Ctx) -> Task<Message> {
    if !std::mem::take(&mut state.fresh_naming) {
        return Task::none();
    }
    let Some(naming) = state.naming.clone() else {
        return Task::none();
    };
    let resolve = ctx.resolve_folder;
    Task::perform(
        async move { tokio::task::spawn_blocking(move || naming.resolve(resolve)).await },
        |joined| {
            joined.unwrap_or(Message::NamedFoldersResolved {
                source: Vec::new(),
                target: None,
            })
        },
    )
}

pub fn resolved(state: &mut State, ctx: &Ctx, source: Vec<PathBuf>, target: Option<Vec<PathBuf>>) {
    let Some(mut naming) = state.naming.take() else {
        return;
    };
    naming.source.folders = source;
    if let (Some(named), Some(found)) = (naming.target.as_mut(), target) {
        named.folders = found;
    }
    settle(state, ctx, naming);
}

/// The user chose one of the folders a name can mean.
pub fn chosen(state: &mut State, ctx: &Ctx, folder: PathBuf) {
    let Some(mut naming) = state.session.choosing.take() else {
        return;
    };
    if let Some(named) = naming.unsettled_mut() {
        if named.folders.contains(&folder) {
            named.folders = vec![folder];
        }
    }
    settle(state, ctx, naming);
}

/// Asks, or says nothing was found, for the first name without exactly one
/// folder; with one folder for each, proposes the tidy-up for confirmation.
fn settle(state: &mut State, ctx: &Ctx, mut naming: Naming) {
    if let Some(named) = naming.unsettled_mut() {
        if named.folders.is_empty() {
            let sentence = fill(
                ctx.lang,
                Msg::HermesChatNotAFolder,
                &[("path", &named.said)],
            );
            say(state, sentence, Vec::new());
            return;
        }
        let sentence = fill(
            ctx.lang,
            Msg::HermesChatWhichFolder,
            &[("name", &named.said)],
        );
        let mut chips: Vec<_> = named
            .folders
            .iter()
            .map(|folder| {
                chip_labelled(
                    folder.display().to_string(),
                    ChipReply::NamedFolder(folder.clone()),
                    false,
                )
            })
            .collect();
        chips.push(cancel_chip(ctx));
        say(state, sentence, chips);
        state.session.choosing = Some(naming);
        return;
    }
    let (Some(source), chosen_target) =
        (single(&naming.source), naming.target.as_ref().map(single))
    else {
        return;
    };
    let target_folder = match chosen_target {
        Some(Some(folder)) => Some(folder),
        Some(None) => return,
        None => None,
    };
    state.session.proposal = Some(Proposal {
        source,
        target: target_folder,
        schedule: naming.schedule,
        transfer: naming.transfer,
    });
    crate::understand::confirm_tidy(state, ctx);
}

fn single(named: &Named) -> Option<PathBuf> {
    match named.folders.as_slice() {
        [only] => Some(only.clone()),
        _ => None,
    }
}
