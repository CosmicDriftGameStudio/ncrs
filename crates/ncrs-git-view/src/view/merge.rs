//! A merge that stopped at conflicts: the banner, the files in conflict on the
//! right and the chosen file in the middle, its markers shown as readable
//! lines with the choice for each block.

use iced::widget::Column;
use iced::{Element, Length};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::action_rows::{action_row, banner, context_area, hunk_bar, ActionRow, BannerTone};
use ncrs_ui_kit::git_rows::{diff_unified_line, DiffTone};
use ncrs_ui_kit::kit::{button, icon_button, Icon, Tone};
use ncrs_ui_kit::page_scroll::page_scroll;

use crate::actions::{Conflict, ConflictLine, MenuTarget, MergeInProgress, Side};
use crate::ctx::ViewCtx;
use crate::message::Message;

const PREVIEW_ID: &str = "git-conflict-diff";

pub fn banner_view<'a>(merge: &'a MergeInProgress, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let lang = ctx.lang;
    let open = merge.open_count();
    let headline = match open {
        0 => fill_template(
            lang.text(Msg::GitViewMergeBannerDone),
            &[("{from}", &merge.from), ("{into}", &merge.into)],
        ),
        1 => fill_template(
            lang.text(Msg::GitViewMergeBannerOne),
            &[("{from}", &merge.from), ("{into}", &merge.into)],
        ),
        count => fill_template(
            lang.text(Msg::GitViewMergeBanner),
            &[
                ("{from}", &merge.from),
                ("{into}", &merge.into),
                ("{count}", &count.to_string()),
            ],
        ),
    };
    banner(
        BannerTone::Attention,
        headline,
        Some(lang.text(Msg::GitViewMergeHint).to_owned()),
        None,
        vec![
            button(
                lang.text(Msg::GitViewMergeAbort),
                Some(Message::MergeAbort),
                Tone::Secondary,
                ctx.palette,
            ),
            button(
                lang.text(Msg::GitViewMergeContinue),
                (open == 0).then_some(Message::MergeContinue),
                Tone::Primary,
                ctx.palette,
            ),
        ],
        ctx.palette,
    )
}

fn conflict_row<'a>(
    index: usize,
    conflict: &'a Conflict,
    chosen: bool,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let state = if conflict.resolved {
        (
            lang.text(Msg::GitViewConflictResolved).to_owned(),
            palette.success,
        )
    } else if conflict.edited {
        (
            lang.text(Msg::GitViewConflictEdited).to_owned(),
            palette.dim_text,
        )
    } else if conflict.places == 1 {
        (
            lang.text(Msg::GitViewConflictPlaceOne).to_owned(),
            palette.error,
        )
    } else {
        (
            fill_template(
                lang.text(Msg::GitViewConflictPlaces),
                &[("{count}", &conflict.places.to_string())],
            ),
            palette.error,
        )
    };
    let row = action_row(
        ActionRow {
            lead: Some((
                "U".to_owned(),
                if conflict.resolved {
                    palette.success
                } else {
                    palette.error
                },
            )),
            label: conflict.path.clone(),
            trail: vec![state],
            actions: Vec::new(),
            chosen,
            on_press: Message::SelectConflict(index),
        },
        palette,
    );
    context_area(row, Message::OpenMenu(MenuTarget::Conflict(index)))
}

/// Mine, theirs or both for one block, as three small icons.
fn block_choice<'a>(block: usize, open: bool, ctx: ViewCtx<'a>) -> Vec<Element<'a, Message>> {
    let lang = ctx.lang;
    [
        (Icon::ArrowUp, Msg::GitViewConflictTakeMine, Side::Mine),
        (
            Icon::ArrowDown,
            Msg::GitViewConflictTakeTheirs,
            Side::Theirs,
        ),
        (Icon::ArrowUpDown, Msg::GitViewConflictTakeBoth, Side::Both),
    ]
    .into_iter()
    .map(|(icon, label, side)| {
        icon_button(
            icon,
            None,
            lang.text(label),
            open.then_some(Message::TakeBlock(block, side)),
            Tone::Secondary,
            ctx.palette,
        )
    })
    .collect()
}

/// The lines of the file; each block of mine and of theirs gets a line above
/// it that says whose it is, and the block ends with a line of its own.
pub fn diff_view<'a>(merge: &'a MergeInProgress, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let mut rows = Column::new().width(Length::Fill);
    let mut previous: Option<&ConflictLine> = None;
    let mut block = 0;
    let open = merge
        .conflicts
        .get(merge.selected)
        .is_some_and(|conflict| !conflict.resolved);
    for line in &merge.preview {
        let same_block = matches!(
            (previous, line),
            (Some(ConflictLine::Mine(_)), ConflictLine::Mine(_))
                | (Some(ConflictLine::Theirs(_)), ConflictLine::Theirs(_))
        );
        if matches!(previous, Some(ConflictLine::Theirs(_)))
            && !matches!(line, ConflictLine::Theirs(_))
        {
            rows = rows.push(hunk_bar(
                lang.text(Msg::GitViewConflictEnd).to_owned(),
                Vec::new(),
                palette,
            ));
        }
        if !same_block {
            let marker = match line {
                ConflictLine::Mine(_) => Some(fill_template(
                    lang.text(Msg::GitViewConflictMine),
                    &[("{branch}", &merge.into)],
                )),
                ConflictLine::Theirs(_) => Some(fill_template(
                    lang.text(Msg::GitViewConflictTheirs),
                    &[("{branch}", &merge.from)],
                )),
                ConflictLine::Shared(..) => None,
            };
            if let Some(content) = marker {
                let choice = if matches!(line, ConflictLine::Mine(_)) {
                    block += 1;
                    block_choice(block - 1, open, ctx)
                } else {
                    Vec::new()
                };
                rows = rows.push(hunk_bar(content, choice, palette));
            }
        }
        rows = rows.push(match line {
            ConflictLine::Shared(number, text) => diff_unified_line(
                Some(*number),
                Some(*number),
                text.clone(),
                DiffTone::Context,
                palette,
            ),
            ConflictLine::Mine(text) => {
                diff_unified_line(None, None, text.clone(), DiffTone::Mine, palette)
            }
            ConflictLine::Theirs(text) => {
                diff_unified_line(None, None, text.clone(), DiffTone::Theirs, palette)
            }
        });
        previous = Some(line);
    }
    if matches!(previous, Some(ConflictLine::Theirs(_))) {
        rows = rows.push(hunk_bar(
            lang.text(Msg::GitViewConflictEnd).to_owned(),
            Vec::new(),
            palette,
        ));
    }
    page_scroll(PREVIEW_ID, rows.into())
}

/// The right side: the files in conflict.
pub fn view<'a>(merge: &'a MergeInProgress, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    Column::with_children(
        merge
            .conflicts
            .iter()
            .enumerate()
            .map(|(index, conflict)| conflict_row(index, conflict, index == merge.selected, ctx)),
    )
    .height(Length::Fill)
    .into()
}
