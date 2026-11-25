//! The Git tab: header with the pickers, the toolbar and the filter, sidebar,
//! commit list and detail pane with a splitter on each side, or the diff of
//! one file in their place.

mod changes;
mod commits;
mod detail;
mod diff;
mod header;
mod merge;
mod overlay;
mod sidebar;
mod toolbar;

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

use crate::actions::Unavailable;
use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::panes::{PaneWidths, Split};
use crate::shortcuts::Shortcut;
use crate::state::{Area, State};
pub use overlay::{NAME_ID, RENAME_ID};

/// The ids of the fields and scroll areas, for the focus and scroll tasks.
pub const FILTER_ID: &str = "git-filter";
pub const DIFF_ID: &str = "git-diff";
pub const COMMITS_ID: &str = "git-commits";
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
        fill_template(
            ctx.lang.text(Msg::GitViewStatusLoaded),
            &[("{count}", &log.commits.len().to_string())],
        )
    });
    let copied = state
        .copied_sha
        .as_ref()
        .map(|short| fill_template(ctx.lang.text(Msg::GitViewShaCopied), &[("{sha}", short)]));
    let right = copied.or(loaded);
    let bar = PageStatusBar {
        mode: ctx.lang.text(Msg::GitViewMode),
        line,
        right: right.as_deref(),
    };
    let body: Element<'a, Message> = match &state.diff {
        Some(shown) => diff::view(shown, state.detail.as_ref(), ctx),
        None => row![
            container(sidebar::view(state, ctx)).width(sidebar_width(ctx, state)),
            splitter_between(Split::Sidebar, state, ctx),
            container(middle_pane(state, ctx)).width(Length::Fill),
            splitter_between(Split::Detail, state, ctx),
            container(detail_pane(state, ctx)).width(detail_width(state, ctx)),
        ]
        .height(Length::Fill)
        .into(),
    };
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

/// What the right side shows: the commit, the local changes or a merge.
enum Pane {
    Commit,
    Changes,
    Merge,
}

fn pane_of(state: &State) -> Pane {
    if state.actions.merge.is_some() {
        return Pane::Merge;
    }
    if state.actions.local.is_some() && state.actions.changes_open {
        return Pane::Changes;
    }
    Pane::Commit
}

/// The middle: the commit list, or the diff of the chosen file while the
/// changes or a merge are the subject.
fn middle_pane<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    match (pane_of(state), &state.actions.merge, &state.actions.local) {
        (Pane::Merge, Some(merge), _) => merge::diff_view(merge, ctx),
        (Pane::Changes, _, Some(local)) => changes::diff_view(local, ctx),
        _ => commits::view(state, ctx),
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
    match (pane_of(state), &state.actions.merge, &state.actions.local) {
        (Pane::Merge, Some(merge), _) => merge::view(merge, ctx),
        (Pane::Changes, _, Some(local)) => changes::view(local, ctx),
        _ => detail::view(state, ctx),
    }
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
    if matches!(pane_of(state), Pane::Commit) && is_narrow(ctx) {
        wanted.min(size::SIDE_PANE)
    } else {
        wanted
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
    if state.picker.is_some() {
        return ctx.lang.text(Msg::GitViewStatusPicker).to_owned();
    }
    if state.diff.is_none() && state.area.split().is_some() {
        return ctx.lang.text(Msg::GitViewStatusSplitter).to_owned();
    }
    let acting = state.actions.sync.is_some() && state.diff.is_none() && !state.filter_focused;
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
        return fill_template(
            ctx.lang.text(Msg::GitViewStatusChanges),
            &[
                ("{stage_all}", &key(Shortcut::StageAll)),
                ("{unstage_all}", &key(Shortcut::UnstageAll)),
                ("{commit}", &key(Shortcut::Commit)),
            ],
        );
    }
    if acting && state.area == Area::Sidebar {
        return fill_template(
            ctx.lang.text(Msg::GitViewStatusSidebarActions),
            &[
                ("{menu}", &key(Shortcut::OpenMenu)),
                ("{new_branch}", &key(Shortcut::NewBranch)),
            ],
        );
    }
    if state.diff.is_none() && !state.filter_focused && state.area == Area::Commits {
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
    if state.diff.is_some() {
        return Msg::GitViewStatusDiff;
    }
    if state.filter_focused {
        return Msg::GitViewStatusFilter;
    }
    match state.area {
        Area::Sidebar => Msg::GitViewStatusSidebar,
        Area::Commits | Area::SidebarSplit | Area::DetailSplit => Msg::GitViewStatusCommits,
        Area::Detail => Msg::GitViewStatusDetail,
    }
}
