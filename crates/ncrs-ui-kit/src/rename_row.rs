//! A file row whose name is being edited: the name is an input, size and date
//! stay as they were, and why the name is refused shows under it.

use iced::widget::{column, container, row, text, text_input, Space};
use iced::{alignment, Background, Border, Element, Length};
use ncrs_core::FileEntry;

use crate::format;
use crate::layout::{DATE_COLUMN_WIDTH, SIZE_COLUMN_WIDTH, TAG_COLUMN_WIDTH};
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
        on_input,
        on_submit,
    } = rename;
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
    let cell = |content: String, width: f32| {
        text(content)
            .size(font_size::FILE)
            .font(crate::APP_FONT)
            .color(colors.text)
            .width(width)
            .align_x(alignment::Horizontal::Right)
            .wrapping(text::Wrapping::None)
    };
    let line = container(
        row![
            Space::new().width(TAG_COLUMN_WIDTH),
            input,
            cell(format::entry_size(entry), SIZE_COLUMN_WIDTH),
            cell(format::time(entry.modified), DATE_COLUMN_WIDTH),
        ]
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
