//! The Git tab: header with the pickers, the toolbar and the filter, sidebar,
//! middle and detail pane with a splitter on each side. The middle is the
//! commit list or what lies over it.

mod blame;
mod changes;
mod commit_box;
mod commit_menu;
mod commits;
mod cleanup;
mod credentials;
mod detail;
mod diff;
mod diff_toolbar;
mod file_menu;
mod header;
mod history;
mod merge;
mod overlay;
mod sidebar;
mod sync_menu;
mod toolbar;
mod wip_pane;

use iced::widget::{column, container, row, stack};
use iced::{Element, Length, Padding};
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_ui_kit::action_rows::BANNER_HEIGHT;
use ncrs_ui_kit::git_rows::drag_capture;
use ncrs_ui_kit::page_status_bar::{page_status_bar, PageStatusBar, PageStatusLine};
use ncrs_ui_kit::page_text::page_line;
use ncrs_ui_kit::splitter::{splitter, width_label, SplitterMessages};
use ncrs_ui_kit::theme::git::SplitterLook;
use ncrs_ui_kit::theme::{self, font_size, size, spacing};

use crate::actions::{Dialog, Unavailable, WipFocus};
use crate::ctx::ViewCtx;
use crate::file_views::{middle, Middle};
use crate::message::Message;
use crate::panes::{PaneWidths, Split};
use crate::shortcuts::Shortcut;
use crate::state::{Area, State};
pub use changes::HUNKS_ID;
pub use commit_box::{BODY_ID, TITLE_ID};
pub use commit_menu::TAG_ID;
pub use overlay::{NAME_ID, RENAME_ID};

/// The ids of the fields and scroll areas, for the focus and scroll tasks.
pub const FILTER_ID: &str = "git-filter";
pub const DIFF_ID: &str = "git-diff";
pub const COMMITS_ID: &str = "git-commits";
pub const HISTORY_ID: &str = "git-history";
pub const HISTORY_DIFF_ID: &str = "git-history-diff";
pub const BLAME_ID: &str = "git-blame";
const DETAIL_ID: &str = "git-detail";
const SIDEBAR_ID: &str = "git-sidebar";
/// Below this window width the detail pane is as wide as the sidebar at most
/// and the commit list drops its SHA column, so the message keeps its room.
const NARROW_WIDTH: f32 = 1400.0;
/// What the label with the width of a dragged bar takes at most.
const SPLIT_LABEL_WIDTH: f32 = 90.0;

fn is_narrow(ctx: ViewCtx<'_>) -> bool {
    ctx.window.width < NARROW_WIDTH
}

/// The title of the tab, `None` while the tab is not open.
pub fn tab_title(state: &State, lang: Language) -> Option<String> {
    state.is_open().then(|| {
        fill_template(
            lang.text(Msg::GitViewTabTitle),
            &[("{repo}", state.name.as_str())],
        )
    })
}

pub fn view<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let hint = hint_line(state, ctx);
    let line = state
        .problem
        .as_deref()
        .map_or(PageStatusLine::Hint(&hint), PageStatusLine::Failure);
    let loaded = state.log.as_ref().map(|log| {
        let commits = fill_template(
            ctx.lang.text(Msg::GitViewStatusLoaded),
            &[("{count}", &log.commits.len().to_string())],
        );
        let changed = state.local_change_count();
        if changed == 0 {
            return commits;
        }
        let local = match changed {
            1 => ctx.lang.text(Msg::GitViewStatusLocalOne).to_owned(),
            _ => fill_template(
                ctx.lang.text(Msg::GitViewStatusLocalMany),
                &[("{count}", &changed.to_string())],
            ),
        };
        format!("{local} \u{b7} {commits}")
    });
    let copied = state
        .copied_sha
        .as_ref()
        .map(|short| fill_template(ctx.lang.text(Msg::GitViewShaCopied), &[("{sha}", short)]));
    let right = copied.or_else(|| diff_summary(state, ctx)).or(loaded);
    let bar = PageStatusBar {
        mode: ctx.lang.text(Msg::GitViewMode),
        line,
        right: right.as_deref(),
    };
    let body = row![
        container(sidebar::view(state, ctx)).width(sidebar_width(ctx, state)),
        splitter_between(Split::Sidebar, state, ctx),
        container(middle_pane(state, ctx)).width(Length::Fill),
        splitter_between(Split::Detail, state, ctx),
        container(detail_pane(state, ctx)).width(detail_width(state, ctx)),
    ]
    .height(Length::Fill);
    let mut top = column![
        header::view(state, ctx),
        theme::horizontal_rule(palette.border_inactive),
    ]
    .width(Length::Fill)
    .height(Length::Fill);
    if let Some(reason) = state.actions.unavailable {
        let text = match reason {
            Unavailable::GitMissing => Msg::GitViewHintGitMissing,
            Unavailable::Sandbox => Msg::GitViewHintSandbox,
        };
        top = top.push(
            container(page_line(
                ctx.lang.text(text),
                font_size::SMALL,
                palette.dim_text,
            ))
            .padding([0.0, spacing::XXL])
            .height(size::ROW)
            .center_y(size::ROW),
        );
    }
    if let Some(merge) = &state.actions.merge {
        top = top.push(merge::banner_view(merge, state.actions.rebasing, ctx));
    }
    if let Some(notice) = &state.actions.notice {
        top = top.push(overlay::notice_banner(notice, ctx));
    }
    let page = top.push(body).push(page_status_bar(bar, palette));
    if state.column_drag.is_some() {
        return stack![
            page,
            drag_capture(Message::ResizeMoved, Message::ResizeReleased)
        ]
        .into();
    }
    if state.pane_drag.is_some() {
        let mut layers = stack![
            page,
            drag_capture(Message::PaneMoved, Message::PaneReleased)
        ];
        if let Some(label) = drag_label(state, ctx) {
            layers = layers.push(label);
        }
        return layers.into();
    }
    overlay::over(page.into(), state, ctx)
}

/// The width of the pane of the local changes and of a file's history: the
/// buttons of a file and the lines of a diff need the room.
const WIP_PANE: f32 = 520.0;

/// What the right side shows.
enum Pane {
    Commit,
    /// The files of a diff that is not about the commit under the cursor.
    Files,
    /// What the chosen commit changed in the file of the history.
    History,
    Changes,
    Merge,
}

fn pane_of(state: &State) -> Pane {
    if state.diff.is_some() {
        return if state.diff_has_own_list() {
            Pane::Files
        } else {
            Pane::Commit
        };
    }
    if state.history.is_some() {
        return Pane::History;
    }
    if state.actions.merge.is_some() {
        return Pane::Merge;
    }
    if state.changes_shown() {
        return Pane::Changes;
    }
    Pane::Commit
}

/// The middle: the commit list, or what lies over it.
fn middle_pane<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let shown = match middle(state) {
        Middle::Commits => None,
        Middle::Blame => state
            .blame
            .as_ref()
            .map(|shown| blame::view(state, shown, ctx)),
        Middle::Diff => state
            .diff
            .as_ref()
            .map(|shown| diff::view(state, shown, ctx)),
        Middle::History => state
            .history
            .as_ref()
            .map(|shown| history::view(state, shown, ctx)),
        Middle::Conflict => state
            .actions
            .merge
            .as_ref()
            .map(|merge| merge::diff_view(merge, ctx)),
        Middle::LocalDiff => state.actions.local.as_ref().map(|local| {
            let has_keys = state.area == Area::Detail
                && matches!(local.focus, WipFocus::Unstaged | WipFocus::Staged);
            changes::diff_view(local, has_keys, ctx)
        }),
    };
    shown.unwrap_or_else(|| commits::view(state, ctx))
}

/// One line in place of a list.
fn notice<'a>(ctx: ViewCtx<'a>, text: Msg) -> Element<'a, Message> {
    container(page_line(
        ctx.lang.text(text),
        font_size::BODY,
        ctx.palette.dim_text,
    ))
    .padding(spacing::M)
    .into()
}

/// The right end of the status bar while a diff is in view.
fn diff_summary(state: &State, ctx: ViewCtx<'_>) -> Option<String> {
    match middle(state) {
        Middle::Diff => {
            let shown = state.diff.as_ref()?;
            let file = shown.files.get(shown.file)?;
            diff::summary(&file.path, shown.diff.as_ref(), ctx)
        }
        Middle::History => {
            let shown = state.history.as_ref()?;
            diff::summary(&shown.entry()?.path_at_commit, shown.diff.as_ref(), ctx)
        }
        Middle::LocalDiff => {
            let local = state.actions.local.as_ref()?;
            let path = crate::wip::selected_path(local)?;
            diff::summary(path, local.diff.as_ref(), ctx)
        }
        Middle::Commits | Middle::Conflict | Middle::Blame => None,
    }
}

/// How far down the body starts: the header, its rule and the banners.
fn body_top(state: &State) -> f32 {
    let banner = |present: bool| if present { BANNER_HEIGHT } else { 0.0 };
    size::PICKER_HEADER
        + spacing::BORDER_WIDTH
        + if state.actions.unavailable.is_some() {
            size::ROW
        } else {
            0.0
        }
        + banner(state.actions.merge.is_some())
        + banner(state.actions.notice.is_some())
}

fn detail_pane<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let shown = match pane_of(state) {
        Pane::Commit => None,
        Pane::Files => state.diff.as_ref().map(|shown| {
            let width = detail_width(state, ctx);
            diff::file_list(shown, width, ctx)
        }),
        Pane::History => state
            .history
            .as_ref()
            .map(|shown| history::diff_pane(state, shown, ctx)),
        Pane::Merge => state
            .actions
            .merge
            .as_ref()
            .map(|merge| merge::view(merge, ctx)),
        Pane::Changes => state
            .actions
            .local
            .as_ref()
            .map(|local| wip_pane::view(state, local, ctx)),
    };
    shown.unwrap_or_else(|| detail::view(state, ctx))
}

/// The widths as shown: the ones being dragged, else the remembered ones.
fn panes_in_view(state: &State, ctx: ViewCtx<'_>) -> PaneWidths {
    state.pane_drag.map_or(ctx.panes, |drag| drag.current)
}

pub fn sidebar_width(ctx: ViewCtx<'_>, state: &State) -> f32 {
    panes_in_view(state, ctx).width(Split::Sidebar)
}

/// A narrow window keeps the detail pane to the width of the sidebar at most,
/// so the commit list keeps its room.
fn detail_width(state: &State, ctx: ViewCtx<'_>) -> f32 {
    let wanted = panes_in_view(state, ctx).width(Split::Detail);
    match pane_of(state) {
        Pane::Commit | Pane::Files if is_narrow(ctx) => wanted.min(size::SIDE_PANE),
        Pane::Changes | Pane::History => wanted.max(WIP_PANE),
        Pane::Commit | Pane::Files | Pane::Merge => wanted,
    }
}

/// How wide the commit list is: what the sidebar, the detail pane and the bars between leave.
fn commits_pane_width(state: &State, ctx: ViewCtx<'_>) -> f32 {
    ctx.window.width - sidebar_width(ctx, state) - detail_width(state, ctx) - 2.0 * size::SPLITTER
}

fn splitter_between<'a>(split: Split, state: &State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let look = if state.pane_drag.is_some_and(|drag| drag.split == split) {
        SplitterLook::Dragged
    } else if state.hovered_split == Some(split) {
        SplitterLook::Hovered
    } else {
        SplitterLook::Idle
    };
    let at = match split {
        Split::Sidebar => Area::SidebarSplit,
        Split::Detail => Area::DetailSplit,
    };
    splitter(
        look,
        state.area == at,
        SplitterMessages {
            press: Message::SplitPressed(split),
            reset: Message::SplitReset(split),
            hover: Message::SplitHover(split),
            leave: Message::SplitLeave,
        },
        ctx.lang,
        ctx.palette,
    )
}

/// The width of the bar being dragged, near the bar.
fn drag_label<'a>(state: &State, ctx: ViewCtx<'a>) -> Option<Element<'a, Message>> {
    let drag = state.pane_drag?;
    let widths = drag.current;
    let left = match drag.split {
        Split::Sidebar => widths.width(Split::Sidebar) + spacing::M,
        Split::Detail => {
            ctx.window.width - widths.width(Split::Detail) - SPLIT_LABEL_WIDTH - spacing::M
        }
    };
    Some(
        container(width_label(widths.width(drag.split), ctx.lang, ctx.palette))
            .padding(Padding {
                left: left.max(0.0),
                top: body_top(state) + spacing::M,
                ..Padding::ZERO
            })
            .into(),
    )
}

/// The keys of the focus; with the actions on, the keys of the actions too.
fn hint_line(state: &State, ctx: ViewCtx<'_>) -> String {
    let key = |shortcut: Shortcut| shortcut.label(ctx.keys);
    if matches!(state.actions.dialog, Some(Dialog::Cleanup(_))) {
        return ctx.lang.text(Msg::GitViewStatusCleanup).to_owned();
    }
    if matches!(state.actions.dialog, Some(Dialog::Credential(_))) {
        return ctx.lang.text(Msg::GitViewStatusCredentials).to_owned();
    }
    if state.actions.sync_menu.is_some() {
        return ctx.lang.text(Msg::GitViewStatusSyncMenu).to_owned();
    }
    if crate::commit_menu::is_open(state) {
        return ctx.lang.text(Msg::GitViewStatusCommitMenu).to_owned();
    }
    if state.picker.is_some() {
        return ctx.lang.text(Msg::GitViewStatusPicker).to_owned();
    }
    if state.area.split().is_some() {
        return ctx.lang.text(Msg::GitViewStatusSplitter).to_owned();
    }
    let covered = state.list_is_covered();
    if covered && !state.filter_focused && state.area != Area::Sidebar {
        return match middle(state) {
            Middle::Blame => fill_template(
                ctx.lang.text(Msg::GitViewStatusBlame),
                &[("{history}", &key(Shortcut::FileHistory))],
            ),
            Middle::History => ctx.lang.text(Msg::GitViewStatusHistory).to_owned(),
            _ => fill_template(
                ctx.lang.text(Msg::GitViewStatusDiff),
                &[
                    ("{previous_file}", &key(Shortcut::PreviousFile)),
                    ("{next_file}", &key(Shortcut::NextFile)),
                ],
            ),
        };
    }
    let acting = state.actions.sync.is_some() && !covered && !state.filter_focused;
    if acting && state.actions.merge.is_some() && state.area != Area::Sidebar {
        return fill_template(
            ctx.lang.text(Msg::GitViewStatusMerge),
            &[
                ("{mine}", &key(Shortcut::TakeMine)),
                ("{theirs}", &key(Shortcut::TakeTheirs)),
                ("{both}", &key(Shortcut::TakeBoth)),
                ("{editor}", &key(Shortcut::OpenEditor)),
                ("{resolved}", &key(Shortcut::MarkResolved)),
                ("{menu}", &key(Shortcut::OpenMenu)),
            ],
        );
    }
    if acting && matches!(pane_of(state), Pane::Changes) && state.area != Area::Sidebar {
        let focus = state
            .actions
            .local
            .as_ref()
            .map(|local| (local.focus, local.diff_open));
        return match focus {
            Some((WipFocus::Unstaged | WipFocus::Staged, true)) => fill_template(
                ctx.lang.text(Msg::GitViewStatusWipDiff),
                &[("{other_file}", &key(Shortcut::NextFile))],
            ),
            Some((WipFocus::Unstaged | WipFocus::Staged, false)) | None => fill_template(
                ctx.lang.text(Msg::GitViewStatusWipFiles),
                &[
                    ("{stage_all}", &key(Shortcut::StageAll)),
                    ("{unstage_all}", &key(Shortcut::UnstageAll)),
                    ("{commit}", &key(Shortcut::Commit)),
                ],
            ),
            Some((_, _)) => fill_template(
                ctx.lang.text(Msg::GitViewStatusWipCommitBox),
                &[
                    ("{commit}", &key(Shortcut::Commit)),
                    ("{commit_push}", &key(Shortcut::CommitAndPush)),
                ],
            ),
        };
    }
    let acting = state.actions.sync.is_some() && !state.filter_focused;
    if acting && state.area == Area::Sidebar {
        return fill_template(
            ctx.lang.text(Msg::GitViewStatusSidebarActions),
            &[
                ("{menu}", &key(Shortcut::OpenMenu)),
                ("{new_branch}", &key(Shortcut::NewBranch)),
            ],
        );
    }
    if !covered && !state.filter_focused && state.area == Area::Commits {
        return fill_template(
            ctx.lang.text(Msg::GitViewStatusCommitsKeys),
            &[
                ("{head}", &key(Shortcut::JumpHead)),
                ("{wip}", &key(Shortcut::JumpWip)),
                ("{parent}", &key(Shortcut::GoParent)),
                ("{copy}", &key(Shortcut::CopySha)),
            ],
        );
    }
    ctx.lang.text(hint_of(state)).to_owned()
}

fn hint_of(state: &State) -> Msg {
    if state.filter_focused {
        return Msg::GitViewStatusFilter;
    }
    match state.area {
        Area::Sidebar => Msg::GitViewStatusSidebar,
        Area::Commits | Area::SidebarSplit | Area::DetailSplit => Msg::GitViewStatusCommits,
        Area::Detail => Msg::GitViewStatusDetail,
    }
}
