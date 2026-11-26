//! What the two toolbar rows of a diff share, for a local change and for a commit.

use iced::widget::{container, row};
use iced::{alignment, Element, Length};
use ncrs_git::ChangeKind;
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::kit::{icon_button, key_button, labelled_checkbox, segmented, Check, Icon, Tone};
use ncrs_ui_kit::page_text::page_line;
use ncrs_ui_kit::theme::{font_size, size, spacing};

use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::shortcuts::Shortcut;

/// Below this width of the middle the toolbar drops what the buttons already say.
pub const TOOLBAR_NARROW: f32 = 1000.0;

pub fn kind_letter(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "A",
        ChangeKind::Modified => "M",
        ChangeKind::Deleted => "D",
        ChangeKind::Renamed => "R",
    }
}

/// One line of the toolbar: 32 px, no wrapping.
pub fn bar<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    container(content)
        .padding([0.0, spacing::M])
        .height(size::COMPARE_BAR)
        .center_y(size::COMPARE_BAR)
        .clip(true)
        .into()
}

/// Side by side or unified.
pub fn layout_choice<'a>(unified: bool, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    segmented(
        &[
            (
                ctx.lang.text(Msg::GitViewDiffSideBySide),
                Message::SetUnified(false),
            ),
            (
                ctx.lang.text(Msg::GitViewDiffUnified),
                Message::SetUnified(true),
            ),
        ],
        usize::from(unified),
        true,
        ctx.palette,
    )
}

pub fn whitespace_choice<'a>(ignored: bool, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    labelled_checkbox(
        if ignored { Check::On } else { Check::Off },
        Some(Message::ToggleIgnoreWhitespace),
        Some(ctx.lang.text(Msg::GitViewDiffIgnoreWhitespace)),
        ctx.palette,
    )
}

/// "↑ ↓ Change x of y": which change of the file has the keys.
pub fn change_stepper<'a>(
    hunk: usize,
    total: usize,
    narrow: bool,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let current = hunk.min(total.saturating_sub(1)) + usize::from(total > 0);
    row![
        icon_button(
            Icon::ArrowUp,
            None,
            lang.text(Msg::GitViewDiffPreviousChange),
            (current > 1).then_some(Message::PreviousHunk),
            Tone::Secondary,
            palette,
        ),
        icon_button(
            Icon::ArrowDown,
            None,
            lang.text(Msg::GitViewDiffNextChange),
            (current < total).then_some(Message::NextHunk),
            Tone::Secondary,
            palette,
        ),
    ]
    .extend((!narrow).then(|| {
        page_line(
            fill_template(
                lang.text(Msg::GitViewDiffChangeOf),
                &[
                    ("{current}", &current.to_string()),
                    ("{total}", &total.to_string()),
                ],
            ),
            font_size::SMALL,
            palette.dim_text,
        )
    }))
    .spacing(spacing::XS)
    .align_y(alignment::Vertical::Center)
    .into()
}

/// Which of the buttons of the second row work on the file.
#[derive(Debug, Clone, Copy)]
pub struct FileButtons {
    /// Git has commits of the file.
    pub committed: bool,
    /// The folder has the file.
    pub on_disk: bool,
}

/// The second row: the history and the blame of the file, what is compared
/// with what, and the way to the editor.
pub fn file_row<'a>(buttons: FileButtons, sides: String, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let button = |label: Msg, shortcut: Shortcut, message: Option<Message>| {
        key_button(
            ctx.lang.text(label),
            &shortcut.label(ctx.keys),
            message,
            Tone::Secondary,
            palette,
        )
    };
    bar(row![
        button(
            Msg::GitViewHistory,
            Shortcut::FileHistory,
            buttons.committed.then_some(Message::HistoryKey),
        ),
        button(
            Msg::GitViewBlame,
            Shortcut::Blame,
            buttons.committed.then_some(Message::BlameKey),
        ),
        container(page_line(sides, font_size::SMALL, palette.dim_text))
            .width(Length::Fill)
            .clip(true),
        button(
            Msg::GitViewOpenEditor,
            Shortcut::OpenEditor,
            buttons.on_disk.then_some(Message::OpenSelectedInEditor),
        ),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center)
    .into())
}
