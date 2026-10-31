//! The quick-fix menu of an inquiry: what Hermes read and why it asks on top,
//! the fixes below, laid over the window at the cursor row.

use iced::widget::{button, column, container, mouse_area, opaque, pin, row, text, Column};
use iced::{alignment, Element, Length};

use ncrs_index::Kinds;

use super::format;
use super::kit;
use super::layout::{MENU_ITEM_HEIGHT, MENU_PADDING, QUICK_FIX_HEADER_HEIGHT, QUICK_FIX_WIDTH};
use super::review_list::reason_text;
use super::theme::{self, font_size, spacing};
use crate::app::fill_template;
use crate::i18n::{Language, Msg};
use crate::messages::{Message, QuickFixMessage};
use crate::palette::Palette;
use crate::quick_fix::{Entry, QuickFix, QuickFixAction};
use crate::review_work::ReviewItem;

/// Width a target path may take beside its entry.
const DETAIL_WIDTH: f32 = 280.0;
const ESCAPE_KEY: &str = "Esc";
const MISSING_FIELD: &str = "?";

pub fn view<'a>(menu: &'a QuickFix, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let entries = menu.entries();
    let mut body = Column::new().push(header(menu, lang, palette));
    if entries.is_empty() {
        body = body.push(
            container(
                text(lang.text(Msg::QuickFixLoading))
                    .size(font_size::ROW)
                    .color(palette.dim_text),
            )
            .padding([0.0, spacing::CELL_PADDING_X])
            .height(MENU_ITEM_HEIGHT)
            .align_y(alignment::Vertical::Center),
        );
    }
    for (index, entry) in entries.iter().enumerate() {
        body = body.push(item(menu, entry, index, lang, palette));
    }
    let menu_box = container(body)
        .padding(MENU_PADDING)
        .width(QUICK_FIX_WIDTH)
        .style(theme::menu(palette));

    // `opaque` so a click on the menu's own padding is not taken for a click
    // beside it.
    pin(opaque(menu_box))
        .x(menu.origin().x)
        .y(menu.origin().y)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn header<'a>(menu: &QuickFix, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let (first, second) = header_lines(menu, lang);
    container(
        column![
            text(first)
                .size(font_size::ROW)
                .color(palette.text)
                .wrapping(text::Wrapping::None),
            text(second)
                .size(font_size::STATUS)
                .color(palette.dim_text)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(spacing::LINE_GAP),
    )
    .padding([0.0, spacing::CELL_PADDING_X])
    .height(QUICK_FIX_HEADER_HEIGHT)
    .align_y(alignment::Vertical::Center)
    .into()
}

/// The two lines above the entries; empty while the reviews load.
pub fn header_lines(menu: &QuickFix, lang: Language) -> (String, String) {
    if let Some(group) = menu.summarised_group() {
        return (
            fill_template(
                lang.text(Msg::QuickFixSummaryTitle),
                &[
                    ("{count}", &group.members.len().to_string()),
                    ("{directory}", &directory_text(&group.directory)),
                ],
            ),
            lang.text(Msg::QuickFixSummaryHint).to_owned(),
        );
    }
    match menu.items() {
        [] => (lang.text(Msg::QuickFixLoading).to_owned(), String::new()),
        [item] => (
            single_header(item, lang),
            read_line(item, menu.kinds(), lang),
        ),
        items => {
            let without = items
                .iter()
                .filter(|item| item.suggested_target(menu.kinds()).is_none())
                .count();
            (
                fill_template(
                    lang.text(Msg::QuickFixHeaderMany),
                    &[("{count}", &items.len().to_string())],
                ),
                fill_template(
                    lang.text(Msg::QuickFixHeaderManyDetail),
                    &[
                        ("{suggested}", &(items.len() - without).to_string()),
                        ("{without}", &without.to_string()),
                    ],
                ),
            )
        }
    }
}

fn single_header(item: &ReviewItem, lang: Language) -> String {
    fill_template(
        lang.text(Msg::QuickFixHeader),
        &[("{reason}", &reason_text(item.reason, item.text_state, lang))],
    )
}

fn read_line(item: &ReviewItem, kinds: &Kinds, lang: Language) -> String {
    let fields = &item.fields;
    let date = fields.date.map(|date| date.to_string());
    let kind = fields
        .kind
        .as_deref()
        .map(|kind| lang.kind_label(&kinds.catalog, kind));
    fill_template(
        lang.text(Msg::QuickFixRead),
        &[
            ("{kind}", kind.as_deref().unwrap_or(MISSING_FIELD)),
            ("{date}", date.as_deref().unwrap_or(MISSING_FIELD)),
            (
                "{sender}",
                fields.sender.as_deref().unwrap_or(MISSING_FIELD),
            ),
        ],
    )
}

fn directory_text(directory: &std::path::Path) -> String {
    let shown = directory.display().to_string();
    if shown.is_empty() {
        ".".to_owned()
    } else {
        shown
    }
}

/// The wording of an entry and, beside it, the path it files to.
pub fn entry_texts(menu: &QuickFix, entry: &Entry, lang: Language) -> (String, Option<String>) {
    let count = entry.count.to_string();
    let several = entry.count > 1;
    match entry.action {
        QuickFixAction::Accept(_) if several => (
            fill_template(
                lang.text(Msg::QuickFixAcceptGroup),
                &[
                    ("{count}", &count),
                    (
                        "{directory}",
                        &directory_text(
                            entry.target.as_deref().unwrap_or(std::path::Path::new("")),
                        ),
                    ),
                ],
            ),
            None,
        ),
        QuickFixAction::Accept(_) => (
            lang.text(Msg::QuickFixAccept).to_owned(),
            entry.target.as_ref().map(|target| {
                format::fit_middle(
                    &format!("→ {}", target.display()),
                    DETAIL_WIDTH,
                    font_size::STATUS,
                )
            }),
        ),
        QuickFixAction::SetFields if menu.items().len() > 1 => (
            fill_template(
                lang.text(Msg::QuickFixSetFieldsGroup),
                &[("{count}", &count)],
            ),
            None,
        ),
        QuickFixAction::SetFields => (lang.text(Msg::QuickFixSetField).to_owned(), None),
        QuickFixAction::CreateRule => (lang.text(Msg::QuickFixCreateRule).to_owned(), None),
        QuickFixAction::Later => (lang.text(Msg::QuickFixLater).to_owned(), None),
        QuickFixAction::Exclude if several => (
            fill_template(lang.text(Msg::QuickFixExcludeAll), &[("{count}", &count)]),
            None,
        ),
        QuickFixAction::Exclude => (lang.text(Msg::QuickFixExclude).to_owned(), None),
        QuickFixAction::FileGroup(_) => (
            fill_template(lang.text(Msg::QuickFixFileGroup), &[("{count}", &count)]),
            None,
        ),
        QuickFixAction::Back => (lang.text(Msg::QuickFixBack).to_owned(), None),
    }
}

fn item<'a>(
    menu: &QuickFix,
    entry: &Entry,
    index: usize,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let is_selected = index == menu.selected();
    let (label, detail) = entry_texts(menu, entry, lang);
    let (label_color, detail_color) = if is_selected {
        (palette.cursor_text, palette.menu_shortcut_on_cursor)
    } else {
        (palette.text, palette.menu_shortcut)
    };
    let key = match entry.action {
        QuickFixAction::Accept(_) => Some(kit::ENTER_KEY),
        QuickFixAction::Later => Some(ESCAPE_KEY),
        _ => None,
    };
    let content = row![
        text(label)
            .size(font_size::ROW)
            .color(label_color)
            .width(Length::Fill)
            .wrapping(text::Wrapping::None),
        text(detail.unwrap_or_default())
            .size(font_size::STATUS)
            .color(detail_color)
            .wrapping(text::Wrapping::None),
    ]
    .extend(key.map(|cap_text| kit::key_cap_in_row(cap_text, is_selected, palette)))
    .spacing(spacing::S)
    .align_y(alignment::Vertical::Center);
    let button = button(content)
        .on_press(Message::QuickFix(QuickFixMessage::Click(index)))
        .padding([0.0, spacing::CELL_PADDING_X])
        .height(MENU_ITEM_HEIGHT)
        .width(Length::Fill)
        .style(theme::menu_item(palette, is_selected));
    mouse_area(button)
        .on_enter(Message::QuickFix(QuickFixMessage::Hover(index)))
        .into()
}
