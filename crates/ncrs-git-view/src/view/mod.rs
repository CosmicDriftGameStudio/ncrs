//! The Git tab: header with the filter, sidebar, commit list and detail pane,
//! or the diff of one file in their place.

mod changes;
mod commits;
mod detail;
mod diff;
mod merge;
mod overlay;
mod sidebar;
mod toolbar;

use iced::widget::{column, container, row, stack, Space};
use iced::{alignment, Element, Length};
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_ui_kit::action_rows::BANNER_HEIGHT;
use ncrs_ui_kit::git_rows::drag_capture;
use ncrs_ui_kit::kit::text_field;
use ncrs_ui_kit::page_status_bar::{page_status_bar, PageStatusBar, PageStatusLine};
use ncrs_ui_kit::page_text::{page_heading, page_line};
use ncrs_ui_kit::theme::{self, font_size, size, spacing};

use crate::actions::Unavailable;
use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::shortcuts::Shortcut;
use crate::state::{Area, State};
pub use overlay::{NAME_ID, RENAME_ID};

/// The ids of the fields and scroll areas, for the focus and scroll tasks.
pub const FILTER_ID: &str = "git-filter";
pub const DIFF_ID: &str = "git-diff";
const COMMITS_ID: &str = "git-commits";
const DETAIL_ID: &str = "git-detail";
const SIDEBAR_ID: &str = "git-sidebar";
/// Below this window width the detail pane is as wide as the sidebar and the
/// commit list drops its SHA column, so the message keeps its room. Windows of
/// 1400 and more keep the full layout.
const NARROW_WIDTH: f32 = 1400.0;

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
    let bar = PageStatusBar {
        mode: ctx.lang.text(Msg::GitViewMode),
        line,
        right: loaded.as_deref(),
    };
    let body: Element<'a, Message> = match &state.diff {
        Some(shown) => diff::view(shown, state.detail.as_ref(), ctx),
        None => row![
            container(sidebar::view(state, ctx)).width(size::SIDE_PANE),
            theme::vertical_rule(palette.border_inactive),
            container(middle_pane(state, ctx)).width(Length::Fill),
            theme::vertical_rule(palette.border_inactive),
            container(detail_pane(state, ctx)).width(detail_width(state, ctx)),
        ]
        .height(Length::Fill)
        .into(),
    };
    let mut top = column![
        header(state, ctx),
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
    size::HEADER
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

fn detail_width(state: &State, ctx: ViewCtx<'_>) -> f32 {
    if !matches!(pane_of(state), Pane::Commit) {
        return size::SIDE_PANE * 1.5;
    }
    if is_narrow(ctx) {
        size::SIDE_PANE
    } else {
        size::SIDE_PANE * 1.5
    }
}

/// How wide the commit list is: what the sidebar, the detail pane and the rules between leave.
fn commits_pane_width(state: &State, ctx: ViewCtx<'_>) -> f32 {
    ctx.window.width - size::SIDE_PANE - detail_width(state, ctx) - 2.0 * spacing::BORDER_WIDTH
}

/// The keys of the focus; with the actions on, the keys of the actions too.
fn hint_line(state: &State, ctx: ViewCtx<'_>) -> String {
    let key = |shortcut: Shortcut| shortcut.label(ctx.keys);
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
        Area::Commits => Msg::GitViewStatusCommits,
        Area::Detail => Msg::GitViewStatusDetail,
    }
}

fn header<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let branch = state
        .overview
        .as_ref()
        .map(
            |overview| match overview.local.iter().find(|branch| branch.is_current) {
                Some(branch) => format!(
                    "{} \u{2191}{} \u{2193}{}",
                    branch.name, branch.ahead, branch.behind
                ),
                None => state
                    .current_worktree()
                    .and_then(|worktree| worktree.head_short.clone())
                    .map(|short| {
                        fill_template(
                            ctx.lang.text(Msg::GitViewWorktreeDetachedAt),
                            &[("{sha}", &short)],
                        )
                    })
                    .unwrap_or_default(),
            },
        )
        .unwrap_or_default();
    let filter = text_field(
        ctx.lang.text(Msg::GitViewFilterPlaceholder),
        &state.filter,
        FILTER_ID,
        Some(Message::FilterChanged),
        Message::FilterSubmit,
        palette,
    );
    let mut line = row![
        page_heading(&state.name, palette),
        page_line(branch, font_size::BODY, palette.dim_text),
    ]
    .spacing(spacing::L)
    .align_y(alignment::Vertical::Center);
    if let Some(sync) = &state.actions.sync {
        line = line.push(toolbar::view(sync, state.actions.busy, ctx));
    }
    container(
        line.push(Space::new().width(Length::Fill))
            .push(container(filter).width(size::FIELD_WIDE)),
    )
    .padding([0.0, spacing::XXL])
    .height(size::HEADER)
    .center_y(size::HEADER)
    .into()
}
