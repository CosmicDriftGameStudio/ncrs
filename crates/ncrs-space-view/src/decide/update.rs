use std::collections::HashSet;
use std::path::{Path, PathBuf};

use iced::Task;
use ncrs_space::decide::DownloadGroups;
use ncrs_space::{InstalledApp, VersionRelation};
use ncrs_ui_kit::row_scroll::{row_into_view, FOCUSED_ROW_ID};

use super::{
    DecideArea, DecideGroup, DecideGroupKind, DecideItem, DecideKey, DecideMessage, DecideNotice,
    DecideReason, DecideState, DECIDE_LIST_ID,
};
use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::Message;
use crate::tasks;
use crate::update::Outcome;

/// How far Page Up and Page Down go.
const PAGE_ROWS: isize = 10;

/// Opens the pane for a folder of the overview and reads it off the UI thread.
pub(crate) fn open(folder: PathBuf, is_downloads: bool, ctx: &Ctx) -> (DecideState, Task<Message>) {
    let task = tasks::load_decide(folder.clone(), is_downloads, ctx.home.clone());
    (DecideState::new(folder, is_downloads), task)
}

/// Reads the folder again, as the Rescan of the tab does while the pane is open.
pub(crate) fn reload(state: &DecideState, ctx: &Ctx) -> Task<Message> {
    tasks::load_decide(state.folder.clone(), state.is_downloads, ctx.home.clone())
}

fn quiet(task: Task<Message>) -> Outcome {
    (task, Vec::new())
}

fn scroll_to_cursor() -> Task<Message> {
    row_into_view(DECIDE_LIST_ID, FOCUSED_ROW_ID)
}

pub(crate) fn update(state: &mut DecideState, message: DecideMessage, ctx: &Ctx) -> Outcome {
    match message {
        DecideMessage::Loaded { folder, groups } => {
            if folder == state.folder {
                fill(state, *groups);
            }
            quiet(Task::none())
        }
        DecideMessage::Up
        | DecideMessage::Down
        | DecideMessage::PageUp
        | DecideMessage::PageDown
        | DecideMessage::First
        | DecideMessage::Last
        | DecideMessage::Toggle
        | DecideMessage::Enter
            if state.area != DecideArea::List =>
        {
            quiet(Task::none())
        }
        DecideMessage::Up => move_cursor(state, -1),
        DecideMessage::Down => move_cursor(state, 1),
        DecideMessage::PageUp => move_cursor(state, -PAGE_ROWS),
        DecideMessage::PageDown => move_cursor(state, PAGE_ROWS),
        DecideMessage::First => jump(state, false),
        DecideMessage::Last => jump(state, true),
        DecideMessage::Toggle => {
            toggle_at_cursor(state);
            quiet(Task::none())
        }
        DecideMessage::Enter => {
            match state.cursor.clone() {
                Some(DecideKey::Head(kind)) => fold(state, kind),
                Some(DecideKey::Item(_)) => toggle_at_cursor(state),
                Some(DecideKey::More(kind)) => open_more(state, kind),
                None => {}
            }
            quiet(Task::none())
        }
        DecideMessage::NextArea => {
            state.area = state.area.next();
            quiet(Task::none())
        }
        DecideMessage::PreviousArea => {
            state.area = state.area.previous();
            quiet(Task::none())
        }
        DecideMessage::RowPressed(path) => {
            state.area = DecideArea::List;
            state.cursor = Some(DecideKey::Item(path));
            quiet(Task::none())
        }
        DecideMessage::RowToggled(path) => {
            state.area = DecideArea::List;
            state.cursor = Some(DecideKey::Item(path.clone()));
            toggle_item(state, &path);
            quiet(Task::none())
        }
        DecideMessage::HeadToggled(kind) => {
            state.area = DecideArea::List;
            state.cursor = Some(DecideKey::Head(kind));
            fold(state, kind);
            quiet(Task::none())
        }
        DecideMessage::GroupToggled(kind) => {
            state.area = DecideArea::List;
            state.cursor = Some(DecideKey::Head(kind));
            toggle_group(state, kind);
            quiet(Task::none())
        }
        DecideMessage::MoreOpened(kind) => {
            state.area = DecideArea::List;
            open_more(state, kind);
            quiet(Task::none())
        }
        DecideMessage::SidePressed(_) => quiet(Task::none()),
        DecideMessage::Trash => {
            let items = state.chosen_items();
            if items.is_empty() {
                return quiet(Task::none());
            }
            state.emptying = state.empty_after && ctx.trash_can_purge;
            let action = Action::TrashOwn {
                root: state.folder.clone(),
                items,
                empty_after: state.emptying,
            };
            (Task::none(), vec![action])
        }
        DecideMessage::Undo => {
            if state.can_undo {
                (Task::none(), vec![Action::UndoTrash])
            } else {
                quiet(Task::none())
            }
        }
        DecideMessage::ToggleEmptyTrash => {
            if ctx.trash_can_purge {
                state.empty_after = !state.empty_after;
            }
            quiet(Task::none())
        }
        DecideMessage::Trashed(paths) => {
            let bytes = remove_items(state, &paths);
            state.notice = Some(DecideNotice::Trashed {
                count: paths.len(),
                bytes,
                emptied: state.emptying,
            });
            state.can_undo = true;
            quiet(Task::none())
        }
        DecideMessage::Undone(count) => {
            state.notice = Some(DecideNotice::Undone { count });
            state.can_undo = false;
            quiet(tasks::load_decide(
                state.folder.clone(),
                state.is_downloads,
                ctx.home.clone(),
            ))
        }
    }
}

/// The items of a sorted folder as groups. The first time, what exists somewhere else is chosen
/// and the rest group is folded; a reload keeps what the user chose and chooses by the start
/// rule only what was not in the list before.
fn fill(state: &mut DecideState, sorted: DownloadGroups) {
    let mut groups = Vec::new();
    let mut add = |kind: DecideGroupKind, items: Vec<DecideItem>| {
        if !items.is_empty() {
            groups.push(DecideGroup { kind, items });
        }
    };
    add(
        DecideGroupKind::Installed,
        sorted
            .installed
            .into_iter()
            .map(|matched| {
                let app = matched.installed.unwrap_or(InstalledApp {
                    path: matched.with,
                    version: None,
                    installer_version: None,
                    relation: VersionRelation::Unknown,
                });
                DecideItem {
                    item: matched.item,
                    reason: DecideReason::Installed(app),
                }
            })
            .collect(),
    );
    add(
        DecideGroupKind::Unpacked,
        sorted
            .unpacked
            .into_iter()
            .map(|matched| DecideItem {
                item: matched.item,
                reason: DecideReason::Unpacked(matched.with),
            })
            .collect(),
    );
    let in_downloads = sorted.duplicates.into_iter().flat_map(|set| {
        let kept = set.keep.path;
        set.extra.into_iter().map(move |item| DecideItem {
            item,
            reason: DecideReason::Duplicate(kept.clone()),
        })
    });
    let elsewhere = sorted.elsewhere.into_iter().map(|matched| DecideItem {
        item: matched.item,
        reason: DecideReason::Duplicate(matched.with),
    });
    add(
        DecideGroupKind::Duplicates,
        in_downloads.chain(elsewhere).collect(),
    );
    add(
        DecideGroupKind::UntouchedYear,
        sorted
            .untouched_year
            .into_iter()
            .map(|item| DecideItem {
                item,
                reason: DecideReason::Untouched,
            })
            .collect(),
    );
    add(
        DecideGroupKind::Rest,
        sorted
            .rest
            .into_iter()
            .map(|item| DecideItem {
                item,
                reason: DecideReason::Rest,
            })
            .collect(),
    );
    let known: HashSet<PathBuf> = state
        .groups
        .iter()
        .flat_map(|group| group.items.iter().map(|entry| entry.item.path.clone()))
        .collect();
    let mut chosen = HashSet::new();
    for group in &groups {
        for entry in &group.items {
            let path = &entry.item.path;
            let keep = if known.contains(path) {
                state.chosen.contains(path)
            } else {
                group.kind.is_chosen_from_the_start()
            };
            if keep {
                chosen.insert(path.clone());
            }
        }
    }
    if !state.loaded {
        state.folded.insert(DecideGroupKind::Rest);
    }
    state.chosen = chosen;
    state.groups = groups;
    state.loaded = true;
    if state.cursor_index().is_none() {
        state.cursor = state.keys().into_iter().next();
    }
}

/// Shows the rest of a long group and puts the cursor on its first new row.
fn open_more(state: &mut DecideState, kind: DecideGroupKind) {
    let first_hidden = state
        .groups
        .iter()
        .find(|group| group.kind == kind)
        .and_then(|group| group.items.get(state.shown_len(group)))
        .map(|entry| entry.item.path.clone());
    state.expanded.insert(kind);
    if let Some(path) = first_hidden {
        state.cursor = Some(DecideKey::Item(path));
    }
}

fn move_cursor(state: &mut DecideState, step: isize) -> Outcome {
    let keys = state.keys();
    if keys.is_empty() {
        return quiet(Task::none());
    }
    let at = state.cursor_index().unwrap_or(0);
    let target = at.saturating_add_signed(step).min(keys.len() - 1);
    state.cursor = keys.get(target).cloned();
    quiet(scroll_to_cursor())
}

fn jump(state: &mut DecideState, to_end: bool) -> Outcome {
    let keys = state.keys();
    let target = if to_end { keys.last() } else { keys.first() };
    state.cursor = target.cloned();
    quiet(scroll_to_cursor())
}

fn toggle_item(state: &mut DecideState, path: &Path) {
    if !state.chosen.remove(path) {
        state.chosen.insert(path.to_path_buf());
    }
}

fn toggle_at_cursor(state: &mut DecideState) {
    match state.cursor.clone() {
        Some(DecideKey::Item(path)) => toggle_item(state, &path),
        Some(DecideKey::Head(kind)) => toggle_group(state, kind),
        Some(DecideKey::More(kind)) => open_more(state, kind),
        None => {}
    }
}

/// Chooses a whole group, or none of it when all of it was chosen.
fn toggle_group(state: &mut DecideState, kind: DecideGroupKind) {
    let paths: Vec<PathBuf> = state
        .groups
        .iter()
        .filter(|group| group.kind == kind)
        .flat_map(|group| group.items.iter().map(|entry| entry.item.path.clone()))
        .collect();
    let all_chosen = paths.iter().all(|path| state.chosen.contains(path));
    for path in paths {
        if all_chosen {
            state.chosen.remove(&path);
        } else {
            state.chosen.insert(path);
        }
    }
}

fn fold(state: &mut DecideState, kind: DecideGroupKind) {
    if !state.folded.remove(&kind) {
        state.folded.insert(kind);
    }
}

/// Drops the items the trash took; the bytes they had.
fn remove_items(state: &mut DecideState, paths: &[PathBuf]) -> u64 {
    let mut bytes: u64 = 0;
    for group in &mut state.groups {
        group.items.retain(|entry| {
            let gone = paths.contains(&entry.item.path);
            if gone {
                bytes = bytes.saturating_add(entry.item.size);
            }
            !gone
        });
    }
    state.groups.retain(|group| !group.items.is_empty());
    state.chosen.retain(|path| !paths.contains(path));
    if state.cursor_index().is_none() {
        state.cursor = state.keys().into_iter().next();
    }
    bytes
}
