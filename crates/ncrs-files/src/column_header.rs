//! The column titles above the rows of a file list.
use iced::widget::{column, container, mouse_area, row, text};
use iced::{alignment, Element, Length};
use ncrs_fs::{SortColumn, SortKey};
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::layout::{
    COLUMN_HEADER_HEIGHT, DATE_COLUMN_WIDTH, HERMES_CELL_GAP, HERMES_COLUMN_WIDTH,
    SIZE_COLUMN_WIDTH, TAG_COLUMN_WIDTH,
};
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::theme::{self, font_size, spacing};

/// The column titles, each a click target that sorts by its column; the sorted
/// one carries an arrow for the direction.
pub fn column_header<'a, M: Clone + 'a>(
    lang: Language,
    palette: &Palette,
    sort: Option<SortKey>,
    with_hermes: bool,
    on_sort: impl Fn(SortColumn) -> Option<M> + 'a,
) -> Element<'a, M> {
    let title = |column: SortColumn, msg: Msg| {
        let arrow = match sort.filter(|key| key.column == column) {
            None => "",
            Some(key) if key.descending => " ▼",
            Some(_) => " ▲",
        };
        let label = mouse_area(
            text(format!("{}{arrow}", lang.text(msg)))
                .size(font_size::SMALL)
                .color(palette.dim_text)
                .wrapping(text::Wrapping::None),
        );
        match on_sort(column) {
            Some(pressed) => label
                .interaction(iced::mouse::Interaction::Pointer)
                .on_press(pressed),
            None => label,
        }
    };
    column![
        container(
            row![
                container(title(SortColumn::Name, Msg::ColumnName))
                    .padding(iced::Padding::ZERO.left(TAG_COLUMN_WIDTH))
                    .width(Length::Fill),
                container(title(SortColumn::Size, Msg::ColumnSize))
                    .width(SIZE_COLUMN_WIDTH)
                    .align_x(alignment::Horizontal::Right),
                container(title(SortColumn::Modified, Msg::ColumnModified))
                    .width(DATE_COLUMN_WIDTH)
                    .align_x(alignment::Horizontal::Right),
            ]
            .extend(with_hermes.then(|| {
                container(
                    text(lang.text(Msg::ColumnHermes))
                        .size(font_size::SMALL)
                        .color(palette.dim_text)
                        .wrapping(text::Wrapping::None),
                )
                .padding(iced::Padding::ZERO.left(HERMES_CELL_GAP))
                .width(HERMES_COLUMN_WIDTH)
                .into()
            }))
            .align_y(alignment::Vertical::Center),
        )
        .padding([0.0, spacing::CELL_PADDING_X])
        .height(Length::Fill)
        .align_y(alignment::Vertical::Center)
        .width(Length::Fill),
        theme::horizontal_rule(palette.border_inactive),
    ]
    .height(COLUMN_HEADER_HEIGHT)
    .width(Length::Fill)
    .into()
}

/// The column titles of a list that a click does not sort.
pub fn plain_column_header<'a, M: Clone + 'a>(lang: Language, palette: &Palette) -> Element<'a, M> {
    column_header(lang, palette, None, false, |_| None)
}
