//! The search view that stands in for a panel: title, fields, summary line and
//! the result rows. Pure view over [`SearchView`]; every click becomes a
//! [`SearchMessage`].
use iced::widget::text::{Span, Wrapping};
use iced::widget::{button, column, container, mouse_area, rich_text, row, span, text, text_input};
use iced::widget::{tooltip, Column};
use iced::{alignment, Element, Font, Length};

use super::key_intercept::key_intercept;
use super::layout::{PANEL_TITLE_HEIGHT, ROW_HEIGHT, SEARCH_HEADER_HEIGHT, TAG_COLUMN_WIDTH};
use super::theme::{self, font_size, spacing};
use crate::app::{fill_template, search_toggle_for_press};
use crate::fs::search::{LineMatch, SearchError};
use crate::i18n::{Language, Msg};
use crate::keymap::Keymap;
use crate::messages::{Message, SearchMessage};
use crate::palette::Palette;
use crate::search_view::{SearchField, SearchRow, SearchView};

pub const QUERY_ID: &str = "search-query";
pub const INCLUDE_ID: &str = "search-include";
pub const EXCLUDE_ID: &str = "search-exclude";

pub fn field_id(field: SearchField) -> &'static str {
    match field {
        SearchField::Query => QUERY_ID,
        SearchField::Include => INCLUDE_ID,
        SearchField::Exclude => EXCLUDE_ID,
    }
}

const FIELD_ROW_HEIGHT: f32 = 28.0;
const LABEL_WIDTH: f32 = 64.0;
const TOGGLE_WIDTH: f32 = 28.0;
const INDENT: f32 = 22.0;

fn to_search(message: SearchMessage) -> Message {
    Message::Search(message)
}

/// The keys of the three toggles as the keymap spells them, for the tooltips.
/// `None` for a toggle without a key.
#[derive(Debug, Clone, Default)]
pub struct ToggleShortcuts {
    pub case: Option<String>,
    pub whole_word: Option<String>,
    pub regex: Option<String>,
}

pub fn view<'a>(
    search: &'a SearchView,
    visible_rows: usize,
    is_active: bool,
    lang: Language,
    palette: &Palette,
    keymap: &'a Keymap,
    shortcuts: &ToggleShortcuts,
) -> Element<'a, Message> {
    let rows = search
        .rows_iter()
        .enumerate()
        .skip(search.scroll_offset)
        .take(visible_rows)
        .filter_map(|(index, search_row)| {
            let cursor = search.cursor == Some(search_row);
            result_row(
                search,
                search_row,
                index,
                RowMarks {
                    highlighted: cursor && is_active,
                    panel_active: is_active,
                },
                palette,
            )
        });

    let body: Element<'a, Message> = if search.results.is_empty() {
        container(
            text(empty_text(search, lang))
                .size(font_size::ROW)
                .color(palette.dim_text),
        )
        .padding(spacing::CELL_PADDING_X)
        .into()
    } else {
        Column::with_children(rows).into()
    };

    let framed = column![
        title_bar(search, lang, palette, is_active),
        header(search, lang, palette, keymap, shortcuts),
        container(body).height(Length::Fill).width(Length::Fill),
    ];
    mouse_area(
        container(framed)
            .padding(spacing::BORDER_WIDTH)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::panel(palette, is_active)),
    )
    .on_scroll(|delta| to_search(SearchMessage::Scrolled(delta)))
    .into()
}

fn title_bar<'a>(
    search: &SearchView,
    lang: Language,
    palette: &Palette,
    active: bool,
) -> Element<'a, Message> {
    let title = fill_template(
        lang.text(Msg::SearchTitle),
        &[("{path}", &search.root.display().to_string())],
    );
    container(text(title).size(font_size::TITLE).wrapping(Wrapping::None))
        .padding([0.0, spacing::CELL_PADDING_X])
        .height(PANEL_TITLE_HEIGHT)
        .width(Length::Fill)
        .align_y(alignment::Vertical::Center)
        .clip(true)
        .style(theme::panel_title(palette, active))
        .into()
}

fn header<'a>(
    search: &'a SearchView,
    lang: Language,
    palette: &Palette,
    keymap: &'a Keymap,
    shortcuts: &ToggleShortcuts,
) -> Element<'a, Message> {
    // Wrapped so the toggle keys act in the field: it would type their
    // character (Option+C is a "ç" on a Mac) before any key routing sees them.
    let field = |placeholder: &'a str,
                 value: &'a str,
                 id: &'static str,
                 changed: fn(String) -> SearchMessage| {
        key_intercept(
            text_input(placeholder, value)
                .id(id)
                .on_input(move |text| to_search(changed(text)))
                .on_submit(to_search(SearchMessage::Activate))
                .size(font_size::ROW)
                .padding([3.0, 6.0])
                .style(theme::search_input(palette))
                .width(Length::Fill),
            move |key, physical_key, modifiers| {
                search_toggle_for_press(keymap, key.clone(), physical_key, modifiers).map(to_search)
            },
        )
    };
    let toggle = |label: &'static str,
                  tip: Msg,
                  shortcut: &Option<String>,
                  on: bool,
                  message: SearchMessage| {
        let tip_text = match shortcut {
            Some(keys) => format!("{} ({keys})", lang.text(tip)),
            None => lang.text(tip).to_owned(),
        };
        tooltip(
            button(
                text(label)
                    .size(font_size::COLUMN_HEADER)
                    .font(Font::MONOSPACE)
                    .align_x(alignment::Horizontal::Center)
                    .width(Length::Fill),
            )
            .width(TOGGLE_WIDTH)
            .padding(3.0)
            .style(theme::search_toggle(palette, on))
            .on_press(to_search(message)),
            container(text(tip_text).size(font_size::COLUMN_HEADER))
                .padding(4.0)
                .style(theme::menu(palette)),
            tooltip::Position::Bottom,
        )
    };
    let labelled = |label: Msg, input: Element<'a, Message>| {
        row![
            text(lang.text(label))
                .size(font_size::COLUMN_HEADER)
                .color(palette.dim_text)
                .width(LABEL_WIDTH),
            input,
        ]
        .align_y(alignment::Vertical::Center)
    };

    let query_row = row![
        field(
            lang.text(Msg::SearchQueryPlaceholder),
            &search.query,
            QUERY_ID,
            SearchMessage::QueryChanged,
        ),
        toggle(
            "Aa",
            Msg::SearchTooltipCase,
            &shortcuts.case,
            search.case_sensitive,
            SearchMessage::ToggleCase
        ),
        toggle(
            "ab",
            Msg::SearchTooltipWord,
            &shortcuts.whole_word,
            search.whole_word,
            SearchMessage::ToggleWholeWord
        ),
        toggle(
            ".*",
            Msg::SearchTooltipRegex,
            &shortcuts.regex,
            search.regex,
            SearchMessage::ToggleRegex
        ),
    ]
    .spacing(2.0)
    .align_y(alignment::Vertical::Center);

    let include = field(
        lang.text(Msg::SearchIncludePlaceholder),
        &search.include,
        INCLUDE_ID,
        SearchMessage::IncludeChanged,
    );
    let exclude = field(
        lang.text(Msg::SearchExcludePlaceholder),
        &search.exclude,
        EXCLUDE_ID,
        SearchMessage::ExcludeChanged,
    );

    let (status, status_color) = status_line(search, lang, palette);
    container(
        column![
            container(query_row).height(FIELD_ROW_HEIGHT),
            container(labelled(Msg::SearchFilesLabel, include)).height(FIELD_ROW_HEIGHT),
            container(labelled(Msg::SearchExcludeLabel, exclude)).height(FIELD_ROW_HEIGHT),
            text(status)
                .size(font_size::COLUMN_HEADER)
                .color(status_color)
                .wrapping(Wrapping::None),
        ]
        .spacing(4.0),
    )
    .padding([2.0, spacing::CELL_PADDING_X])
    .height(SEARCH_HEADER_HEIGHT)
    .width(Length::Fill)
    .clip(true)
    .into()
}

fn error_text(error: &SearchError, lang: Language) -> String {
    match error {
        SearchError::Glob { pattern, reason } => fill_template(
            lang.text(Msg::SearchErrorPattern),
            &[("{pattern}", pattern), ("{reason}", reason)],
        ),
        SearchError::Regex(reason) => {
            fill_template(lang.text(Msg::SearchErrorRegex), &[("{reason}", reason)])
        }
        SearchError::Empty => String::new(),
    }
}

/// The line under the fields and the color it is drawn in.
fn status_line(search: &SearchView, lang: Language, palette: &Palette) -> (String, iced::Color) {
    if let Some(error) = search.error.as_ref() {
        return (error_text(error, lang), palette.error);
    }
    if search.running {
        let scanned = search.scanned.to_string();
        let text = fill_template(lang.text(Msg::SearchRunning), &[("{scanned}", &scanned)]);
        return (text, palette.dim_text);
    }
    let Some(totals) = search.totals else {
        return (String::new(), palette.dim_text);
    };
    let files = totals.files.to_string();
    let mut line = if search.is_name_search() {
        fill_template(lang.text(Msg::SearchSummaryNames), &[("{files}", &files)])
    } else {
        let lines = totals.matches.to_string();
        fill_template(
            lang.text(Msg::SearchSummaryContent),
            &[("{files}", &files), ("{lines}", &lines)],
        )
    };
    if totals.skipped > 0 {
        let count = totals.skipped.to_string();
        line.push_str(&fill_template(
            lang.text(Msg::SearchSkipped),
            &[("{count}", &count)],
        ));
    }
    if totals.truncated {
        line.push_str(lang.text(Msg::SearchTruncated));
    }
    (line, palette.dim_text)
}

/// What the empty result area says: how to start, or that nothing matched.
fn empty_text(search: &SearchView, lang: Language) -> &'static str {
    let nothing_asked = search.query.is_empty() && search.include.is_empty();
    if nothing_asked || search.running || search.totals.is_none() {
        if nothing_asked {
            lang.text(Msg::SearchHint)
        } else {
            ""
        }
    } else {
        lang.text(Msg::SearchNoResults)
    }
}

/// How a result row is drawn besides its content.
#[derive(Clone, Copy)]
struct RowMarks {
    /// The cursor is on the row and its panel is the active one.
    highlighted: bool,
    panel_active: bool,
}

fn result_row<'a>(
    search: &'a SearchView,
    kind: SearchRow,
    index: usize,
    marks: RowMarks,
    palette: &Palette,
) -> Option<Element<'a, Message>> {
    let RowMarks {
        highlighted,
        panel_active,
    } = marks;
    // Every row starts with the tag column, so the lines stay under the names.
    let (tagged, content): (bool, Element<'a, Message>) = match kind {
        SearchRow::File(file) => (
            search.is_tagged(file),
            file_row(
                search,
                file,
                RowMarks {
                    highlighted,
                    panel_active,
                },
                search.is_tagged(file),
                palette,
            )?,
        ),
        SearchRow::Line(file, line) => {
            let matched = search.results.get(file)?.lines.get(line)?;
            (false, line_row(matched, highlighted, palette))
        }
    };
    let star = text(if tagged { "*" } else { " " })
        .size(font_size::ROW)
        .color(if tagged {
            palette.accent
        } else {
            palette.dim_text
        })
        .width(TAG_COLUMN_WIDTH)
        .wrapping(Wrapping::None);
    Some(
        mouse_area(
            container(row![star, content].align_y(alignment::Vertical::Center))
                .padding([0.0, spacing::CELL_PADDING_X])
                .height(ROW_HEIGHT)
                .width(Length::Fill)
                .align_y(alignment::Vertical::Center)
                .clip(true)
                .style(theme::search_row(
                    palette,
                    highlighted,
                    tagged,
                    panel_active,
                )),
        )
        .on_press(to_search(SearchMessage::RowClicked(index)))
        .on_double_click(to_search(SearchMessage::RowActivated(index)))
        .into(),
    )
}

fn file_row<'a>(
    search: &'a SearchView,
    file: usize,
    marks: RowMarks,
    tagged: bool,
    palette: &Palette,
) -> Option<Element<'a, Message>> {
    let RowMarks {
        highlighted,
        panel_active,
    } = marks;
    let result = search.results.get(file)?;
    let relative = result
        .path
        .strip_prefix(&search.root)
        .unwrap_or(&result.path);
    let name = relative
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let directory = relative
        .parent()
        .map(|parent| parent.display().to_string())
        .unwrap_or_default();

    let chevron: Element<'a, Message> = if result.lines.is_empty() {
        iced::widget::Space::new().width(INDENT).into()
    } else {
        let folded = search.collapsed.contains(&result.path);
        button(
            text(if folded { "▸" } else { "▾" })
                .size(font_size::ROW)
                .align_x(alignment::Horizontal::Center),
        )
        .width(INDENT)
        .height(ROW_HEIGHT)
        .padding(0.0)
        .style(theme::chevron_button(
            palette,
            highlighted,
            tagged,
            panel_active,
        ))
        .on_press(to_search(SearchMessage::ToggleGroup(file)))
        .into()
    };
    // The tint of a tagged row is the panel's, where the text on it is the
    // plain one: the dim colour nearly vanishes on it.
    let dim = if highlighted {
        palette.cursor_text
    } else if tagged {
        theme::tagged_text_color(palette, panel_active)
    } else {
        palette.dim_text
    };
    let count = if result.lines.is_empty() {
        String::new()
    } else {
        result.lines.len().to_string()
    };
    Some(
        row![
            chevron,
            text(name)
                .size(font_size::ROW)
                .font(Font {
                    weight: iced::font::Weight::Bold,
                    ..Font::MONOSPACE
                })
                .wrapping(Wrapping::None),
            text(directory)
                .size(font_size::COLUMN_HEADER)
                .color(dim)
                .wrapping(Wrapping::None),
            iced::widget::Space::new().width(Length::Fill),
            text(count)
                .size(font_size::COLUMN_HEADER)
                .color(dim)
                .wrapping(Wrapping::None),
        ]
        .spacing(8.0)
        .align_y(alignment::Vertical::Center)
        .into(),
    )
}

fn line_row<'a>(
    matched: &'a LineMatch,
    highlighted: bool,
    palette: &Palette,
) -> Element<'a, Message> {
    let dim = if highlighted {
        palette.cursor_text
    } else {
        palette.dim_text
    };
    let spans = match_spans(matched, highlighted, palette);
    row![
        iced::widget::Space::new().width(INDENT),
        text(format!("{}", matched.number))
            .size(font_size::ROW)
            .font(Font::MONOSPACE)
            .color(dim)
            .width(56.0)
            .align_x(alignment::Horizontal::Right)
            .wrapping(Wrapping::None),
        rich_text(spans)
            .size(font_size::ROW)
            .font(Font::MONOSPACE)
            .wrapping(Wrapping::None),
    ]
    .spacing(8.0)
    .align_y(alignment::Vertical::Center)
    .into()
}

/// The line cut at the matches, which are drawn in the accent color. Ranges
/// come from the engine as byte offsets into the stored text; one that does not
/// fall on characters of it is skipped rather than trusted.
fn match_spans<'a>(
    matched: &'a LineMatch,
    highlighted: bool,
    palette: &Palette,
) -> Vec<Span<'a, ()>> {
    let plain = if highlighted {
        palette.cursor_text
    } else {
        palette.text
    };
    let mut spans = Vec::new();
    let mut at = 0;
    for range in &matched.ranges {
        let (Some(before), Some(hit)) = (
            matched.text.get(at..range.start),
            matched.text.get(range.clone()),
        ) else {
            continue;
        };
        if !before.is_empty() {
            spans.push(span(before).color(plain));
        }
        spans.push(
            span(hit)
                .color(if highlighted { plain } else { palette.accent })
                .underline(true),
        );
        at = range.end;
    }
    if let Some(rest) = matched.text.get(at..) {
        if !rest.is_empty() {
            spans.push(span(rest).color(plain));
        }
    }
    spans
}
