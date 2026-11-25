//! `update` of the Git tab: reads, cursors and the diff.

use std::path::PathBuf;

use iced::widget::operation;
use iced::widget::scrollable::AbsoluteOffset;
use iced::Task;
use ncrs_git::{ChangedFile, CommitDetail, DiffTarget, FileDiff, Log, RefsOverview};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::focus::release_focus;
use ncrs_ui_kit::page_scroll::scroll_page_by;
use ncrs_ui_kit::theme::size;

use crate::action::Action;
use crate::actions::{Dialog, MenuTarget, OpenMenu};
use crate::commit_menu;
use crate::ctx::Ctx;
use crate::diff_layout::rows_before;
use crate::file_views;
use crate::handlers;
use crate::load;
use crate::message::Message;
use crate::navigate;
use crate::picker;
use crate::state::{
    remotes_were_chosen, Area, ColumnDrag, Detail, DiffState, Fold, Section, SidebarItem, State,
    Subject, PAGE,
};
use crate::sync_handlers;
use crate::view::{DIFF_ID, FILTER_ID};
use crate::wip;

/// Lines an arrow key scrolls the diff by.
pub const SCROLL_LINES: f32 = 3.0;

pub type Outcome = (Task<Message>, Option<Action>);

pub fn quiet(task: Task<Message>) -> Outcome {
    (task, None)
}

/// Shows the tab for the repository at `root` and reads it. The same
/// repository again only reads it anew. A repository that is opened takes the
/// folded groups the app remembers for it.
pub fn open(state: &mut State, root: PathBuf, name: String, ctx: &Ctx) -> Task<Message> {
    if state.root.as_ref() != Some(&root) {
        state.close();
        state.root = Some(root);
        state.name = name;
        state.limit = PAGE;
        state.collapsed = ctx
            .folded
            .iter()
            .filter_map(|key| Fold::from_key(key))
            .collect();
        state.remotes_chosen = remotes_were_chosen(&ctx.folded);
        if !state.remotes_chosen {
            state.collapsed.insert(Fold::Section(Section::Remotes));
        }
    }
    state.problem = None;
    load::everything(state)
}

pub fn update(state: &mut State, message: Message, ctx: &Ctx) -> Outcome {
    let before = file_views::middle(state);
    let (task, action) = dispatch(state, message, ctx);
    if file_views::middle(state) == before {
        return (task, action);
    }
    (
        Task::batch([file_views::scroll_into_place(state), task]),
        action,
    )
}

fn dispatch(state: &mut State, message: Message, ctx: &Ctx) -> Outcome {
    match message {
        Message::RefsLoaded(generation, result) => refs_loaded(state, ctx, generation, *result),
        Message::LogLoaded(generation, result) => log_loaded(state, ctx, generation, *result),
        Message::DetailLoaded(generation, result) => detail_loaded(state, ctx, generation, *result),
        Message::WorkingLoaded(generation, result) => working_loaded(state, ctx, generation, result),
        Message::DiffLoaded(generation, result) => diff_loaded(state, ctx, generation, *result),
        Message::FilterChanged(filter) => {
            state.filter = filter;
            state.limit = PAGE;
            state.commit_cursor = 0;
            state.detail = None;
            quiet(load::log(state))
        }
        Message::FocusFilter => {
            state.filter_focused = true;
            quiet(operation::focus(FILTER_ID))
        }
        Message::FilterSubmit => {
            state.filter_focused = false;
            state.area = Area::Commits;
            quiet(release_focus())
        }
        Message::SidebarPressed(index) => {
            state.area = Area::Sidebar;
            state.sidebar_cursor = index;
            activate_sidebar(state, index)
        }
        Message::CommitPressed(row) => {
            state.area = Area::Commits;
            state.commit_cursor = row;
            close_wip_diff(state);
            quiet(ensure_detail(state))
        }
        Message::FolderPressed => {
            state.area = Area::Detail;
            quiet(Task::none())
        }
        Message::FilePressed(index) => file_pressed(state, index),
        Message::ToggleFold(fold) => (Task::none(), toggle_fold(state, fold)),
        Message::ResizePressed(column) => {
            state.column_drag = Some(ColumnDrag {
                column,
                start: ctx.columns,
                origin: None,
                current: ctx.columns,
            });
            quiet(Task::none())
        }
        Message::ResizeMoved(x) => {
            if let Some(drag) = state.column_drag.as_mut() {
                let origin = *drag.origin.get_or_insert(x);
                drag.current = drag.start.dragged(drag.column, x - origin);
            }
            quiet(Task::none())
        }
        Message::ResizeReleased => {
            let resized = state
                .column_drag
                .take()
                .filter(|drag| drag.current != drag.start);
            (
                Task::none(),
                resized.map(|drag| Action::ColumnsResized(drag.current)),
            )
        }
        Message::LoadMore => quiet(load_more(state)),
        Message::SetTreeMode(tree) => {
            state.tree_mode = tree;
            quiet(Task::none())
        }
        Message::SetUnified(unified) => {
            if let Some(shown) = state.diff.as_mut() {
                shown.unified = unified;
            }
            if let Some(local) = state.actions.local.as_mut() {
                local.unified = unified;
            }
            quiet(Task::none())
        }
        Message::ToggleIgnoreWhitespace if state.changes_shown() => {
            if let Some(local) = state.actions.local.as_mut() {
                local.ignore_whitespace = !local.ignore_whitespace;
            }
            quiet(load::actions(state))
        }
        Message::ToggleIgnoreWhitespace => {
            let Some(shown) = state.diff.as_mut() else {
                return quiet(Task::none());
            };
            shown.ignore_whitespace = !shown.ignore_whitespace;
            quiet(reload_diff(state))
        }
        Message::Up if state.actions.sync_menu.is_some() => sync_handlers::step_menu(state, false),
        Message::Down if state.actions.sync_menu.is_some() => sync_handlers::step_menu(state, true),
        Message::Enter if state.actions.sync_menu.is_some() => sync_handlers::run_cursor(state),
        Message::Up | Message::Down | Message::Left | Message::Right | Message::Enter
            if commit_menu::is_open(state) =>
        {
            commit_menu::key(state, &message)
        }
        Message::Up | Message::Down | Message::Left | Message::Right | Message::Enter
            if file_views::menu_is_open(state) =>
        {
            file_views::menu_key(state, &message)
        }
        Message::Up if state.picker.is_some() => {
            picker::move_cursor(state, -1);
            quiet(Task::none())
        }
        Message::Down if state.picker.is_some() => {
            picker::move_cursor(state, 1);
            quiet(Task::none())
        }
        Message::Up => quiet(move_cursor(state, -1)),
        Message::Down => quiet(move_cursor(state, 1)),
        // On a group the arrows fold it first; otherwise they move
        // between the areas, without wrapping around.
        Message::Left => {
            let folded = fold_at_cursor(state, true);
            if folded.is_none() && state.area != Area::Sidebar {
                state.area = state.area.previous_list();
            }
            (Task::none(), folded)
        }
        Message::Right => {
            let folded = fold_at_cursor(state, false);
            if folded.is_none() && state.area != Area::Detail {
                state.area = state.area.next_list();
                if state.area == Area::Detail && state.changes_shown() {
                    wip::enter_pane(state);
                }
            }
            (Task::none(), folded)
        }
        Message::Enter => enter(state, ctx),
        Message::NextArea => quiet(wip::tab(state, true)),
        Message::PreviousArea => quiet(wip::tab(state, false)),
        Message::NextHunk => quiet(step_hunk(state, true)),
        Message::PreviousHunk => quiet(step_hunk(state, false)),
        Message::NextFile if state.changes_shown() => quiet(wip::move_selection(state, 1)),
        Message::PreviousFile if state.changes_shown() => quiet(wip::move_selection(state, -1)),
        Message::NextFile => quiet(step_file(state, 1)),
        Message::PreviousFile => quiet(step_file(state, -1)),
        Message::ViewFile => {
            let file = state
                .diff
                .as_ref()
                .and_then(|shown| shown.files.get(shown.file));
            let action = state
                .root
                .as_ref()
                .zip(file)
                .map(|(root, changed)| Action::ViewFile(root.join(&changed.path)));
            (Task::none(), action)
        }
        Message::Escape => escape(state, ctx),
        Message::Close => {
            state.close();
            (Task::none(), Some(Action::Close))
        }
        Message::OpenMenu(target) => open_menu(state, target),
        Message::OpenMenuKey => {
            let target = match state.area {
                Area::Commits if state.list_is_covered() => file_views::menu_of_current(state),
                Area::Commits => state
                    .commit_at(state.commit_cursor)
                    .map(|commit| MenuTarget::Commit(commit.id.clone())),
                Area::Detail => file_views::menu_of_current(state),
                Area::Sidebar | Area::SidebarSplit | Area::DetailSplit => state
                    .sidebar_items()
                    .get(state.sidebar_cursor)
                    .and_then(|item| menu_target_of(state, *item)),
            };
            state.actions.menu = target.map(OpenMenu::new);
            file_views::start_menu_on_first_enabled(state);
            quiet(Task::none())
        }
        file @ (Message::OpenHistory(_)
        | Message::OpenBlame(..)
        | Message::HistoryKey
        | Message::BlameKey
        | Message::HistoryLoaded(..)
        | Message::HistoryDiffLoaded(..)
        | Message::BlameLoaded(..)
        | Message::HistoryPressed(_)
        | Message::HistoryMore
        | Message::HistoryScrolled(_)
        | Message::BlamePressed(_)
        | Message::BlameScrolled(_)
        | Message::FileMenu(_)) => file_views::handle(state, file, ctx),
        Message::Cleanup(request) => crate::cleanup::update(state, request),
        Message::ConfirmDialog if matches!(state.actions.dialog, Some(Dialog::Tag(_))) => {
            commit_menu::handle(state, Message::CreateTag, ctx)
        }
        commit @ (Message::CommitAction(_)
        | Message::MenuCursor(_)
        | Message::MenuSubmenuCursor(_)
        | Message::MarkCommit
        | Message::CommitsScrolled(_)
        | Message::TagNameChanged(_)
        | Message::CreateTag
        | Message::ResetLossLoaded(..)
        | Message::CompareLoaded(..)) => commit_menu::handle(state, commit, ctx),
        Message::CloseMenu => {
            state.actions.menu = None;
            sync_handlers::close_menu(state);
            quiet(Task::none())
        }
        Message::OpenSyncMenu(kind) => sync_handlers::toggle_menu(state, kind, ctx),
        Message::SyncMenuPressed(action) => sync_handlers::run_entry(state, action),
        Message::SyncMenuCursor(entry) => sync_handlers::hover(state, entry),
        Message::SyncMenuLetter(typed) => sync_handlers::letter(state, typed),
        Message::ForcePush => sync_handlers::force_push(state),
        Message::CredentialAsked(request) => sync_handlers::credential_asked(state, request),
        Message::CredentialTyped(id, typed) => sync_handlers::credential_typed(state, id, typed),
        Message::CredentialSend(id) => sync_handlers::send_credential(state, id),
        Message::CredentialCancel(id) => sync_handlers::cancel_credential_of(state, id),
        Message::BackgroundFetch => sync_handlers::background_fetch(state),
        Message::Refresh => {
            state.problem = None;
            let files = file_views::reload(state);
            quiet(Task::batch([load::everything(state), files]))
        }
        forwarded @ (Message::ActionsLoaded(..)
        | Message::JobDone(_)
        | Message::SelectChange(..)
        | Message::Stage(_)
        | Message::Unstage(_)
        | Message::StageAll
        | Message::UnstageAll
        | Message::StageHunk(_)
        | Message::DiscardHunk(_)
        | Message::UnstageHunk(_)
        | Message::DiscardFile(_)
        | Message::DiscardAll
        | Message::CommitTitleChanged(_)
        | Message::CommitBodyEdited(_)
        | Message::ToggleAmend
        | Message::ToggleWipSection(_)
        | Message::TogglePushAfter
        | Message::ToggleStageSelected
        | Message::FocusCommitTitle
        | Message::WipFocused(_)
        | Message::WipFieldPressed(_)
        | Message::IgnoreFile(_)
        | Message::IgnoreSelected
        | Message::IgnoreChoiceChanged(_)
        | Message::OpenSelectedInEditor
        | Message::Commit
        | Message::CommitAndPush
        | Message::Fetch
        | Message::Pull
        | Message::Push
        | Message::Stash
        | Message::Undo
        | Message::MenuPressed(_)
        | Message::NewBranchNameChanged(_)
        | Message::ToggleNewBranchCheckout
        | Message::CreateBranch
        | Message::CloseDialog
        | Message::ConfirmDialog
        | Message::RenameNameChanged(_)
        | Message::MergeAbort
        | Message::MergeContinue
        | Message::SelectConflict(_)
        | Message::TakeBlock(..)
        | Message::ConflictPressed(..)
        | Message::ConflictShortcut(_)
        | Message::PullFirst
        | Message::PullWith(_)
        | Message::DeleteBranchAnyway
        | Message::Retry
        | Message::ToggleNoticeDetails
        | Message::DismissNotice) => handlers::act(state, forwarded, ctx),
        moved @ (Message::OpenPicker(_)
        | Message::PickerQuery(_)
        | Message::PickerPressed(_)
        | Message::PickerEnterWithStash
        | Message::PickerNewBranch
        | Message::PickerSubmit
        | Message::SplitPressed(_)
        | Message::SplitHover(_)
        | Message::SplitLeave
        | Message::SplitReset(_)
        | Message::SplitNudge(..)
        | Message::PaneMoved(_)
        | Message::PaneReleased
        | Message::JumpToHead
        | Message::JumpToWip
        | Message::JumpToCommit(_)
        | Message::GoToParent
        | Message::CopySha
        | Message::ToggleTreeKey
        | Message::ChipPressed(_)
        | Message::ContainingLoaded(..)) => navigate::handle(state, moved, ctx),
    }
}

fn refs_loaded(
    state: &mut State,
    ctx: &Ctx,
    generation: u64,
    result: Result<RefsOverview, String>,
) -> Outcome {
    if !state.loads.refs.is_current(generation) {
        return quiet(Task::none());
    }
    match result {
        Ok(overview) => {
            state.overview = Some(overview);
            handlers::refresh_sync(state, ctx);
            state.sidebar_cursor = state
                .sidebar_cursor
                .min(state.sidebar_items().len().saturating_sub(1));
            quiet(ensure_detail(state))
        }
        Err(reason) => failed(state, ctx, &reason),
    }
}

fn log_loaded(
    state: &mut State,
    ctx: &Ctx,
    generation: u64,
    result: Result<Log, String>,
) -> Outcome {
    if !state.loads.log.is_current(generation) {
        return quiet(Task::none());
    }
    match result {
        Ok(log) => {
            let cursor_sha = state
                .commit_at(state.commit_cursor)
                .map(|commit| commit.id.clone());
            state.log = Some(log);
            if let Some(row) = cursor_sha.and_then(|sha| state.row_of_commit(&sha)) {
                state.commit_cursor = row;
            }
            state.close_menu_if_stale();
            let jump = match state.pending_jump.take() {
                Some(sha) => navigate::jump_to(state, sha),
                None => Task::none(),
            };
            quiet(Task::batch([ensure_detail(state), jump]))
        }
        Err(reason) => failed(state, ctx, &reason),
    }
}

fn detail_loaded(
    state: &mut State,
    ctx: &Ctx,
    generation: u64,
    result: Result<CommitDetail, String>,
) -> Outcome {
    if !state.loads.detail.is_current(generation) {
        return quiet(Task::none());
    }
    match result {
        Ok(info) => {
            let sha = info.id.clone();
            state.file_cursor = 0;
            state.detail = Some(Detail {
                subject: Subject::Commit(info.id.clone()),
                files: info.files.clone(),
                info: Some(info),
            });
            state.close_menu_if_stale();
            quiet(load::containing(state, sha))
        }
        Err(reason) => failed(state, ctx, &reason),
    }
}

fn working_loaded(
    state: &mut State,
    ctx: &Ctx,
    generation: u64,
    result: Result<Vec<ChangedFile>, String>,
) -> Outcome {
    if !state.loads.detail.is_current(generation) {
        return quiet(Task::none());
    }
    match result {
        Ok(files) => {
            state.file_cursor = 0;
            state.detail = Some(Detail {
                subject: Subject::Working,
                info: None,
                files,
            });
            state.close_menu_if_stale();
            quiet(Task::none())
        }
        Err(reason) => failed(state, ctx, &reason),
    }
}

fn diff_loaded(
    state: &mut State,
    ctx: &Ctx,
    generation: u64,
    result: Result<FileDiff, String>,
) -> Outcome {
    if !state.loads.diff.is_current(generation) {
        return quiet(Task::none());
    }
    match result {
        Ok(diff) => {
            if let Some(shown) = state.diff.as_mut() {
                shown.diff = Some(diff);
                shown.hunk = 0;
            }
            quiet(operation::scroll_to(
                DIFF_ID,
                AbsoluteOffset {
                    x: None,
                    y: Some(0.0),
                },
            ))
        }
        Err(reason) => failed(state, ctx, &reason),
    }
}

fn file_pressed(state: &mut State, index: usize) -> Outcome {
    state.area = Area::Detail;
    state.file_cursor = index;
    file_views::close_blame(state);
    if state.diff_has_own_list() {
        if let Some(shown) = state
            .diff
            .as_mut()
            .filter(|shown| index < shown.files.len())
        {
            shown.file = index;
            shown.diff = None;
            shown.hunk = 0;
        }
        return quiet(reload_diff(state));
    }
    quiet(open_diff(state, index))
}

fn open_menu(state: &mut State, target: MenuTarget) -> Outcome {
    if let MenuTarget::Commit(sha) = &target {
        let Some(row) = state.row_of_commit(sha) else {
            return quiet(Task::none());
        };
        state.area = Area::Commits;
        state.commit_cursor = row;
    }
    if let MenuTarget::File { path, .. } = &target {
        state.area = Area::Detail;
        let mut listed = state.files_in_detail().iter();
        if let Some(index) = listed.position(|file| file.path == *path) {
            state.file_cursor = index;
        }
    }
    state.actions.menu = Some(OpenMenu::new(target));
    file_views::start_menu_on_first_enabled(state);
    state.close_menu_if_stale();
    quiet(Task::none())
}

pub fn failed(state: &mut State, ctx: &Ctx, reason: &str) -> Outcome {
    state.problem = Some(fill_template(
        ctx.lang.text(Msg::GitViewReadFailed),
        &[("{reason}", reason)],
    ));
    quiet(Task::none())
}

/// Reads the detail of the row under the cursor unless it is the one shown.
pub fn ensure_detail(state: &mut State) -> Task<Message> {
    let row = state.commit_cursor.min(state.row_count().saturating_sub(1));
    state.commit_cursor = row;
    let Some(subject) = state.subject_at(row) else {
        return Task::none();
    };
    if state
        .detail
        .as_ref()
        .is_some_and(|detail| detail.subject == subject)
    {
        return Task::none();
    }
    state.detail = None;
    state.containing = None;
    state.copied_sha = None;
    match subject {
        Subject::Working => load::working_changes(state),
        Subject::Commit(sha) => load::commit_detail(state, sha),
        Subject::Compare { .. } => Task::none(),
    }
}

pub fn load_more(state: &mut State) -> Task<Message> {
    if !state.log.as_ref().is_some_and(|log| log.has_more) {
        return Task::none();
    }
    state.limit += PAGE;
    load::log(state)
}

/// Folds `fold` if it is open and unfolds it if it is folded; the action tells
/// the app what to remember.
fn toggle_fold(state: &mut State, fold: Fold) -> Option<Action> {
    if fold == Fold::Section(Section::Remotes) {
        state.remotes_chosen = true;
    }
    if !state.collapsed.remove(&fold) {
        state.collapsed.insert(fold);
    }
    state.sidebar_cursor = state
        .sidebar_cursor
        .min(state.sidebar_items().len().saturating_sub(1));
    Some(Action::FoldsChanged {
        root: state.root.clone()?,
        folded: state.folded_keys(),
    })
}

/// Folds or unfolds the group under the cursor; `None` if there is none or it is
/// as asked already.
fn fold_at_cursor(state: &mut State, collapse: bool) -> Option<Action> {
    if state.area != Area::Sidebar {
        return None;
    }
    let item = state.sidebar_items().get(state.sidebar_cursor).copied()?;
    let fold = state.fold_of(item)?;
    if state.collapsed.contains(&fold) == collapse {
        return None;
    }
    toggle_fold(state, fold)
}

/// What a menu on this row of the left list is about.
pub fn menu_target_of(state: &State, item: SidebarItem) -> Option<MenuTarget> {
    match item {
        SidebarItem::Local(branch) => Some(MenuTarget::LocalBranch(branch)),
        SidebarItem::Worktree(index) => Some(MenuTarget::Worktree(index)),
        SidebarItem::Remote(branch) => Some(MenuTarget::RemoteBranch(branch)),
        SidebarItem::Stash(stash) => state
            .actions
            .stash_ids
            .get(stash)
            .map(|id| MenuTarget::Stash(id.clone())),
        _ => None,
    }
}

fn activate_sidebar(state: &mut State, index: usize) -> Outcome {
    let item = state.sidebar_items().get(index).copied();
    if let Some(fold) = item.and_then(|row| state.fold_of(row)) {
        return (Task::none(), toggle_fold(state, fold));
    }
    match item {
        Some(SidebarItem::Worktree(position)) => {
            let path = state
                .overview
                .as_ref()
                .and_then(|overview| overview.worktrees.get(position))
                .map(|worktree| worktree.path.clone());
            (Task::none(), path.map(Action::OpenWorktreeTab))
        }
        _ => quiet(Task::none()),
    }
}

fn enter(state: &mut State, ctx: &Ctx) -> Outcome {
    if state.actions.dialog_open() {
        if let Some(Dialog::Ask(question)) = &state.actions.dialog {
            if question.enter_is_safe() {
                return handlers::act(state, Message::CloseDialog, ctx);
            }
        }
        if matches!(state.actions.dialog, Some(Dialog::Tag(_))) {
            return commit_menu::handle(state, Message::CreateTag, ctx);
        }
        // The field of a login question sends its own Enter with the id of the
        // question; this one would also hit the next question after it.
        if let Some(Dialog::Credential(ask)) = &state.actions.dialog {
            return match ask.kind {
                ncrs_git::askpass::PromptKind::Confirm => sync_handlers::credential_enter(state),
                ncrs_git::askpass::PromptKind::Text | ncrs_git::askpass::PromptKind::Secret => {
                    quiet(Task::none())
                }
            };
        }
        // A text field sends Enter as its own submit too; the second one finds nothing to do.
        let answer = if state.actions.dialog.is_some() {
            Message::ConfirmDialog
        } else {
            Message::CreateBranch
        };
        return handlers::act(state, answer, ctx);
    }
    if file_views::has_keys(state) {
        return quiet(file_views::key(state, &Message::Enter));
    }
    if state.diff.is_some() && state.area != Area::Sidebar {
        return quiet(Task::none());
    }
    match state.area {
        Area::Sidebar => {
            let index = state.sidebar_cursor;
            activate_sidebar(state, index)
        }
        Area::Commits => {
            state.area = Area::Detail;
            if state.changes_shown() {
                wip::enter_pane(state);
            }
            quiet(Task::none())
        }
        Area::Detail if state.changes_shown() => wip::enter(state),
        Area::Detail => {
            let index = state.file_cursor;
            quiet(open_diff(state, index))
        }
        Area::SidebarSplit | Area::DetailSplit => quiet(Task::none()),
    }
}

/// The diff of a change is in the middle only while the working copy row is the chosen one.
fn close_wip_diff(state: &mut State) {
    if state.commit_cursor != 0 {
        if let Some(local) = state.actions.local.as_mut() {
            local.diff_open = false;
        }
    }
}

pub fn step(cursor: usize, delta: isize, len: usize) -> usize {
    cursor
        .saturating_add_signed(delta)
        .min(len.saturating_sub(1))
}

fn move_cursor(state: &mut State, delta: isize) -> Task<Message> {
    if file_views::has_keys(state) {
        let key = if delta < 0 {
            Message::Up
        } else {
            Message::Down
        };
        return file_views::key(state, &key);
    }
    if state.diff.is_some() && matches!(state.area, Area::Commits | Area::Detail) {
        return scroll_page_by(DIFF_ID, delta as f32 * SCROLL_LINES * size::ROW);
    }
    match state.area {
        Area::Sidebar => {
            let len = state.sidebar_items().len();
            state.sidebar_cursor = step(state.sidebar_cursor, delta, len);
            Task::none()
        }
        Area::Commits => {
            let rows = state.row_count();
            state.commit_cursor = step(state.commit_cursor, delta, rows);
            close_wip_diff(state);
            let detail = ensure_detail(state);
            let near_end = state.commit_cursor + 1 >= rows;
            if near_end && delta > 0 {
                Task::batch([detail, load_more(state)])
            } else {
                detail
            }
        }
        Area::Detail if state.changes_shown() => wip::move_selection(state, delta),
        Area::Detail => {
            let len = state.files_in_detail().len();
            state.file_cursor = step(state.file_cursor, delta, len);
            Task::none()
        }
        Area::SidebarSplit | Area::DetailSplit => Task::none(),
    }
}

fn target_of(subject: &Subject, file: &ChangedFile) -> DiffTarget {
    DiffTarget {
        commit: match subject {
            Subject::Commit(sha) => Some(sha.clone()),
            Subject::Working | Subject::Compare { .. } => None,
        },
        path: file.path.clone(),
        old_path: file.old_path.clone(),
    }
}

fn open_diff(state: &mut State, index: usize) -> Task<Message> {
    let Some(detail) = &state.detail else {
        return Task::none();
    };
    if index >= detail.files.len() {
        return Task::none();
    }
    state.diff = Some(DiffState {
        subject: detail.subject.clone(),
        files: detail.files.clone(),
        file: index,
        diff: None,
        unified: false,
        ignore_whitespace: false,
        hunk: 0,
    });
    reload_diff(state)
}

pub fn reload_diff(state: &mut State) -> Task<Message> {
    let Some(shown) = &state.diff else {
        return Task::none();
    };
    let Some(file) = shown.files.get(shown.file) else {
        return Task::none();
    };
    let ignore_whitespace = shown.ignore_whitespace;
    if let Subject::Compare { from, to } = &shown.subject {
        let (ends, compared) = ((from.clone(), to.clone()), file.clone());
        return load::compare_diff(state, ends, &compared, ignore_whitespace);
    }
    let target = target_of(&shown.subject, file);
    load::file_diff(state, target, ignore_whitespace)
}

fn step_file(state: &mut State, delta: isize) -> Task<Message> {
    let Some(shown) = state.diff.as_mut() else {
        return Task::none();
    };
    let next = step(shown.file, delta, shown.files.len());
    if next == shown.file {
        return Task::none();
    }
    shown.file = next;
    shown.diff = None;
    if !state.diff_has_own_list() {
        state.file_cursor = next;
    }
    reload_diff(state)
}

fn step_hunk(state: &mut State, forward: bool) -> Task<Message> {
    if state.changes_shown() {
        return wip::step_hunk(state, forward);
    }
    let Some(shown) = state.diff.as_mut() else {
        return Task::none();
    };
    let Some(diff) = &shown.diff else {
        return Task::none();
    };
    let last = diff.hunks.len().saturating_sub(1);
    shown.hunk = if forward {
        (shown.hunk + 1).min(last)
    } else {
        shown.hunk.saturating_sub(1)
    };
    scroll_to_hunk(state)
}

pub fn scroll_to_hunk(state: &State) -> Task<Message> {
    let Some(shown) = &state.diff else {
        return Task::none();
    };
    let Some(diff) = &shown.diff else {
        return Task::none();
    };
    let rows = rows_before(diff, shown.unified, shown.hunk);
    operation::scroll_to(
        DIFF_ID,
        AbsoluteOffset {
            x: None,
            y: Some(rows as f32 * size::ROW),
        },
    )
}

fn escape(state: &mut State, ctx: &Ctx) -> Outcome {
    if state.picker.is_some() {
        return quiet(picker::close(state));
    }
    if state.actions.dialog_open() {
        return handlers::act(state, Message::CloseDialog, ctx);
    }
    if state.actions.sync_menu.is_some() {
        sync_handlers::close_menu(state);
        return quiet(Task::none());
    }
    if state.actions.menu.is_some() {
        state.actions.menu = None;
        return quiet(Task::none());
    }
    if state.column_drag.take().is_some() {
        return quiet(Task::none());
    }
    if state.filter_focused {
        state.filter_focused = false;
        return quiet(release_focus());
    }
    if file_views::escape_step(state) {
        return quiet(Task::none());
    }
    if let Some(task) = wip::escape_step(state) {
        return quiet(task);
    }
    if state.diff.take().is_some() {
        state.loads.diff.cancel();
        return quiet(Task::none());
    }
    state.close();
    (Task::none(), Some(Action::Close))
}
