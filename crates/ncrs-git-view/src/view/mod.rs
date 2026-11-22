//! The Git tab: header with the filter, sidebar, commit list and detail pane,
//! or the diff of one file in their place.

mod commits;
mod detail;
mod diff;
mod sidebar;

use iced::widget::{column, container, row, Space};
use iced::{alignment, Element, Length};
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_ui_kit::kit::text_field;
use ncrs_ui_kit::page_status_bar::{page_status_bar, PageStatusBar, PageStatusLine};
use ncrs_ui_kit::page_text::{page_heading, page_line};
use ncrs_ui_kit::theme::{self, font_size, size, spacing};

use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::state::{Area, State};

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
    let hint = ctx.lang.text(hint_of(state));
    let line = state
        .problem
        .as_deref()
        .map_or(PageStatusLine::Hint(hint), PageStatusLine::Failure);
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
            container(commits::view(state, ctx)).width(Length::Fill),
            theme::vertical_rule(palette.border_inactive),
            container(detail::view(state, ctx)).width(if is_narrow(ctx) {
                size::SIDE_PANE
            } else {
                size::SIDE_PANE * 1.5
            }),
        ]
        .height(Length::Fill)
        .into(),
    };
    column![
        header(state, ctx),
        theme::horizontal_rule(palette.border_inactive),
        body,
        page_status_bar(bar, palette),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
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
    container(
        row![
            page_heading(&state.name, palette),
            page_line(branch, font_size::BODY, palette.dim_text),
            Space::new().width(Length::Fill),
            container(filter).width(size::FIELD_WIDE),
        ]
        .spacing(spacing::L)
        .align_y(alignment::Vertical::Center),
    )
    .padding([0.0, spacing::XXL])
    .height(size::HEADER)
    .center_y(size::HEADER)
    .into()
}
