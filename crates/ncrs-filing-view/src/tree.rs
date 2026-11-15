//! The tree of the write folder: the folders that get something, the new
//! files in them and a few of the files that are there already.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ncrs_filing::chat_work::{LivePlan, LiveState};

use crate::state::Session;

/// How many of the files that are there already a folder lists.
pub const EXISTING_SHOWN: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Folder {
        /// Relative to the write folder; empty for the folder itself.
        path: PathBuf,
        depth: usize,
        open: bool,
        new_files: usize,
    },
    NewFile {
        /// Relative to the source.
        from: PathBuf,
        depth: usize,
        new_name: String,
        /// The name it had, when it is renamed.
        old_name: Option<String>,
    },
    Existing {
        folder: PathBuf,
        depth: usize,
        name: String,
    },
    More {
        folder: PathBuf,
        depth: usize,
        count: usize,
    },
}

fn depth_of(path: &Path) -> usize {
    path.components().count()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The folders that are shown: those with new files and their parents, and
/// with `only_new` off the folders of the write folder too.
fn folders_shown(session: &Session, plan: &LivePlan) -> BTreeSet<PathBuf> {
    let mut folders = BTreeSet::new();
    let mut add = |folder: &Path| {
        for ancestor in folder.ancestors() {
            if ancestor.as_os_str().is_empty() {
                break;
            }
            folders.insert(ancestor.to_path_buf());
        }
    };
    for file in &plan.files {
        if let LiveState::Filed { folder, .. } = &file.state {
            add(folder);
        }
    }
    if !session.only_new {
        for folder in &session.target_folders {
            add(folder);
        }
    }
    let root_has_new = plan.files.iter().any(
        |file| matches!(&file.state, LiveState::Filed { folder, .. } if folder.as_os_str().is_empty()),
    );
    if root_has_new {
        folders.insert(PathBuf::new());
    }
    folders
}

pub fn tree_items(session: &Session, plan: &LivePlan) -> Vec<Item> {
    let mut items = Vec::new();
    for folder in folders_shown(session, plan) {
        let hidden = folder
            .ancestors()
            .skip(1)
            .any(|ancestor| session.closed_folders.contains(ancestor));
        if hidden {
            continue;
        }
        let depth = depth_of(&folder);
        let open = !session.closed_folders.contains(&folder);
        let new_files: Vec<_> = plan
            .files
            .iter()
            .filter_map(|file| match &file.state {
                LiveState::Filed {
                    folder: to, name, ..
                } if *to == folder => Some((file, name)),
                _ => None,
            })
            .collect();
        items.push(Item::Folder {
            path: folder.clone(),
            depth,
            open,
            new_files: new_files.len(),
        });
        if !open {
            continue;
        }
        for (file, name) in new_files {
            let old = file_name(&file.from);
            items.push(Item::NewFile {
                from: file.from.clone(),
                depth: depth + 1,
                new_name: name.clone(),
                old_name: (old != *name).then_some(old),
            });
        }
        let there = session.existing.get(&folder).map_or(&[][..], Vec::as_slice);
        for name in there.iter().take(EXISTING_SHOWN) {
            items.push(Item::Existing {
                folder: folder.clone(),
                depth: depth + 1,
                name: name.clone(),
            });
        }
        if there.len() > EXISTING_SHOWN {
            items.push(Item::More {
                folder: folder.clone(),
                depth: depth + 1,
                count: there.len() - EXISTING_SHOWN,
            });
        }
    }
    items
}
