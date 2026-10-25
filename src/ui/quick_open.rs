//! The Quick Open box: a text field, a status line and the best matches with
//! the matched characters marked. Pure view over [`QuickOpen`].

use iced::{Element, Size};

use super::kit;
use super::picker;
use crate::app::fill_template;
use crate::i18n::{Language, Msg};
use crate::messages::{Message, QuickOpenMessage};
use crate::palette::Palette;
use crate::quick_open::{Hit, QuickOpen};

pub const QUICK_OPEN_ID: &str = "quick-open-query";

fn to_quick_open(message: QuickOpenMessage) -> Message {
    Message::QuickOpen(message)
}

pub fn view<'a>(
    quick_open: &'a QuickOpen,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let directory = quick_open.root().file_name().map_or_else(
        || quick_open.root().display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let subtitle = fill_template(lang.text(Msg::QuickopenSubtitle), &[("{dir}", &directory)]);

    let field = kit::text_field(
        lang.text(Msg::QuickopenPlaceholder),
        quick_open.query(),
        QUICK_OPEN_ID,
        Some(|text| to_quick_open(QuickOpenMessage::QueryChanged(text))),
        to_quick_open(QuickOpenMessage::Activate),
        palette,
    );

    let rows = quick_open
        .hits()
        .iter()
        .enumerate()
        .map(|(index, hit)| hit_row(quick_open, hit, index, window, palette))
        .collect();

    picker::frame(
        picker::Content {
            title: subtitle,
            title_theme: None,
            subtitle: Some(lang.text(Msg::QuickopenTitle).to_owned()),
            field,
            status: status(quick_open, lang),
            rows,
            on_close: to_quick_open(QuickOpenMessage::Close),
        },
        window,
        lang,
        palette,
    )
}

fn status(quick_open: &QuickOpen, lang: Language) -> String {
    let count = quick_open.file_count().to_string();
    let filled = |message| fill_template(lang.text(message), &[("{count}", &count)]);
    if quick_open.has_query() && !quick_open.is_ranking() && quick_open.hits().is_empty() {
        lang.text(Msg::QuickopenNoMatch).to_owned()
    } else if quick_open.walking() {
        filled(Msg::QuickopenStatusWalking)
    } else if quick_open.truncated() {
        filled(Msg::QuickopenStatusTruncated)
    } else {
        filled(Msg::QuickopenStatusDone)
    }
}

fn hit_row<'a>(
    quick_open: &'a QuickOpen,
    hit: &Hit,
    index: usize,
    window: Size,
    palette: &Palette,
) -> Element<'a, Message> {
    let selected = index == quick_open.cursor();
    let path = quick_open.relative_path(hit);
    kit::list_row(
        picker::highlighted(
            path,
            &hit.highlight,
            selected,
            picker::row_chars(window, 0.0),
            palette,
        ),
        to_quick_open(QuickOpenMessage::RowActivated(index)),
        selected,
        palette,
    )
}
