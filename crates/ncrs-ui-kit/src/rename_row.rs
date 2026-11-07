//! A file row whose name is being edited: the name is an input, the other
//! columns stay as they were, and why the name is refused shows under it.

use iced::widget::{column, container, text, text_input, Row, Space};
use iced::{alignment, Background, Border, Element, Length};
use ncrs_core::{Column, FileEntry};

use crate::column_header::{column_alignment, ColumnLayout};
use crate::file_icon::{file_icon, icon_color, icon_kind, SpecialFolder};
use crate::file_row::cell_text;
use crate::layout::{FILE_ICON_SIZE, TAG_COLUMN_WIDTH};
use crate::palette::Palette;
use crate::theme::{self, font_size, spacing};

/// The id of the name input, for the operations that focus it and select in it.
pub const RENAME_INPUT_ID: &str = "rename-input";

/// What the row shows and sends.
pub struct RenameRow<'a, M> {
    pub entry: &'a FileEntry,
    pub draft: &'a str,
    /// Why the draft is refused, shown under the input.
    pub error: Option<&'a str>,
    /// The rename is running: the input takes no more typing.
    pub busy: bool,
    pub height: f32,
    /// The columns of the panel, so the cells line up with the other rows.
    pub columns: ColumnLayout<'a>,
    pub special: Option<SpecialFolder>,
    pub on_input: fn(String) -> M,
    pub on_submit: M,
}

impl<M> std::fmt::Debug for RenameRow<'_, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenameRow").finish_non_exhaustive()
    }
}

/// The row of the cursor entry while it is renamed.
pub fn rename_row<'a, M: Clone + 'a>(
    rename: RenameRow<'a, M>,
    palette: &Palette,
) -> Element<'a, M> {
    let RenameRow {
        entry,
        draft,
        error,
        busy,
        height,
        columns,
        special,
        on_input,
        on_submit,
    } = rename;
    let kind = icon_kind(entry, special);
    let colors = *palette;
    let mut input = text_input("", draft)
        .id(RENAME_INPUT_ID)
        .size(font_size::FILE)
        .font(crate::APP_FONT)
        .padding([0.0, spacing::XS])
        .width(Length::Fill)
        .style(theme::search_input(palette));
    if !busy {
        input = input.on_input(on_input).on_submit(on_submit);
    }
    let mut input = Some(input);
    let cells: Vec<Element<'a, M>> = columns
        .shown()
        .map(|shown| match shown {
            Column::Name => input
                .take()
                .map_or_else(|| Space::new().into(), Element::from),
            Column::Icon => container(file_icon(
                kind,
                icon_color(kind, palette, colors.text, true),
                FILE_ICON_SIZE,
            ))
            .width(columns.width_of(Column::Icon))
            .into(),
            other => container(
                text(cell_text(entry, other, &columns, kind))
                    .size(font_size::FILE)
                    .font(crate::APP_FONT)
                    .color(colors.text)
                    .width(Length::Fill)
                    .align_x(column_alignment(other))
                    .wrapping(text::Wrapping::None),
            )
            .width(columns.width_of(other))
            .clip(true)
            .into(),
        })
        .collect();
    let line = container(
        Row::new()
            .push(Space::new().width(TAG_COLUMN_WIDTH))
            .extend(cells)
            .align_y(alignment::Vertical::Center),
    )
    .padding([0.0, spacing::CELL_PADDING_X])
    .height(height)
    .width(Length::Fill)
    .align_y(alignment::Vertical::Center)
    .style(move |_theme| iced::widget::container::Style {
        background: Some(Background::Color(colors.cursor)),
        border: Border::default(),
        ..iced::widget::container::Style::default()
    });
    match error {
        Some(reason) => column![
            line,
            container(
                text(reason.to_owned())
                    .size(font_size::SMALL)
                    .font(crate::APP_FONT)
                    .color(colors.error)
                    .wrapping(text::Wrapping::None),
            )
            .padding(iced::Padding::ZERO.left(TAG_COLUMN_WIDTH + spacing::CELL_PADDING_X))
            .width(Length::Fill)
            .clip(true),
        ]
        .into(),
        None => line.into(),
    }
}
