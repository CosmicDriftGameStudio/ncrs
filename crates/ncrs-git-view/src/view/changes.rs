//! The local changes: the files not staged and staged with the commit message
//! on the right, the diff of the chosen file with a button per hunk in the middle.

use iced::widget::{column, container, row, Column, Space};
use iced::{alignment, Element, Length};
use ncrs_git::{ChangeKind, ChangedFile, RowKind};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::action_rows::{action_row, hunk_bar, section_bar, ActionRow};
use ncrs_ui_kit::git_rows::{diff_unified_line, DiffTone};
use ncrs_ui_kit::kit::{
    icon_button, key_button, labelled_checkbox, text_area, text_field, Check, Icon, Tone,
};
use ncrs_ui_kit::page_scroll::page_scroll;
use ncrs_ui_kit::page_text::page_line;
use ncrs_ui_kit::theme::{self, font_size, spacing};

use crate::actions::{ChangeSide, LocalChanges};
use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::shortcuts::Shortcut;

use super::toolbar::tip;

const TITLE_ID: &str = "git-commit-title";
const BODY_ID: &str = "git-commit-body";
const HUNKS_ID: &str = "git-local-diff";
/// Height of the field for the longer commit text.
const BODY_HEIGHT: f32 = 84.0;

fn kind_letter(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "A",
        ChangeKind::Modified => "M",
        ChangeKind::Deleted => "D",
        ChangeKind::Renamed => "R",
    }
}

fn file_rows<'a>(
    files: &'a [ChangedFile],
    side: ChangeSide,
    local: &'a LocalChanges,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    Column::with_children(files.iter().enumerate().map(|(index, file)| {
        let actions = match side {
            ChangeSide::Unstaged => vec![
                icon_button(
                    Icon::Plus,
                    None,
                    lang.text(Msg::GitViewStage),
                    Some(Message::Stage(index)),
                    Tone::Secondary,
                    palette,
                ),
                icon_button(
                    Icon::Trash,
                    None,
                    lang.text(Msg::GitViewDiscardFile),
                    Some(Message::DiscardFile(index)),
                    Tone::Secondary,
                    palette,
                ),
            ],
            ChangeSide::Staged => vec![icon_button(
                Icon::Minus,
                None,
                lang.text(Msg::GitViewUnstage),
                Some(Message::Unstage(index)),
                Tone::Secondary,
                palette,
            )],
        };
        action_row(
            ActionRow {
                lead: Some((kind_letter(file.kind).to_owned(), palette.text)),
                label: file.path.clone(),
                trail: vec![
                    (format!("+{}", file.added), palette.success),
                    (format!("\u{2212}{}", file.removed), palette.error),
                ],
                actions,
                chosen: local.selected == Some((side, index)),
                on_press: Message::SelectChange(side, index),
            },
            palette,
        )
    }))
    .into()
}

fn tone_of(kind: RowKind) -> DiffTone {
    match kind {
        RowKind::Context => DiffTone::Context,
        RowKind::Added => DiffTone::Added,
        RowKind::Removed => DiffTone::Removed,
    }
}

fn hunks<'a>(local: &'a LocalChanges, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let Some(diff) = &local.diff else {
        return Space::new().into();
    };
    let staged = matches!(local.selected, Some((ChangeSide::Staged, _)));
    let mut rows = Column::new().width(Length::Fill);
    rows = rows.push(hunk_bar(diff.path.clone(), Vec::new(), palette));
    for (index, hunk) in diff.hunks.iter().enumerate() {
        let actions = if staged {
            vec![icon_button(
                Icon::Minus,
                None,
                ctx.lang.text(Msg::GitViewHunkUnstage),
                Some(Message::UnstageHunk(index)),
                Tone::Secondary,
                palette,
            )]
        } else {
            vec![
                icon_button(
                    Icon::Plus,
                    None,
                    ctx.lang.text(Msg::GitViewHunkStage),
                    Some(Message::StageHunk(index)),
                    Tone::Secondary,
                    palette,
                ),
                icon_button(
                    Icon::Trash,
                    None,
                    ctx.lang.text(Msg::GitViewHunkDiscard),
                    Some(Message::DiscardHunk(index)),
                    Tone::Danger,
                    palette,
                ),
            ]
        };
        rows = rows.push(hunk_bar(
            format!("@@ -{} +{} @@", hunk.old_start, hunk.new_start),
            actions,
            palette,
        ));
        for line in &hunk.rows {
            rows = rows.push(diff_unified_line(
                line.old_line,
                line.new_line,
                line.text.clone(),
                tone_of(line.kind),
                palette,
            ));
        }
    }
    page_scroll(HUNKS_ID, rows.into())
}

fn commit_form<'a>(local: &'a LocalChanges, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let staged = local.staged.len();
    let label = match staged {
        1 => ctx.lang.text(Msg::GitViewCommitOne).to_owned(),
        _ => fill_template(
            ctx.lang.text(Msg::GitViewCommitMany),
            &[("{count}", &staged.to_string())],
        ),
    };
    let can_commit = staged > 0 || local.amend;
    column![
        text_field(
            ctx.lang.text(Msg::GitViewCommitTitlePlaceholder),
            &local.title,
            TITLE_ID,
            Some(Message::CommitTitleChanged),
            Message::Commit,
            palette,
        ),
        page_line(
            ctx.lang.text(Msg::GitViewCommitBodyLabel),
            font_size::SMALL,
            palette.dim_text
        ),
        container(text_area(
            &local.body,
            BODY_ID,
            Message::CommitBodyEdited,
            palette
        ))
        .height(BODY_HEIGHT),
        row![
            labelled_checkbox(
                if local.amend { Check::On } else { Check::Off },
                Some(Message::ToggleAmend),
                Some(ctx.lang.text(Msg::GitViewCommitAmend)),
                palette,
            ),
            Space::new().width(Length::Fill),
            key_button(
                &label,
                &Shortcut::Commit.label(ctx.keys),
                can_commit.then_some(Message::Commit),
                Tone::Primary,
                palette
            ),
        ]
        .align_y(alignment::Vertical::Center),
    ]
    .spacing(spacing::M)
    .padding(spacing::M)
    .into()
}

/// The middle: the diff of the chosen file.
pub fn diff_view<'a>(local: &'a LocalChanges, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    hunks(local, ctx)
}

/// The right side: the files and the commit message.
pub fn view<'a>(local: &'a LocalChanges, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let rule = || theme::horizontal_rule(palette.border_inactive);
    column![
        section_bar(
            lang.text(Msg::GitViewChangesUnstaged).to_owned(),
            local.unstaged.len(),
            vec![
                icon_button(
                    Icon::Trash,
                    None,
                    &tip(
                        lang.text(Msg::GitViewDiscardAll),
                        Shortcut::DiscardAll,
                        ctx.keys
                    ),
                    (!local.unstaged.is_empty()).then_some(Message::DiscardAll),
                    Tone::Secondary,
                    palette,
                ),
                icon_button(
                    Icon::Plus,
                    None,
                    &tip(
                        lang.text(Msg::GitViewStageAll),
                        Shortcut::StageAll,
                        ctx.keys
                    ),
                    (!local.unstaged.is_empty()).then_some(Message::StageAll),
                    Tone::Secondary,
                    palette,
                ),
            ],
            palette,
        ),
        file_rows(&local.unstaged, ChangeSide::Unstaged, local, ctx),
        rule(),
        section_bar(
            lang.text(Msg::GitViewChangesStaged).to_owned(),
            local.staged.len(),
            vec![icon_button(
                Icon::Minus,
                None,
                &tip(
                    lang.text(Msg::GitViewUnstageAll),
                    Shortcut::UnstageAll,
                    ctx.keys
                ),
                (!local.staged.is_empty()).then_some(Message::UnstageAll),
                Tone::Secondary,
                palette,
            )],
            palette,
        ),
        file_rows(&local.staged, ChangeSide::Staged, local, ctx),
        rule(),
        Space::new().height(Length::Fill),
        rule(),
        commit_form(local, ctx),
    ]
    .height(Length::Fill)
    .into()
}
