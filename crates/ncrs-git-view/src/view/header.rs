//! The head of the tab: repository and branch pickers, the state chips, the
//! worktree path, the buttons and the filter.

use iced::widget::{container, row, text, Space};
use iced::{alignment, Element, Length};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::format::home_relative;
use ncrs_ui_kit::git_state::{standing_chip, Standing};
use ncrs_ui_kit::header_picker::{
    header_picker, picker_list, picker_separator, PickerLine, PickerMark, PickerRow,
};
use ncrs_ui_kit::kit::text_field;
use ncrs_ui_kit::list_below::list_below_own_width;
use ncrs_ui_kit::open_list::{window_start, OPEN_LIST_ROWS};
use ncrs_ui_kit::theme::git::PickerLook;
use ncrs_ui_kit::theme::{font_size, size, spacing};

use super::{toolbar, FILTER_ID};
use crate::ctx::ViewCtx;
use crate::git_keys::chord_label;
use crate::message::{Chip, Message};
use crate::picker::{self, Choice, Entry, PickerKind, PICKER_ID};
use crate::shortcuts::Shortcut;
use crate::state::State;

/// How wide the open list of a picker is.
const PICKER_WIDTH: f32 = 440.0;

pub fn view<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let open_kind = state.picker.as_ref().map(|open| open.kind);
    let look = |kind: PickerKind| {
        if open_kind == Some(kind) {
            PickerLook::Open
        } else {
            PickerLook::Idle
        }
    };
    let branch_name = current_branch_label(state, ctx);
    let mut repository: Element<'a, Message> = header_picker(
        lang.text(Msg::GitViewPickerRepository),
        &state.name,
        look(PickerKind::Repository),
        Message::OpenPicker(PickerKind::Repository),
        palette,
    );
    if open_kind == Some(PickerKind::Repository) {
        repository =
            list_below_own_width(repository, picker_view(state, ctx, PickerKind::Repository));
    }
    let mut branch: Element<'a, Message> = header_picker(
        lang.text(Msg::GitViewPickerBranch),
        &branch_name,
        look(PickerKind::Branch),
        Message::OpenPicker(PickerKind::Branch),
        palette,
    );
    if open_kind == Some(PickerKind::Branch) {
        branch = list_below_own_width(branch, picker_view(state, ctx, PickerKind::Branch));
    }
    let mut line = row![repository, picker_separator(palette), branch]
        .spacing(spacing::M)
        .align_y(alignment::Vertical::Center);
    for (standing, chip) in standings(state) {
        line = line.push(standing_chip(
            &standing,
            lang,
            palette,
            Some(Message::ChipPressed(chip)),
        ));
    }
    if let Some(path) = state
        .current_worktree()
        .map(|worktree| worktree.path.as_path())
    {
        line = line.push(
            text(fill_template(
                lang.text(Msg::GitViewWorktreePath),
                &[("{path}", &home_relative(path, ctx.home))],
            ))
            .size(font_size::SMALL)
            .color(palette.dim_text)
            .wrapping(text::Wrapping::None),
        );
    }
    let filter = text_field(
        lang.text(Msg::GitViewFilterPlaceholder),
        &state.filter,
        FILTER_ID,
        Some(Message::FilterChanged),
        Message::FilterSubmit,
        palette,
    );
    container(
        line.push(Space::new().width(Length::Fill))
            .push(toolbar::view(state, ctx))
            .push(container(filter).width(size::FIELD_WIDE))
            .spacing(spacing::L),
    )
    .padding([0.0, spacing::XXL])
    .height(size::PICKER_HEADER)
    .center_y(size::PICKER_HEADER)
    .into()
}

fn current_branch_label(state: &State, ctx: ViewCtx<'_>) -> String {
    let Some(overview) = &state.overview else {
        return String::new();
    };
    match overview.local.iter().find(|branch| branch.is_current) {
        Some(branch) => branch.name.clone(),
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
    }
}

/// The chips that have a value; a chip for "nothing" is not drawn.
fn standings(state: &State) -> Vec<(Standing, Chip)> {
    let mut chips = Vec::new();
    let current = state
        .overview
        .as_ref()
        .and_then(|overview| overview.local.iter().find(|branch| branch.is_current));
    if let Some(branch) = current.filter(|branch| branch.has_upstream) {
        if branch.behind > 0 {
            chips.push((
                Standing::Behind {
                    count: branch.behind,
                    upstream: branch.upstream.clone().unwrap_or_default(),
                },
                Chip::Behind,
            ));
        }
        if branch.ahead > 0 {
            chips.push((
                Standing::Ahead {
                    count: branch.ahead,
                },
                Chip::Ahead,
            ));
        }
    }
    let changed = state.actions.local.as_ref().map_or_else(
        || state.current_worktree().map_or(0, |worktree| worktree.wip),
        |local| local.unstaged.len() + local.staged.len(),
    );
    if changed > 0 {
        chips.push((
            Standing::Changes {
                count: u32::try_from(changed).unwrap_or(u32::MAX),
            },
            Chip::Changes,
        ));
    }
    if let Some(merge) = &state.actions.merge {
        let open = merge.open_count();
        if open > 0 {
            chips.push((
                Standing::Conflicts {
                    count: u32::try_from(open).unwrap_or(u32::MAX),
                },
                Chip::Conflicts,
            ));
        }
    }
    let in_sync =
        current.is_some_and(|branch| branch.has_upstream && branch.ahead + branch.behind == 0);
    if in_sync && chips.is_empty() {
        chips.push((Standing::UpToDate, Chip::Behind));
    }
    chips
}

fn picker_view<'a>(state: &'a State, ctx: ViewCtx<'a>, kind: PickerKind) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let Some(open) = &state.picker else {
        return Space::new().into();
    };
    let placeholder = match kind {
        PickerKind::Repository => Msg::GitViewPickerSearchRepository,
        PickerKind::Branch => Msg::GitViewPickerSearchBranch,
    };
    let search = text_field(
        lang.text(placeholder),
        &open.query,
        PICKER_ID,
        Some(Message::PickerQuery),
        Message::PickerSubmit,
        palette,
    );
    let rows = picker::visible(state);
    let listed = rows
        .iter()
        .filter(|entry| entry.choice != Choice::NewBranch)
        .count();
    let start = window_start(listed, open.cursor.min(listed.saturating_sub(1)));
    let mut lines: Vec<PickerLine<Message>> = Vec::new();
    let mut group: Option<Msg> = None;
    for (place, entry) in rows.iter().enumerate().filter(|(place, entry)| {
        entry.choice == Choice::NewBranch || (*place >= start && *place < start + OPEN_LIST_ROWS)
    }) {
        let wanted = match entry.choice {
            Choice::Local(_) => Some(Msg::GitViewPickerGroupLocal),
            Choice::Remote(_) => Some(Msg::GitViewPickerGroupRemote),
            Choice::Repository(_) | Choice::NewBranch => None,
        };
        if entry.choice == Choice::NewBranch {
            lines.push(PickerLine::Rule);
        } else if wanted.is_some() && wanted != group {
            lines.push(PickerLine::Group(
                lang.text(wanted.unwrap_or(Msg::GitViewPickerGroupLocal))
                    .to_owned(),
            ));
        }
        group = wanted;
        lines.push(PickerLine::Row(picker_row(
            entry,
            place == open.cursor,
            ctx,
        )));
    }
    let enter = chord_label(
        iced::keyboard::Modifiers::empty(),
        &iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
    );
    let stash_enter = chord_label(
        iced::keyboard::Modifiers::COMMAND,
        &iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
    );
    let hints: Vec<(&str, &str)> = match kind {
        PickerKind::Repository => vec![
            ("\u{2191}\u{2193}", lang.text(Msg::GitViewPickerHintMove)),
            (&enter, lang.text(Msg::GitViewPickerHintOpen)),
            ("Esc", lang.text(Msg::GitViewPickerHintClose)),
        ],
        PickerKind::Branch => vec![
            ("\u{2191}\u{2193}", lang.text(Msg::GitViewPickerHintMove)),
            (&enter, lang.text(Msg::GitViewPickerHintSwitch)),
            (&stash_enter, lang.text(Msg::GitViewPickerHintStash)),
            ("Esc", lang.text(Msg::GitViewPickerHintClose)),
        ],
    };
    picker_list(search, lines, &hints, PICKER_WIDTH, palette)
}

fn picker_row(entry: &Entry, highlighted: bool, ctx: ViewCtx<'_>) -> PickerRow<Message> {
    let is_new = entry.choice == Choice::NewBranch;
    PickerRow {
        mark: if is_new {
            PickerMark::Action
        } else if entry.current {
            PickerMark::Current
        } else {
            PickerMark::None
        },
        label: if is_new {
            ctx.lang.text(Msg::GitViewPickerNewBranch).to_owned()
        } else {
            entry.label.clone()
        },
        trail: entry.trail.clone(),
        key: is_new
            .then(|| Shortcut::NewBranch.label(ctx.keys))
            .filter(|key| !key.is_empty()),
        current: entry.current,
        highlighted,
        on_press: Message::PickerPressed(entry.id),
    }
}
