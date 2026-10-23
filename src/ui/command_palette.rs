//! The command palette box: a text field and the actions that match, each
//! with its key on the right. Pure view over [`CommandPalette`].

use iced::widget::{container, row, text};
use iced::{alignment, Element, Length};

use super::picker;
use super::theme::font_size;
use crate::command_palette::{CommandPalette, VISIBLE_ENTRIES};
use crate::i18n::{Language, Msg};
use crate::messages::{CommandPaletteMessage, Message};
use crate::palette::Palette;

pub const COMMAND_PALETTE_ID: &str = "command-palette-query";

fn to_palette(message: CommandPaletteMessage) -> Message {
    Message::CommandPalette(message)
}

pub fn view<'a>(
    command_palette: &'a CommandPalette,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let field = picker::query_field(
        lang.text(Msg::CommandPalettePlaceholder),
        command_palette.query(),
        COMMAND_PALETTE_ID,
        |text| to_palette(CommandPaletteMessage::QueryChanged(text)),
        to_palette(CommandPaletteMessage::Run),
        palette,
    );

    let visible = command_palette.visible();
    let rows = command_palette
        .hits()
        .iter()
        .enumerate()
        .skip(visible.start)
        .take(visible.len())
        .filter_map(|(index, hit)| {
            let entry = command_palette.entry(hit)?;
            let selected = index == command_palette.cursor();
            let shortcut_color = if selected {
                palette.cursor_text
            } else {
                palette.dim_text
            };
            let content = row![
                container(picker::highlighted(
                    entry.label,
                    &hit.highlight,
                    selected,
                    palette
                ))
                .width(Length::Fill),
                text(entry.shortcut.as_deref().unwrap_or_default())
                    .size(font_size::ROW)
                    .color(shortcut_color),
            ]
            .align_y(alignment::Vertical::Center);
            Some(picker::list_row(
                content.into(),
                to_palette(CommandPaletteMessage::Select(index)),
                selected,
                palette,
            ))
        });

    let status = if command_palette.hits().is_empty() {
        lang.text(Msg::CommandPaletteNoMatch)
    } else {
        ""
    };
    picker::frame(
        lang.text(Msg::CommandPaletteTitle).to_owned(),
        field,
        status.to_owned(),
        rows,
        VISIBLE_ENTRIES,
        palette,
    )
}
