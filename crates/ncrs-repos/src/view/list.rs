//! The groups and rows of the list, in both shapes: the list itself and the
//! list of a run, where the last column holds the result.

use std::path::{Path, PathBuf};

use iced::widget::{column, container, mouse_area};
use iced::{Element, Length, Padding};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::format::{date_text, local_time, DateFormat};
use ncrs_ui_kit::git_state::{outcome_mark, outcome_mark_inked};
use ncrs_ui_kit::kit::{self, Check, Tone};
use ncrs_ui_kit::page_scroll::page_scroll_tracked_padded;
use ncrs_ui_kit::page_text::page_line;
use ncrs_ui_kit::repo_row::{
    path_tail, repo_changes, repo_header_titled, repo_row, repo_standing, BranchTag, RepoColumns,
    RepoRow,
};
use ncrs_ui_kit::row_scroll::FOCUSED_ROW_ID;
use ncrs_ui_kit::section_head::{section_head, SectionHead};
use ncrs_ui_kit::theme::{font_size, spacing};

use crate::bulk::Outcome;
use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::rows::{name_of, Group, GroupKey, Row, RowKind};
use crate::state::State;
use crate::summary::Reading;
use crate::texts::log_text;
use crate::update::LIST_ID;

/// The widest a location gets before its start is cut.
const LOCATION_CHARS: usize = 52;
/// The last column of a run holds the result and needs more room.
const RESULT_WIDTH: f32 = 440.0;

/// The chips of "3 behind origin/main" and "11 local changes" and the time "yesterday 17:01"
/// need more than the standard widths.
/// Beside the clean-up there is only room for the branch and the standing.
fn columns(state: &State) -> RepoColumns {
    let is_run = state.run().is_some();
    let beside_cleanup = state.cleanup.is_some();
    RepoColumns {
        location: !beside_cleanup,
        standing: 250.0,
        changes: if beside_cleanup { 0.0 } else { 160.0 },
        fetched: if beside_cleanup { 0.0 } else { 122.0 },
        actions: if is_run {
            RESULT_WIDTH
        } else {
            RepoColumns::STANDARD.actions
        },
        ..RepoColumns::STANDARD
    }
}

/// What a repository could lose; none when nothing or not known yet.
fn cleanable_count(state: &State, path: &Path) -> Option<usize> {
    state
        .cleanable
        .get(path)
        .copied()
        .filter(|count| *count > 0)
}

fn shortened(path: &Path, home: Option<&Path>) -> String {
    let text = home
        .and_then(|root| path.strip_prefix(root).ok())
        .map_or_else(
            || path.to_string_lossy().into_owned(),
            |rest| format!("~/{}", rest.to_string_lossy()),
        );
    path_tail(&text, LOCATION_CHARS)
}

fn group_title(key: &GroupKey, ctx: ViewCtx<'_>) -> String {
    match key {
        GroupKey::Favourites => ctx.lang.text(Msg::ReposGroupFavourites).to_owned(),
        GroupKey::Open => ctx.lang.text(Msg::ReposGroupOpen).to_owned(),
        GroupKey::Unreachable => ctx.lang.text(Msg::ReposGroupUnreachable).to_owned(),
        GroupKey::Added => ctx.lang.text(Msg::ReposGroupAdded).to_owned(),
        GroupKey::FoundIn(folder) => fill_template(
            ctx.lang.text(Msg::ReposGroupFoundIn),
            &[("{folder}", &shortened(folder, ctx.home))],
        ),
    }
}

fn fetched_text(reading: Option<&Reading>, ctx: ViewCtx<'_>) -> String {
    let at = reading
        .and_then(Reading::summary)
        .and_then(|summary| summary.fetched_at)
        .and_then(local_time);
    match (at, local_time(ctx.now)) {
        (Some(fetched), Some(now)) => date_text(fetched, now, DateFormat::Relative, ctx.lang),
        _ => ctx.lang.text(Msg::ReposNeverFetched).to_owned(),
    }
}

fn result_cell<'a>(
    cursor: bool,
    path: &Path,
    state: &'a State,
    ctx: ViewCtx<'a>,
) -> Option<Element<'a, Message>> {
    let outcome = state.run()?.result(path)?;
    let detail = match outcome {
        Outcome::AlreadyCurrent | Outcome::Running => None,
        other => Some(log_text(other, ctx.lang)),
    };
    let mark = if cursor {
        outcome_mark_inked(outcome.kind(), ctx.lang, ctx.palette.cursor_text)
    } else {
        outcome_mark(outcome.kind(), ctx.lang, ctx.palette)
    };
    let mut cell = iced::widget::row![mark].spacing(spacing::M);
    if let Some(note) = detail {
        let ink = if cursor {
            ctx.palette.cursor_text
        } else {
            ctx.palette.dim_text
        };
        cell = cell.push(page_line(note, font_size::SMALL, ink));
    }
    Some(cell.into())
}

fn one_row<'a>(
    row: &Row,
    state: &'a State,
    cursor: Option<&PathBuf>,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let path = row.path.clone();
    let reading = state.reading(&path);
    let summary = reading.and_then(Reading::summary);
    let is_run = state.run().is_some();
    let is_worktree = row.kind == RowKind::Worktree;
    let unreachable = reading.is_some_and(Reading::is_unreachable);
    let branch = match (&row.branch, summary) {
        (Some(name), _) => Some(BranchTag {
            name: name.clone(),
            is_default: false,
        }),
        (None, Some(known)) => known.branch.as_ref().map(|name| BranchTag {
            name: name.clone(),
            is_default: known.default_branch.as_ref() == Some(name),
        }),
        (None, None) => None,
    };
    let (standing, changes) = match summary.filter(|_| !is_worktree) {
        Some(shown) => (
            repo_standing(
                shown.behind,
                shown.ahead,
                &format!("origin/{}", shown.branch.as_deref().unwrap_or_default()),
            ),
            repo_changes(u32::try_from(shown.changed).unwrap_or(u32::MAX), 0),
        ),
        None => (Vec::new(), Vec::new()),
    };
    let list_has_keys = state
        .cleanup
        .as_ref()
        .is_none_or(|cleanup| cleanup.list_focus);
    let is_cursor = cursor == Some(&path) && list_has_keys;
    let note_ink = if is_cursor {
        ctx.palette.cursor_text
    } else {
        ctx.palette.text
    };
    let actions = if is_run {
        result_cell(is_cursor, &row.owner, state, ctx).filter(|_| !is_worktree)
    } else if let Some(count) =
        cleanable_count(state, &path).filter(|_| !is_worktree && !unreachable)
    {
        Some(page_line(count.to_string(), font_size::BODY, note_ink))
    } else if unreachable {
        Some(kit::button(
            ctx.lang.text(Msg::ReposRemove),
            Some(Message::RowRemoved(path.clone())),
            Tone::Secondary,
            ctx.palette,
        ))
    } else {
        None
    };
    let item = RepoRow {
        name: name_of(&path),
        location: shortened(&path, ctx.home),
        branch,
        standing,
        changes,
        fetched: if is_worktree {
            String::new()
        } else {
            fetched_text(reading, ctx)
        },
        favourite: !is_worktree && state.store().is_favourite(&path),
        checked: !is_worktree && state.is_marked(&path),
        cursor: is_cursor,
        worktree: is_worktree,
        unreachable,
        on_toggle: (!is_worktree).then(|| Message::RowToggled(path.clone())),
        on_favourite: (!is_worktree).then(|| Message::FavouriteClicked(path.clone())),
        on_standing: is_run.then(|| Message::ShowDetails(path.clone())),
        actions,
    };
    let built = repo_row(item, columns(state), ctx.lang, ctx.palette);
    let framed = if is_cursor {
        container(built).id(FOCUSED_ROW_ID).into()
    } else {
        built
    };
    mouse_area(framed)
        .on_press(Message::RowPressed(path.clone()))
        .on_enter(Message::Hovered(Some(path.clone())))
        .on_exit(Message::Hovered(None))
        .on_right_press(Message::OpenMenuAt(path.clone()))
        .on_double_click(Message::RowOpened(path))
        .into()
}

fn group_view<'a>(
    group: &Group,
    state: &'a State,
    cursor: Option<&PathBuf>,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let title = group_title(&group.key, ctx);
    let mut body = column![section_head(
        SectionHead {
            title: &title,
            count: group.count,
            open: group.open,
            has_focus: false,
            on_toggle: Message::GroupToggled(group.key.clone()),
        },
        ctx.palette,
    )];
    for row in &group.rows {
        body = body.push(one_row(row, state, cursor, ctx));
    }
    body.into()
}

fn all_checked(state: &State) -> Check {
    let marked = state.marked_repos().len();
    let repos = state
        .groups()
        .iter()
        .map(|group| group.count)
        .sum::<usize>();
    match marked {
        0 => Check::Off,
        count if count >= repos => Check::On,
        _ => Check::Mixed,
    }
}

/// The column head and the scrolling groups below it.
pub fn view<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let is_run = state.run().is_some();
    let cleanable_title = (!is_run).then(|| ctx.lang.text(Msg::ReposColumnCleanable));
    let header = container(repo_header_titled(
        columns(state),
        ctx.lang,
        ctx.palette,
        all_checked(state),
        Some(Message::AllToggled),
        cleanable_title,
    ))
    .padding(Padding::from([0.0, spacing::XXL]));
    let cursor = state.cursor();
    let groups = state.groups();
    let body: Element<'a, Message> = if groups.is_empty() {
        container(page_line(
            ctx.lang.text(Msg::ReposEmpty),
            font_size::BODY,
            ctx.palette.dim_text,
        ))
        .padding(spacing::XXL)
        .into()
    } else {
        column(
            groups
                .iter()
                .map(|group| group_view(group, state, cursor.as_ref(), ctx)),
        )
        .width(Length::Fill)
        .into()
    };
    column![
        header,
        page_scroll_tracked_padded(
            LIST_ID,
            body,
            Padding::from([0.0, spacing::XXL]),
            Message::ListScrolled
        ),
        footnote(is_run, ctx),
    ]
    .height(Length::Fill)
    .into()
}

fn footnote<'a>(is_run: bool, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    if is_run {
        return iced::widget::Space::new().into();
    }
    container(page_line(
        ctx.lang.text(Msg::ReposCleanableNote),
        font_size::SMALL,
        ctx.palette.dim_text,
    ))
    .padding(Padding::from([spacing::M, spacing::XXL]))
    .into()
}
