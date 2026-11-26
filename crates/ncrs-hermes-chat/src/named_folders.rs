//! The folder the model named in a sentence: the name is mapped to every
//! folder it can mean, checked with the filing setup's check off the UI
//! thread; the user chooses when there are several.

use std::path::{Component, Path, PathBuf};

use iced::Task;
use ncrs_filing::chat_work::same_word;
use ncrs_i18n::Msg;

use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::{ChipReply, State};
use crate::texts::{fill, folder_name};
use crate::update::{
    cancel_chip, chip_labelled, expand_home, looks_like_typed_path, lost_folder_chips, say,
    sort_hint, well_known,
};

/// What the folder is wanted for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// Its order is looked at.
    Open,
    /// The sentence wants it filed, which the chat only points to.
    Sort,
}

/// A name as the model said it and the folders it can mean: unchecked first,
/// then only those that exist.
#[derive(Debug, Clone)]
pub struct Naming {
    pub said: String,
    pub folders: Vec<PathBuf>,
    /// The targets to search for folders of this name when no candidate
    /// exists; empty for a path.
    pub search_below: Vec<PathBuf>,
    pub purpose: Purpose,
}

impl Naming {
    /// The check of every candidate, as the message the app sends back.
    pub fn resolve(&self, resolve: fn(&Path) -> Result<PathBuf, String>) -> Message {
        Message::NamedFoldersResolved {
            folders: existing(resolve, self),
        }
    }
}

/// How deep below a target a folder is looked for by its name alone.
const SEARCH_DEPTH: usize = 4;
/// Folders looked at in one search, so a huge archive cannot hold the check.
const SEARCH_LIMIT: usize = 20_000;

fn existing(resolve: fn(&Path) -> Result<PathBuf, String>, naming: &Naming) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for candidate in &naming.folders {
        if let Ok(real) = resolve(candidate) {
            if !found.contains(&real) {
                found.push(real);
            }
        }
    }
    if found.is_empty() {
        for target in &naming.search_below {
            for folder in folders_named_below(target, &naming.said) {
                if let Ok(real) = resolve(&folder) {
                    if !found.contains(&real) {
                        found.push(real);
                    }
                }
            }
        }
    }
    found
}

/// The folders below `root`, at most `SEARCH_DEPTH` levels down, whose name
/// is `word`; breadth first, so the shallow ones come first.
fn folders_named_below(root: &Path, word: &str) -> Vec<PathBuf> {
    let mut hits = Vec::new();
    let mut level = vec![root.to_path_buf()];
    let mut looked_at = 0;
    for _ in 0..SEARCH_DEPTH {
        let mut deeper = Vec::new();
        for folder in &level {
            let Ok(entries) = std::fs::read_dir(folder) else {
                continue;
            };
            let mut subfolders: Vec<PathBuf> = entries
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .is_some_and(|name| !name.to_string_lossy().starts_with('.'))
                })
                .collect();
            subfolders.sort();
            for path in subfolders {
                looked_at += 1;
                if looked_at > SEARCH_LIMIT {
                    return hits;
                }
                let named_so = path
                    .file_name()
                    .is_some_and(|name| same_word(word, &name.to_string_lossy()));
                if named_so {
                    hits.push(path.clone());
                }
                deeper.push(path);
            }
        }
        level = deeper;
    }
    hits
}

/// Configured, favourite or well-known folders the word names.
fn known_folders(ctx: &Ctx, word: &str) -> Vec<PathBuf> {
    let usual = well_known(ctx);
    ctx.targets
        .iter()
        .chain(&ctx.sources)
        .chain(&ctx.favourites)
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
fn candidates(ctx: &Ctx, name: &str) -> Option<Vec<PathBuf>> {
    let path = Path::new(name);
    if path.components().any(|part| part == Component::ParentDir) {
        return None;
    }
    if looks_like_typed_path(name) {
        // "/Bank" also names the area below a target, as the chat shows areas.
        let below_targets = name
            .strip_prefix('/')
            .filter(|rest| !rest.is_empty())
            .into_iter()
            .flat_map(|rest| ctx.targets.iter().map(move |target| target.join(rest)));
        let mut typed = vec![expand_home(ctx, path)];
        typed.extend(below_targets);
        return Some(typed);
    }
    let mut found = known_folders(ctx, name);
    if let Some((first, rest)) = name.split_once('/') {
        found.extend(
            known_folders(ctx, first)
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

/// Starts the check of the folder a sentence named. `false` when the name
/// climbs out of its folder, which the caller answers with the fallback.
pub fn start(state: &mut State, ctx: &Ctx, said: &str, purpose: Purpose) -> bool {
    let Some(folders) = candidates(ctx, said) else {
        return false;
    };
    let search_below = if looks_like_typed_path(said) || said.contains('/') {
        Vec::new()
    } else {
        ctx.targets.clone()
    };
    state.session.choosing = None;
    state.naming = Some(Naming {
        said: said.to_owned(),
        folders,
        search_below,
        purpose,
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
                folders: Vec::new(),
            })
        },
    )
}

pub fn resolved(state: &mut State, ctx: &Ctx, folders: Vec<PathBuf>) {
    let Some(mut naming) = state.naming.take() else {
        return;
    };
    naming.folders = folders;
    settle(state, ctx, naming);
}

/// The user chose one of the folders a name can mean.
pub fn chosen(state: &mut State, ctx: &Ctx, folder: PathBuf) {
    let Some(mut naming) = state.session.choosing.take() else {
        return;
    };
    if naming.folders.contains(&folder) {
        naming.folders = vec![folder];
    }
    settle(state, ctx, naming);
}

/// Asks which folder is meant, or goes on with the only one, or without any.
fn settle(state: &mut State, ctx: &Ctx, naming: Naming) {
    match (naming.folders.as_slice(), naming.purpose) {
        ([], Purpose::Open) => {
            let sentence = fill(
                ctx.lang,
                Msg::HermesChatNotAFolder,
                &[("name", &naming.said)],
            );
            say(state, sentence, lost_folder_chips(ctx));
        }
        ([], Purpose::Sort) => sort_hint(state, ctx, None),
        ([only], Purpose::Open) => crate::rule_flow::open(state, ctx, only.clone()),
        ([only], Purpose::Sort) => sort_hint(state, ctx, Some(only.clone())),
        (several, _) => {
            let sentence = fill(
                ctx.lang,
                Msg::HermesChatWhichFolder,
                &[("name", &naming.said)],
            );
            let mut chips: Vec<_> = several
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
        }
    }
}
