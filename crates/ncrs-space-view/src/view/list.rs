//! The groups and their rows, the empty note, the summary under them and the detail pane
//! beside them. After a run the same list shows what the run did to each row.

use iced::widget::{column, container, mouse_area, row, Space};
use iced::{alignment, Color, Element, Length, Padding};
use ncrs_i18n::{fill_template, Msg};
use ncrs_space::decide::DecideKind;
use ncrs_space::Group;
use ncrs_ui_kit::folded_row::folded_row;
use ncrs_ui_kit::format::{shown_path, size_shown};
use ncrs_ui_kit::git_state::RunOutcome;
use ncrs_ui_kit::kit::{self, Check};
use ncrs_ui_kit::page_scroll::page_scroll_tracked_padded;
use ncrs_ui_kit::page_text::page_line;
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::row_scroll::FOCUSED_ROW_ID;
use ncrs_ui_kit::section_head::{section_head_with, SectionHead};
use ncrs_ui_kit::size_row::{size_row_ext, size_row_header, SizeRow, SizeRowExt, SizeRowHeader};
use ncrs_ui_kit::theme::{self, font_size, size, spacing};

use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::rows::{matches_filter, DisplayRow, RowKind, GROUP_ORDER};
use crate::state::{CursorKey, State};
use crate::texts::{
    fill_count, fill_count_size, fill_size, group_title, row_check, row_name, row_reason, skip_text,
};
use crate::update::LIST_ID;

pub fn group_color(group: Group, palette: &Palette) -> Color {
    match group {
        Group::Safe => theme::space::safe(palette),
        Group::Worktrees => theme::space::worktrees(),
        Group::Decide => theme::space::decide(),
        Group::InUse => theme::space::in_use(palette),
    }
}

/// The column of the buttons exists only when a row of the list has one: a row a run skipped.
fn has_actions(state: &State) -> bool {
    state
        .result()
        .is_some_and(|done| done.log.rows.iter().any(|row| row.outcome.is_err()))
}

fn one_row<'a>(
    row: &'a DisplayRow,
    biggest: u64,
    state: &'a State,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let is_cursor = state.cursor == Some(CursorKey::Row(row.path.clone()));
    let in_result = state.result().is_some();
    let check = if in_result {
        None
    } else if row.is_deletable() {
        Some(if state.is_ticked(row) {
            Check::On
        } else {
            Check::Off
        })
    } else {
        (row.group == Group::InUse).then_some(Check::Off)
    };
    let (name, path, chip) = match &row.kind {
        RowKind::Worktree(branch) => (
            shown_path(&row.path, ctx.home),
            String::new(),
            Some(branch.clone()),
        ),
        _ => (
            row_name(row, ctx.lang),
            shown_path(&row.path, ctx.home),
            None,
        ),
    };
    let mut ext = SizeRowExt {
        check_column: !in_result,
        chip,
        ..SizeRowExt::default()
    };
    let mut reason = row_reason(row, ctx.lang);
    let mut outcome = None;
    if let Some(result) = state.result() {
        match result.logged(&row.path).map(|logged| &logged.outcome) {
            Some(Ok(freed)) => {
                ext.gone = true;
                outcome = Some((
                    RunOutcome::Updated,
                    fill_size(Msg::SpaceResultRowDeleted, *freed, ctx.lang),
                ));
            }
            Some(Err(skip)) => {
                let kind = if matches!(skip, ncrs_space::Skip::Failed(_)) {
                    RunOutcome::Failed
                } else {
                    RunOutcome::Skipped
                };
                outcome = Some((kind, skip_text(skip, ctx.lang)));
                ext.action_column = true;
                ext.action = Some((ctx.lang.text(Msg::SpaceRetry).to_owned(), Message::Retry));
            }
            None => {
                reason = match (&row.kind, row.group) {
                    (_, Group::InUse) => reason,
                    (RowKind::Place(DecideKind::ModelCache), _) => {
                        ctx.lang.text(Msg::SpaceResultRowUntouchedModels).to_owned()
                    }
                    _ => ctx.lang.text(Msg::SpaceResultRowUntouched).to_owned(),
                };
            }
        }
    } else {
        ext.check = row_check(row, ctx.processes_known, ctx.lang);
    }
    ext.action_column = has_actions(state);
    let built = size_row_ext(
        SizeRow {
            check,
            name,
            path,
            size_text: size_shown(row.bytes, ctx.lang),
            fraction: row.bytes as f32 / biggest.max(1) as f32,
            color: group_color(row.group, ctx.palette),
            reason,
            outcome,
            cursor: is_cursor,
            on_press: Some(Message::RowPressed(row.path.clone())),
            on_toggle: row
                .is_deletable()
                .then(|| Message::RowToggled(row.path.clone())),
        },
        ext,
        ctx.palette,
    );
    let framed = if is_cursor {
        container(built).id(FOCUSED_ROW_ID).into()
    } else {
        built
    };
    mouse_area(framed)
        .on_right_press(Message::OpenMenuAt(row.path.clone()))
        .on_double_click(Message::Enter)
        .into()
}

/// What the head of a group says next to its title.
fn head_hint(group: Group, state: &State, ctx: ViewCtx<'_>) -> Option<String> {
    let in_result = state.result().is_some();
    Some(match group {
        Group::Safe if in_result => ctx.lang.text(Msg::SpaceGroupSafeResultHint).to_owned(),
        Group::Safe => return None,
        Group::Worktrees => {
            let roots = state.scan().map(|scan| scan.worktree_roots.as_slice())?;
            let first = roots.first()?;
            let total_count: usize = roots.iter().map(|root| root.total.count).sum();
            let total_bytes: u64 = roots.iter().map(|root| root.total.bytes).sum();
            let kept: usize = roots.iter().map(|root| root.kept.count).sum();
            fill_template(
                ctx.lang.text(Msg::SpaceGroupWorktreesHint),
                &[
                    ("{folder}", &shown_path(&first.folder, ctx.home)),
                    ("{total}", &total_count.to_string()),
                    ("{size}", &size_shown(total_bytes, ctx.lang)),
                    ("{kept}", &kept.to_string()),
                ],
            )
        }
        Group::Decide if in_result => ctx.lang.text(Msg::SpaceGroupDecideResultHint).to_owned(),
        Group::Decide => ctx.lang.text(Msg::SpaceGroupDecideHint).to_owned(),
        Group::InUse => ctx.lang.text(Msg::SpaceGroupInUseHint).to_owned(),
    })
}

/// The box of a head: on, off or partly, of the rows the group lets the user tick.
fn head_box<'a>(group: Group, state: &State, ctx: ViewCtx<'a>) -> Option<Element<'a, Message>> {
    if state.result().is_some() {
        return None;
    }
    match group {
        Group::Safe | Group::Worktrees => {
            let rows = state.group_rows(group).count();
            let ticked = state.ticked_count(group);
            let shown = if ticked == 0 {
                Check::Off
            } else if ticked == rows {
                Check::On
            } else {
                Check::Mixed
            };
            Some(kit::checkbox(
                shown,
                Some(Message::HeadTicked(group)),
                ctx.palette,
            ))
        }
        Group::InUse => Some(kit::checkbox(Check::Off, None, ctx.palette)),
        Group::Decide => None,
    }
}

fn group_view<'a>(
    group: Group,
    biggest: u64,
    state: &'a State,
    ctx: ViewCtx<'a>,
) -> Option<Element<'a, Message>> {
    let rows: Vec<&DisplayRow> = state
        .group_rows(group)
        .filter(|row| matches_filter(row, state.filter()))
        .collect();
    if rows.is_empty() {
        return None;
    }
    let in_result = state.result().is_some();
    let title = if in_result && group == Group::Safe {
        ctx.lang.text(Msg::SpaceGroupSafeResult).to_owned()
    } else {
        group_title(group, ctx.lang)
    };
    let bytes: u64 = rows
        .iter()
        .fold(0, |sum: u64, row| sum.saturating_add(row.bytes));
    let sum_text = if in_result && group == Group::Safe {
        fill_template(
            ctx.lang.text(Msg::SpaceGroupSafeResultSum),
            &[
                ("{freed}", &size_shown(state.freed_in(group), ctx.lang)),
                (
                    "{left}",
                    &size_shown(bytes.saturating_sub(state.freed_in(group)), ctx.lang),
                ),
            ],
        )
    } else {
        size_shown(bytes, ctx.lang)
    };
    let mut extras: Vec<Element<'a, Message>> = Vec::new();
    if let Some(hint) = head_hint(group, state, ctx) {
        extras.push(page_line(hint, font_size::SMALL, ctx.palette.dim_text));
    }
    extras.push(page_line(sum_text, font_size::SMALL, ctx.palette.dim_text));
    if let Some(tick) = head_box(group, state, ctx) {
        extras.push(tick);
    }
    let head = section_head_with(
        SectionHead {
            title: &title,
            count: rows.len(),
            open: !state.is_folded(group),
            has_focus: state.cursor == Some(CursorKey::Head(group)),
            on_toggle: Message::HeadToggled(group),
        },
        vec![row(extras)
            .spacing(spacing::XL)
            .align_y(alignment::Vertical::Center)
            .into()],
        ctx.palette,
    );
    let mut body = column![head];
    if in_result && group == Group::Worktrees {
        if !state.is_folded(group) {
            body = body.push(worktree_summary(state, ctx));
        }
    } else if !state.is_folded(group) {
        let shown = state.row_limit(group);
        let hidden: Vec<&DisplayRow> = rows.iter().skip(shown).copied().collect();
        for row in rows.into_iter().take(shown) {
            body = body.push(one_row(row, biggest, state, ctx));
        }
        if !hidden.is_empty() {
            let hidden_bytes = hidden
                .iter()
                .fold(0, |sum: u64, row| sum.saturating_add(row.bytes));
            body = body.push(folded_row(
                &fill_count_size(
                    Msg::SpaceGroupMoreWorktrees,
                    hidden.len(),
                    hidden_bytes,
                    ctx.lang,
                ),
                "\u{2192}",
                ctx.lang.text(Msg::SpaceGroupExpand),
                Message::ExpandWorktrees,
                ctx.palette,
            ));
        }
    }
    Some(body.into())
}

/// After a run the worktrees are two lines: those removed or still there, and those that stay.
fn worktree_summary<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let removed: Vec<&DisplayRow> = state
        .group_rows(Group::Worktrees)
        .filter(|row| state.is_done(row))
        .collect();
    let there: Vec<&DisplayRow> = state
        .group_rows(Group::Worktrees)
        .filter(|row| !state.is_done(row))
        .collect();
    let bytes_of = |rows: &[&DisplayRow]| rows.iter().fold(0, |sum, row| sum + row.bytes);
    let line =
        |name: String, path: String, reason: String, bytes: u64, gone: bool, palette: &Palette| {
            size_row_ext(
                SizeRow {
                    check: None,
                    name,
                    path,
                    size_text: size_shown(bytes, ctx.lang),
                    fraction: 1.0,
                    color: group_color(Group::Worktrees, palette),
                    reason,
                    outcome: None,
                    cursor: false,
                    on_press: None,
                    on_toggle: None,
                },
                SizeRowExt {
                    gone,
                    action_column: has_actions(state),
                    ..SizeRowExt::default()
                },
                palette,
            )
        };
    let mut lines = column![];
    if !removed.is_empty() {
        lines = lines.push(line(
            fill_template(
                ctx.lang.text(Msg::SpaceResultWorktreesDone),
                &[
                    ("{count}", &removed.len().to_string()),
                    ("{size}", &size_shown(bytes_of(&removed), ctx.lang)),
                ],
            ),
            String::new(),
            String::new(),
            bytes_of(&removed),
            true,
            ctx.palette,
        ));
    }
    if !there.is_empty() {
        lines = lines.push(line(
            fill_count(Msg::SpaceResultWorktreesClean, there.len(), ctx.lang),
            String::new(),
            fill_template(
                ctx.lang.text(Msg::SpaceResultRowStillThere),
                &[(
                    "{key}",
                    &ctx.keys.label(crate::shortcuts::Shortcut::RemoveWorktrees),
                )],
            ),
            bytes_of(&there),
            false,
            ctx.palette,
        ));
    }
    if let Some(scan) = state.scan() {
        let kept: usize = scan.worktree_roots.iter().map(|root| root.kept.count).sum();
        let kept_bytes: u64 = scan.worktree_roots.iter().map(|root| root.kept.bytes).sum();
        if kept > 0 {
            lines = lines.push(line(
                fill_count(Msg::SpaceResultWorktreesStay, kept, ctx.lang),
                ctx.lang.text(Msg::SpaceResultWorktreesStayWhy).to_owned(),
                ctx.lang.text(Msg::SpaceResultRowStay).to_owned(),
                kept_bytes,
                false,
                ctx.palette,
            ));
        }
    }
    lines.into()
}

fn groups<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let biggest = state.rows().iter().map(|row| row.bytes).max().unwrap_or(0);
    let shown: Vec<Element<'a, Message>> = GROUP_ORDER
        .iter()
        .filter_map(|group| group_view(*group, biggest, state, ctx))
        .collect();
    if shown.is_empty() {
        let note = if state.is_measuring() || state.scan().is_none() {
            String::new()
        } else {
            ctx.lang.text(Msg::SpaceEmpty).to_owned()
        };
        return container(page_line(note, font_size::BODY, ctx.palette.dim_text))
            .padding(spacing::XXL)
            .into();
    }
    column(shown).width(Length::Fill).into()
}

fn column_header<'a>(state: &State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let word = |msg: Msg| ctx.lang.text(msg).to_owned();
    let header = if state.result().is_some() {
        SizeRowHeader {
            bar: String::new(),
            name: word(Msg::SpaceColumnName),
            reason: word(Msg::SpaceColumnResult),
            check: None,
            size: word(Msg::SpaceColumnSize),
            action_column: has_actions(state),
        }
    } else {
        SizeRowHeader {
            bar: word(Msg::SpaceColumnSize),
            name: word(Msg::SpaceColumnFolder),
            reason: word(Msg::SpaceColumnWhat),
            check: Some(word(Msg::SpaceColumnCheck)),
            size: word(Msg::SpaceColumnUsed),
            action_column: false,
        }
    };
    container(size_row_header(header, ctx.palette))
        .padding(Padding::from([0.0, spacing::XL]))
        .into()
}

/// What is ticked, and where the safe things go.
fn footer<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let safe = state.open_ticked_count(Group::Safe);
    let worktrees = if ctx.store_build {
        0
    } else {
        state.open_ticked_count(Group::Worktrees)
    };
    let bytes = state
        .open_ticked_bytes(Group::Safe)
        .saturating_add(if ctx.store_build {
            0
        } else {
            state.open_ticked_bytes(Group::Worktrees)
        });
    let mut line = row![]
        .spacing(spacing::XL)
        .align_y(alignment::Vertical::Center);
    if state.result().is_none() && safe + worktrees > 0 {
        let template = if worktrees > 0 {
            Msg::SpaceSelectedSummary
        } else {
            Msg::SpaceSelectedSummarySafe
        };
        line = line.push(page_line(
            fill_template(
                ctx.lang.text(template),
                &[
                    ("{safe}", &safe.to_string()),
                    ("{worktrees}", &worktrees.to_string()),
                    ("{size}", &size_shown(bytes, ctx.lang)),
                ],
            ),
            font_size::BODY,
            palette.success,
        ));
    }
    let note = if state.result().is_some() {
        Msg::SpaceResultNote
    } else {
        Msg::SpacePermanentNote
    };
    if state.result().is_some() {
        line = line.push(page_line(
            ctx.lang.text(Msg::SpaceResultPermanent),
            font_size::SMALL,
            palette.dim_text,
        ));
    }
    line = line.push(Space::new().width(Length::Fill)).push(page_line(
        ctx.lang.text(note),
        font_size::SMALL,
        palette.dim_text,
    ));
    column![
        theme::horizontal_rule(palette.border_inactive),
        container(line)
            .padding([spacing::M, spacing::XL])
            .width(Length::Fill),
    ]
    .into()
}

/// The scrolling groups with their column header and summary, and, outside a result, the
/// detail pane of the row under the cursor.
pub fn view<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let scroll = page_scroll_tracked_padded(
        LIST_ID,
        groups(state, ctx),
        Padding::from([0.0, spacing::XL]),
        Message::ListScrolled,
    );
    let list = column![
        column_header(state, ctx),
        container(scroll).width(Length::Fill).height(Length::Fill),
        footer(state, ctx),
    ]
    .width(Length::Fill)
    .height(Length::Fill);
    if state.result().is_some() {
        return list.into();
    }
    row![
        list,
        container(super::detail::view(state, ctx))
            .width(size::DETAIL_PANE)
            .height(Length::Fill),
    ]
    .height(Length::Fill)
    .into()
}
