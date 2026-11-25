//! What the actions show of the repository, read in one go off the UI thread.

use std::path::Path;

use ncrs_git::{
    ActionError, ChangeLists, ConflictKind, FileDiff, Git, HunkSide, Integration,
    IntegrationDetail, PreviewLine, StashEntry,
};

use crate::actions::{ChangeSide, ConflictLine, LocalChanges, WipFocus};
use crate::state::State;

/// What the user has chosen, so a new read shows the same things.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Want {
    pub change: Option<(ChangeSide, String)>,
    pub conflict: Option<String>,
    pub ignore_whitespace: bool,
}

impl Want {
    pub fn of(state: &State) -> Self {
        let change = state.actions.local.as_ref().and_then(|local| {
            let (side, index) = local.selected?;
            let files = match side {
                ChangeSide::Unstaged => &local.unstaged,
                ChangeSide::Staged => &local.staged,
            };
            Some((side, files.get(index)?.path.clone()))
        });
        let conflict = state.actions.merge.as_ref().and_then(|merge| {
            merge
                .conflicts
                .get(merge.selected)
                .map(|conflict| conflict.path.clone())
        });
        let ignore_whitespace = state
            .actions
            .local
            .as_ref()
            .is_some_and(|local| local.ignore_whitespace);
        Self {
            change,
            conflict,
            ignore_whitespace,
        }
    }
}

/// A file still in conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenConflict {
    pub path: String,
    pub places: u32,
    /// No markers left in a text file, but git still has it in conflict.
    pub edited: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IntegrationSnapshot {
    pub detail: IntegrationDetail,
    pub conflicts: Vec<OpenConflict>,
    /// The file shown in the middle, as git's sides.
    pub preview: Option<(String, Vec<PreviewLine>)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub changes: ChangeLists,
    pub selected: Option<(ChangeSide, usize)>,
    pub diff: Option<FileDiff>,
    pub stashes: Vec<StashEntry>,
    pub integration: Option<IntegrationSnapshot>,
}

/// The wanted file where it is now: on its side, or on the other one after it
/// was staged; else the first file there is.
pub fn resolve_selection(
    changes: &ChangeLists,
    want: Option<&(ChangeSide, String)>,
) -> Option<(ChangeSide, usize)> {
    let position = |side: ChangeSide, path: &str| {
        let files = match side {
            ChangeSide::Unstaged => &changes.unstaged,
            ChangeSide::Staged => &changes.staged,
        };
        files
            .iter()
            .position(|file| file.path == path)
            .map(|index| (side, index))
    };
    let other = |side: ChangeSide| match side {
        ChangeSide::Unstaged => ChangeSide::Staged,
        ChangeSide::Staged => ChangeSide::Unstaged,
    };
    want.and_then(|(side, path)| position(*side, path).or_else(|| position(other(*side), path)))
        .or_else(|| (!changes.unstaged.is_empty()).then_some((ChangeSide::Unstaged, 0)))
        .or_else(|| (!changes.staged.is_empty()).then_some((ChangeSide::Staged, 0)))
}

fn hunk_side(side: ChangeSide) -> HunkSide {
    match side {
        ChangeSide::Unstaged => HunkSide::Unstaged,
        ChangeSide::Staged => HunkSide::Staged,
    }
}

fn integration_of(
    git: &Git,
    detail: IntegrationDetail,
    want: &Want,
) -> Result<IntegrationSnapshot, ActionError> {
    let files = git.conflicts()?;
    let conflicts: Vec<OpenConflict> = files
        .iter()
        .map(|file| OpenConflict {
            path: file.path.clone(),
            places: u32::try_from(file.blocks.len()).unwrap_or(u32::MAX),
            edited: file.blocks.is_empty()
                && matches!(
                    file.kind,
                    ConflictKind::BothModified | ConflictKind::BothAdded
                )
                && ncrs_git::is_text_without_markers(git.root(), &file.path),
        })
        .collect();
    let shown = want
        .conflict
        .as_ref()
        .and_then(|path| conflicts.iter().find(|conflict| &conflict.path == path))
        .or_else(|| conflicts.first());
    let preview = shown.map(|conflict| {
        let text = ncrs_git::read_working_text(git.root(), &conflict.path).unwrap_or_default();
        (conflict.path.clone(), ncrs_git::preview_lines(&text))
    });
    Ok(IntegrationSnapshot {
        detail,
        conflicts,
        preview,
    })
}

/// Reads everything the actions show. Fails with `GitNotFound` or
/// `DisabledInSandbox` where there are no actions.
pub fn read(root: &Path, want: &Want) -> Result<Snapshot, ActionError> {
    let git = Git::open_reading(root)?;
    let changes = git.changes()?;
    let selected = resolve_selection(&changes, want.change.as_ref());
    let diff = selected.and_then(|(side, index)| {
        let files = match side {
            ChangeSide::Unstaged => &changes.unstaged,
            ChangeSide::Staged => &changes.staged,
        };
        git.hunks_shown(
            &files.get(index)?.path,
            hunk_side(side),
            want.ignore_whitespace,
        )
        .ok()
    });
    let stashes = git.stashes()?;
    let integration = git
        .integration_detail()
        .map(|detail| integration_of(&git, detail, want))
        .transpose()?;
    Ok(Snapshot {
        changes,
        selected,
        diff,
        stashes,
        integration,
    })
}

/// The lines of a file with markers as the view draws them: "mine" first. In a
/// rebase git's "ours" is the branch rebased onto, so the sides swap.
pub fn view_lines(lines: Vec<PreviewLine>, rebasing: bool) -> Vec<ConflictLine> {
    if !rebasing {
        return lines
            .into_iter()
            .map(|line| match line {
                PreviewLine::Shared(number, text) => ConflictLine::Shared(number, text),
                PreviewLine::Ours(text) => ConflictLine::Mine(text),
                PreviewLine::Theirs(text) => ConflictLine::Theirs(text),
            })
            .collect();
    }
    let mut shown = Vec::new();
    let mut ours = Vec::new();
    let mut theirs = Vec::new();
    let flush = |out: &mut Vec<ConflictLine>, left: &mut Vec<String>, right: &mut Vec<String>| {
        out.extend(right.drain(..).map(ConflictLine::Mine));
        out.extend(left.drain(..).map(ConflictLine::Theirs));
    };
    for line in lines {
        match line {
            PreviewLine::Shared(number, text) => {
                flush(&mut shown, &mut ours, &mut theirs);
                shown.push(ConflictLine::Shared(number, text));
            }
            PreviewLine::Ours(text) => {
                if !theirs.is_empty() {
                    flush(&mut shown, &mut ours, &mut theirs);
                }
                ours.push(text);
            }
            PreviewLine::Theirs(text) => theirs.push(text),
        }
    }
    flush(&mut shown, &mut ours, &mut theirs);
    shown
}

/// Whether the integration in the snapshot is a rebase.
pub fn is_rebase(detail: &IntegrationDetail) -> bool {
    detail.kind == Integration::Rebase
}

/// The stash list as the left list shows it: the message and the branch.
pub fn stash_row(entry: &StashEntry) -> crate::actions::StashEntry {
    let text = entry.message.as_str();
    let named = text
        .strip_prefix("WIP on ")
        .or_else(|| text.strip_prefix("On "))
        .and_then(|rest| rest.split_once(": "));
    match named {
        Some((branch, message)) => crate::actions::StashEntry {
            message: message.to_owned(),
            origin: branch.to_owned(),
        },
        None => crate::actions::StashEntry {
            message: text.to_owned(),
            origin: String::new(),
        },
    }
}

impl LocalChanges {
    /// Takes the lists of a new read; the message the user is typing stays.
    pub fn take(&mut self, snapshot: &Snapshot) {
        if self.selected != snapshot.selected {
            self.hunk = 0;
        }
        self.unstaged.clone_from(&snapshot.changes.unstaged);
        self.staged.clone_from(&snapshot.changes.staged);
        self.selected = snapshot.selected;
        if let Some((side, _)) = self.selected {
            if matches!(self.focus, WipFocus::Unstaged | WipFocus::Staged) {
                self.focus = crate::wip::focus_of(side);
            }
        }
        if self.selected.is_none() {
            self.diff_open = false;
        }
        self.diff.clone_from(&snapshot.diff);
    }
}
