//! The command palette box: a text field and the actions that match, each
//! with its key on the right. Pure view over [`CommandPalette`].

use iced::widget::text::Wrapping;
use iced::widget::{container, row, text};
use iced::{alignment, Element, Length, Size};

use super::frame;
use super::kit;
use super::layout::ROW_HEIGHT;
use super::picker;
use super::theme::{font_size, spacing};
use crate::command_palette::{CommandPalette, Entry, Header, Hit, Row, MIN_VISIBLE_ROWS};
use crate::i18n::{Language, Msg};
use crate::messages::{CommandPaletteMessage, Message};
use crate::palette::Palette;

pub const COMMAND_PALETTE_ID: &str = "command-palette-query";

fn to_palette(message: CommandPaletteMessage) -> Message {
    Message::CommandPalette(message)
}

/// What the frame, the field and the status line take of the frame's height,
/// measured; the rest is the list.
const LIST_CHROME: f32 = 170.0;

/// The rows the list shows in a window of `window`: as many as the frame has
/// room for, never fewer than [`MIN_VISIBLE_ROWS`].
pub fn visible_rows(window: Size) -> usize {
    let room = frame::frame_size(picker::SIZE, window).height - LIST_CHROME;
    ((room / ROW_HEIGHT).floor().max(0.0) as usize).max(MIN_VISIBLE_ROWS)
}

/// Room kept at the right of a row for the key of the action.
const SHORTCUT_COLUMN: f32 = 110.0;

pub fn view<'a>(
    command_palette: &'a CommandPalette,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let field = kit::text_field(
        lang.text(Msg::CommandPalettePlaceholder),
        command_palette.query(),
        COMMAND_PALETTE_ID,
        |text| to_palette(CommandPaletteMessage::QueryChanged(text)),
        to_palette(CommandPaletteMessage::Run),
        palette,
    );

    let visible = command_palette.visible();
    let rows = command_palette
        .rows()
        .iter()
        .enumerate()
        .skip(visible.start)
        .take(visible.len())
        .filter_map(|(index, row)| match row {
            Row::Header(header) => Some(header_row(header, palette)),
            Row::Hit(hit) => {
                let entry = command_palette.entry(hit)?;
                let selected = index == command_palette.cursor();
                Some(kit::list_row(
                    command_row(entry, hit, selected, window, palette),
                    to_palette(CommandPaletteMessage::Select(index)),
                    selected,
                    palette,
                ))
            }
        });

    let status = if command_palette.hits().next().is_none() {
        lang.text(Msg::CommandPaletteNoMatch)
    } else {
        ""
    };
    picker::frame(
        picker::Content {
            title: lang.text(Msg::ActionCommandPalette).to_owned(),
            title_theme: Some(lang.text(Msg::ActionCommandPaletteTheme).to_owned()),
            subtitle: None,
            field,
            status: status.to_owned(),
            rows: rows.collect(),
            row_count: command_palette.visible_rows(),
            on_close: to_palette(CommandPaletteMessage::Close),
        },
        window,
        lang,
        palette,
    )
}

/// Characters kept apart between the label and the theme name.
const NAME_GAP: usize = 2;

/// Fewer characters than this are not worth showing of a theme name.
const MIN_THEME_CHARS: usize = 4;

/// The plain label, the theme name dimmed after it, and the key at the right,
/// on one line.
fn command_row<'a>(
    entry: &'a Entry,
    hit: &Hit,
    selected: bool,
    window: Size,
    palette: &Palette,
) -> Element<'a, Message> {
    let shortcut_color = if selected {
        palette.cursor_text
    } else {
        palette.dim_text
    };
    let room = picker::row_chars(window, SHORTCUT_COLUMN);
    let label_chars = entry.label.chars().count().min(room);
    let theme_room = room.saturating_sub(label_chars + NAME_GAP);
    let theme_text = entry
        .theme_name
        .filter(|_| theme_room >= MIN_THEME_CHARS)
        .map(|name| {
            picker::highlighted_dim(name, &hit.theme_highlight, selected, theme_room, palette)
        });
    let names = row![picker::highlighted(
        entry.label,
        &hit.label_highlight,
        selected,
        room,
        palette
    )]
    .extend(theme_text)
    .spacing(spacing::SECTION_GAP);
    row![
        container(names).width(Length::Fill),
        text(entry.shortcut.as_deref().unwrap_or_default())
            .size(font_size::ROW)
            .color(shortcut_color)
            .wrapping(Wrapping::None),
    ]
    .align_y(alignment::Vertical::Center)
    .into()
}

/// A group heading, which is not a place for the cursor.
fn header_row<'a>(header: &Header, palette: &Palette) -> Element<'a, Message> {
    let mut line = row![text(header.label.to_uppercase())
        .size(font_size::COLUMN_HEADER)
        .color(palette.dim_text)
        .wrapping(Wrapping::None)]
    .spacing(spacing::SECTION_GAP)
    .align_y(alignment::Vertical::Center);
    if let Some(theme_name) = header.theme_name {
        line = line.push(
            text(theme_name.to_owned())
                .size(font_size::COLUMN_HEADER)
                .color(palette.dim_text)
                .wrapping(Wrapping::None),
        );
    }
    container(line)
        .height(ROW_HEIGHT)
        .padding([0.0, spacing::CELL_PADDING_X])
        .align_y(alignment::Vertical::Center)
        .into()
}
