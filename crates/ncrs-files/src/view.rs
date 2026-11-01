//! The view of a file panel: a pure function of [`PanelState`]. The component
//! owns no state; it lives in the app state and is only mutated by `update`.

use iced::widget::{column, container, mouse_area, Column, Row};
use iced::{Element, Length};
use ncrs_ui_kit::file_row::{file_row, tag_star, RowClick, RowExtras};
use ncrs_ui_kit::theme;

use crate::column_header::column_header;
use crate::ctx::ViewCtx;
use crate::message::PanelMessage;
use crate::panel::PanelState;

/// Renders one panel.
///
/// A row is one clickable thing with several meanings, so it sends one message
/// with what the click did. A right click sends [`PanelMessage::RowContext`].
pub fn view<'a>(state: &'a PanelState, ctx: ViewCtx<'a>) -> Element<'a, PanelMessage> {
    let side = ctx.side;
    let lang = ctx.lang;
    let palette = &ctx.palette;
    let hermes_cell = ctx.hermes_cell.as_deref();
    let on_row_click = move |index, click| match click {
        RowClick::Open => PanelMessage::RowActivated { side, index },
        RowClick::Tag | RowClick::Cursor => PanelMessage::RowClicked {
            side,
            index,
            on_tag: click == RowClick::Tag,
        },
    };
    let rows = state
        .entries
        .iter()
        .enumerate()
        .skip(state.scroll_offset)
        .take(state.rows_in_view(ctx.visible_rows))
        .map(|(index, entry)| {
            let mark = state
                .selection
                .state_of(index, &state.entries, state.selected);
            // Two targets per row, as in NC: the star itself toggles the tag,
            // the rest of the row moves the cursor. iced gives a button one
            // message and no click position, so the star has to be its own
            // button rather than a region of the row button.
            let row = Row::new().push(tag_star(
                mark,
                ctx.is_active,
                palette,
                state.row_height(),
                on_row_click(index, RowClick::Tag),
            ));
            // A button takes only the left button, so the right press reaches
            // the area around it.
            mouse_area(file_row(
                entry,
                mark,
                ctx.is_active,
                palette,
                |click| on_row_click(index, click),
                row,
                RowExtras {
                    third_line: None,
                    hermes: hermes_cell.map(|cell| cell(entry)),
                    hit_scope: state
                        .hit_list
                        .as_ref()
                        .map(|list| (list.scope.as_path(), state.row_height())),
                },
            ))
            .on_right_press(PanelMessage::RowContext { side, index })
            .into()
        });

    // The wheel belongs to the panel under the pointer. The rows are drawn from
    // the window `scroll_offset` selects, not by a `scrollable`, so the panel
    // has to ask for the wheel itself.
    mouse_area(
        column![
            column_header(
                lang,
                palette,
                Some(state.sort),
                hermes_cell.is_some(),
                move |column| Some(PanelMessage::SortBy { side, column }),
            ),
            Column::with_children(rows).height(Length::Fill),
        ]
        .apply_frame(palette),
    )
    .on_scroll(move |delta| PanelMessage::PanelScrolled { side, delta })
    .on_enter(PanelMessage::PanelEntered(side))
    .on_exit(PanelMessage::PanelExited(side))
    .into()
}

/// Small extension to wrap the panel content in its styled frame.
trait ApplyFrame<'a, M> {
    fn apply_frame(self, palette: &ncrs_ui_kit::palette::Palette) -> Element<'a, M>;
}

impl<'a, M: 'a> ApplyFrame<'a, M> for Column<'a, M> {
    fn apply_frame(self, palette: &ncrs_ui_kit::palette::Palette) -> Element<'a, M> {
        container(self)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::panel(palette))
            .into()
    }
}
