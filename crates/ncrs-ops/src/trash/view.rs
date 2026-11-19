//! The trash view that stands in for a panel: a title, the columns and the
//! entries the trash can give back. Pure view over [`TrashView`]; every click
//! becomes a [`TrashMessage`].
use std::time::{Duration, SystemTime};

use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, text, Space};
use iced::{alignment, Color, Element, Length};

use ncrs_core::DateFormat;
use ncrs_fs::TrashReach;
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_ui_kit::format;
use ncrs_ui_kit::frame::chars_that_fit;
use ncrs_ui_kit::kit::{cells_row, panel_frame, scroll_list, Columns, ListView, RowMarks};
use ncrs_ui_kit::layout::{ROW_HEIGHT, TAG_COLUMN_WIDTH};
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::theme::{font_size, spacing};

use super::TrashView;
use crate::message::{OpsMessage, TrashMessage};

/// Name, original folder, deletion time.
const PORTIONS: [u16; 3] = [4, 5, 3];

/// What the trash view is called, in its tab, above the list and in the drive
/// menu.
pub fn heading(reach: TrashReach, lang: Language) -> &'static str {
    lang.text(match reach {
        TrashReach::OwnDeletions => Msg::TrashTitleOwn,
        TrashReach::Everything | TrashReach::FinderOnly => Msg::TrashTitleEverything,
    })
}

/// When an entry was deleted. Empty for a trash that did not say.
fn deleted_text(deleted_at: i64, date_format: DateFormat, lang: Language) -> String {
    let Ok(seconds) = u64::try_from(deleted_at) else {
        return String::new();
    };
    if seconds == 0 {
        return String::new();
    }
    format::time_in(
        SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(seconds)),
        date_format,
        lang,
    )
}

#[derive(Debug)]
pub struct Props<'a> {
    pub view: &'a TrashView,
    pub visible_rows: usize,
    pub is_active: bool,
    pub panel_width: f32,
    pub lang: Language,
    pub date_format: DateFormat,
    pub palette: &'a Palette,
    /// The key that opens the trash in the Finder, when it has one.
    pub finder_key: Option<String>,
}

pub fn view<'a>(props: Props<'a>) -> Element<'a, OpsMessage> {
    let Props {
        view,
        visible_rows,
        is_active,
        panel_width,
        lang,
        date_format,
        palette,
        finder_key,
    } = props;
    let columns = Columns {
        portions: &PORTIONS,
        line_chars: chars_that_fit(panel_width - 2.0 * spacing::CELL_PADDING_X - TAG_COLUMN_WIDTH),
    };
    let title = if view.loading {
        heading(view.reach, lang).to_owned()
    } else {
        format!("{} ({})", heading(view.reach, lang), view.items.len())
    };
    let header = row![
        Space::new().width(spacing::CELL_PADDING_X + TAG_COLUMN_WIDTH),
        columns.header(
            &[
                lang.text(Msg::TrashColumnName),
                lang.text(Msg::TrashColumnFolder),
                lang.text(Msg::TrashColumnDeleted),
            ],
            palette,
        ),
    ];

    let offset = view.window.offset(view.items.len(), visible_rows);
    let rows: Vec<Element<'a, OpsMessage>> = view
        .items
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible_rows)
        .map(|(index, item)| {
            let marks = RowMarks {
                highlighted: index == view.cursor && is_active,
                tagged: view.is_tagged(index),
                panel_active: is_active,
            };
            let name = item
                .original
                .file_name()
                .map(|name| format::shown_name(&name.to_string_lossy()))
                .unwrap_or_default();
            let folder = item
                .original
                .parent()
                .map(|parent| format::shown_name(&parent.display().to_string()))
                .unwrap_or_default();
            cells_row(
                &columns,
                [
                    name,
                    folder,
                    deleted_text(item.deleted_at, date_format, lang),
                ],
                marks,
                OpsMessage::Trash(TrashMessage::RowClicked(index)),
                palette,
            )
        })
        .collect();

    let body: Element<'a, OpsMessage> = match status_text(view, lang, palette) {
        Some((shown, color)) => container(text(shown).size(font_size::ROW).color(color))
            .padding(spacing::CELL_PADDING_X)
            .into(),
        None => scroll_list(
            header.into(),
            rows,
            ListView {
                len: view.items.len(),
                visible: visible_rows,
                offset,
            },
            |delta| OpsMessage::Trash(TrashMessage::Scrolled(delta)),
            palette,
        ),
    };

    let note = (view.reach == TrashReach::OwnDeletions).then(|| match finder_key {
        Some(key) => fill_template(lang.text(Msg::TrashFooterOwnWithKey), &[("{key}", &key)]),
        None => lang.text(Msg::TrashFooterOwn).to_owned(),
    });
    let dim_line = |shown: String| {
        text(shown)
            .size(font_size::COLUMN_HEADER)
            .color(palette.dim_text)
            .wrapping(Wrapping::None)
    };
    let mut footer = column![dim_line(lang.text(Msg::TrashFooter).to_owned())]
        .padding([0.0, spacing::CELL_PADDING_X]);
    if let Some(shown) = note {
        footer = footer.push(dim_line(shown));
    }

    panel_frame(
        column![
            container(text(title).size(font_size::ROW).wrapping(Wrapping::None))
                .padding([0.0, spacing::CELL_PADDING_X])
                .height(ROW_HEIGHT)
                .align_y(alignment::Vertical::Center),
            container(body).height(Length::Fill).width(Length::Fill),
            footer,
        ],
        palette,
    )
}

/// What stands in place of the rows: the reading, the error or the empty list.
fn status_text(view: &TrashView, lang: Language, palette: &Palette) -> Option<(String, Color)> {
    if view.loading && view.items.is_empty() {
        return Some((lang.text(Msg::TrashReading).to_owned(), palette.dim_text));
    }
    if let Some(reason) = view.error.as_deref() {
        return Some((
            fill_template(lang.text(Msg::TrashError), &[("{reason}", reason)]),
            palette.error,
        ));
    }
    view.items.is_empty().then(|| {
        let empty = match view.reach {
            TrashReach::OwnDeletions => Msg::TrashEmptyOwn,
            TrashReach::Everything | TrashReach::FinderOnly => Msg::TrashEmptyEverything,
        };
        (lang.text(empty).to_owned(), palette.dim_text)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_deletion_time_beyond_what_the_system_can_hold_shows_nothing() {
        let text = deleted_text(i64::MAX, DateFormat::Iso, Language::default());
        assert!(text.is_empty());
    }
}
