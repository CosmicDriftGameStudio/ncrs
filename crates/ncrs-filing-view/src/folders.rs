//! Reading the folders of the targets for the tree, off the UI thread.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use iced::Task;
use ncrs_index::TargetId;

use crate::message::Message;
use crate::state::Session;
use crate::tree::folders_with_new;

/// How deep the folders of a target are listed for the full tree.
const FOLDER_DEPTH: usize = 3;

/// The folders of `root`, relative to it, down to `FOLDER_DEPTH` levels.
fn list_folders(root: &Path) -> BTreeSet<PathBuf> {
    fn walk(root: &Path, relative: &Path, found: &mut BTreeSet<PathBuf>) {
        if relative.components().count() >= FOLDER_DEPTH {
            return;
        }
        let Ok(entries) = std::fs::read_dir(root.join(relative)) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let is_folder = entry.file_type().is_ok_and(|kind| kind.is_dir());
            if !is_folder || name.to_string_lossy().starts_with('.') {
                continue;
            }
            let child = relative.join(name);
            walk(root, &child, found);
            found.insert(child);
        }
    }
    let mut found = BTreeSet::new();
    walk(root, Path::new(""), &mut found);
    found
}

/// The names of the files in `folder`, sorted, without the hidden ones.
fn names_in(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| !kind.is_dir()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    names
}

fn roots(session: &Session) -> Vec<(TargetId, PathBuf)> {
    session
        .source_targets()
        .map(|(_, target)| (target.id.clone(), target.path.clone()))
        .collect()
}

/// Lists the folders of the targets.
pub fn list_target_folders(session: &mut Session) -> Task<Message> {
    if session.listing {
        return Task::none();
    }
    session.listing = true;
    let id = session.id;
    let roots = roots(session);
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                roots
                    .into_iter()
                    .flat_map(|(target, root)| {
                        list_folders(&root)
                            .into_iter()
                            .map(move |folder| (target.clone(), folder))
                    })
                    .collect()
            })
            .await
            .unwrap_or_default()
        },
        move |folders| Message::FoldersListed(id, folders),
    )
}

/// Reads the files of every shown folder of the targets, once per folder.
pub fn read_existing(session: &mut Session) -> Task<Message> {
    let mut wanted = folders_with_new(session);
    if !session.only_new {
        wanted.extend(session.target_folders.iter().cloned());
    }
    let roots = roots(session);
    let missing: Vec<((TargetId, PathBuf), PathBuf)> = wanted
        .into_iter()
        .filter(|key| !session.existing.contains_key(key) && !session.reading_folders.contains(key))
        .filter_map(|key| {
            let (_, root) = roots.iter().find(|(id, _)| *id == key.0)?;
            let folder = root.join(&key.1);
            Some((key, folder))
        })
        .collect();
    if missing.is_empty() {
        return Task::none();
    }
    session
        .reading_folders
        .extend(missing.iter().map(|(key, _)| key.clone()));
    let id = session.id;
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                missing
                    .into_iter()
                    .map(|(key, folder)| (key, names_in(&folder)))
                    .collect::<Vec<_>>()
            })
            .await
            .unwrap_or_default()
        },
        move |read| Message::ExistingRead(id, read),
    )
}
