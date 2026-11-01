//! The search view that stands in for a panel: title, fields, summary line and
//! the result rows. Pure view over [`SearchView`]; every click becomes a
//! [`SearchMessage`].
use iced::widget::text::{Span, Wrapping};
use iced::widget::{button, column, container, mouse_area, rich_text, row, span, text, text_input};
use iced::widget::{tooltip, Column, Row};
use iced::{alignment, Element, Font, Length};

use super::format;
use super::hermes_kit::{self, ChipKind, MenuItem};
use super::key_intercept::key_intercept;
use super::layout;
use super::panel::{self, HitRow, RowClick};
use super::theme::{self, font_size, spacing};
use crate::app::{fill_template, search_toggle_for_press};
use crate::fs::search::{LineMatch, SearchError};
use crate::i18n::{FuturamaName, Language, Msg};
use crate::keymap::Keymap;
use crate::messages::{Message, PanelSide, SearchMessage};
use crate::palette::Palette;
use crate::query_work::QueryHit;
use crate::review_work::amount_text;
use crate::search_filters::{Facet, FacetValue, SearchFilters};
use crate::search_view::{FileResult, SearchField, SearchView};
use crate::selection::Selection;

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
const TOGGLE_WIDTH: f32 = 28.0;

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

/// The space the search has in its panel.
#[derive(Debug, Clone, Copy)]
pub struct Room {
    pub visible_rows: usize,
}

pub fn view<'a>(
    search: &'a SearchView,
    room: Room,
    is_active: bool,
    lang: Language,
    palette: &Palette,
    keymap: &'a Keymap,
    shortcuts: &ToggleShortcuts,
) -> Element<'a, Message> {
    let rows = search
        .results
        .iter()
        .enumerate()
        .skip(search.scroll_offset)
        .take(room.visible_rows)
        .filter_map(|(index, result)| result_row(search, result, index, is_active, lang, palette));

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
        header(search, lang, palette, keymap, shortcuts),
        panel::plain_column_header(lang, palette),
        container(body).height(Length::Fill).width(Length::Fill),
    ];
    mouse_area(
        container(framed)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::panel(palette)),
    )
    .on_scroll(|delta| to_search(SearchMessage::Scrolled(delta)))
    .into()
}

/// What the search view's tab in the title bar says.
pub fn title(search: &SearchView, lang: Language) -> String {
    let plain = fill_template(
        lang.text(Msg::SearchTitle),
        &[("{path}", &search.root.display().to_string())],
    );
    match lang.futurama_name(FuturamaName::ActionSearch) {
        Some(theme) => format!("{plain} ({theme})"),
        None => plain,
    }
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
    // The index answers a search with filters; its words ignore these.
    let toggles_disabled = search.is_filtered();
    let toggle = |label: &'static str,
                  tip: Msg,
                  shortcut: &Option<String>,
                  on: bool,
                  message: SearchMessage| {
        let look = theme::search_toggle(palette, on);
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
            .style(move |theme, status| {
                let mut style = look(theme, status);
                if toggles_disabled {
                    style.text_color = style.text_color.scale_alpha(theme::DISABLED_OPACITY);
                }
                style
            })
            .on_press_maybe((!toggles_disabled).then(|| to_search(message))),
            container(text(tip_text).size(font_size::COLUMN_HEADER))
                .padding(4.0)
                .style(theme::menu(palette)),
            tooltip::Position::Bottom,
        )
    };
    let label_width = label_column_width(lang);
    let labelled = |label: Msg, input: Element<'a, Message>| {
        row![
            text(lang.text(label))
                .size(font_size::COLUMN_HEADER)
                .color(palette.dim_text)
                .wrapping(Wrapping::None)
                .width(label_width),
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

    let (status_text, status_color) = status_line(search, lang, palette);
    let status = text(status_text)
        .size(font_size::COLUMN_HEADER)
        .color(status_color)
        .wrapping(Wrapping::None);
    let mut fields = column![
        container(query_row).height(FIELD_ROW_HEIGHT),
        container(labelled(Msg::SearchFilesLabel, include)).height(FIELD_ROW_HEIGHT),
        container(labelled(Msg::SearchExcludeLabel, exclude)).height(FIELD_ROW_HEIGHT),
    ]
    .spacing(4.0);
    let height = layout::search_header_height(search.filters.is_some());
    match search.filters.as_ref() {
        Some(filters) => {
            fields = fields
                .push(filter_row(filters, lang, palette))
                .push(filter_hint(filters, lang, palette))
                .push(summary_row(search, filters, status, lang, palette));
        }
        None => fields = fields.push(status),
    }
    fields = fields.push(hit_list_row(search, lang, palette));
    container(fields)
        .padding([2.0, spacing::CELL_PADDING_X])
        .height(height)
        .width(Length::Fill)
        .clip(true)
        .into()
}

/// The hits as a list in one of the panels; nothing to open without hits.
fn hit_list_row<'a>(
    search: &SearchView,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let open = |label: Msg, side: PanelSide| {
        hermes_kit::secondary(
            lang.text(label),
            (!search.results.is_empty()).then(|| to_search(SearchMessage::OpenHitList(side))),
            palette,
        )
    };
    row![
        iced::widget::Space::new().width(Length::Fill),
        text(lang.text(Msg::SearchHitsAsListLabel))
            .size(font_size::COLUMN_HEADER)
            .color(palette.dim_text)
            .wrapping(Wrapping::None),
        open(Msg::SearchOpenHitsLeft, PanelSide::Left),
        open(Msg::SearchOpenHitsRight, PanelSide::Right),
    ]
    .spacing(hermes_kit::GAP_S)
    .align_y(alignment::Vertical::Center)
    .height(layout::SEARCH_HIT_ROW_HEIGHT)
    .into()
}

/// A filter of the row: what it is called, what is chosen and what it offers.
struct FilterControl {
    label: Msg,
    facet: Facet,
    chosen: Option<String>,
    items: Vec<MenuItem>,
}

fn filter_controls(filters: &SearchFilters, lang: Language) -> Vec<FilterControl> {
    let counts = filters.counts();
    let selection = filters.selection();
    let kinds = filters.kinds();
    let item = |label: String, count: usize, value: FacetValue| {
        let mark = if selection.holds(&value, kinds) {
            "✓ "
        } else {
            ""
        };
        MenuItem::new(
            format!("{mark}{label} ({count})"),
            to_search(SearchMessage::ToggleFacet(value)),
        )
    };
    let chosen_tags = (!selection.tags.is_empty()).then(|| {
        selection
            .tags
            .iter()
            .map(|tag| lang.tag_label(kinds, tag))
            .collect::<Vec<_>>()
            .join(", ")
    });
    vec![
        FilterControl {
            label: Msg::DocumentSearchFacetTags,
            facet: Facet::Tags,
            chosen: chosen_tags,
            items: counts
                .tags
                .iter()
                .map(|(tag, count)| {
                    item(
                        lang.tag_label(kinds, tag),
                        *count,
                        FacetValue::Tag(tag.clone()),
                    )
                })
                .collect(),
        },
        FilterControl {
            label: Msg::DocumentSearchFacetKind,
            facet: Facet::Kind,
            chosen: selection
                .kind
                .as_deref()
                .map(|kind| lang.kind_label(kinds, kind).into_owned()),
            items: counts
                .kinds
                .iter()
                .map(|(kind, count)| {
                    item(
                        lang.kind_label(kinds, kind).into_owned(),
                        *count,
                        FacetValue::Kind(kind.clone()),
                    )
                })
                .collect(),
        },
        FilterControl {
            label: Msg::DocumentSearchFacetYear,
            facet: Facet::Year,
            chosen: selection.year.map(|year| year.to_string()),
            items: counts
                .years
                .iter()
                .map(|(year, count)| item(year.to_string(), *count, FacetValue::Year(*year)))
                .collect(),
        },
        FilterControl {
            label: Msg::DocumentSearchFacetSender,
            facet: Facet::Sender,
            chosen: selection.sender.clone(),
            items: counts
                .senders
                .iter()
                .map(|(name, count)| item(name.clone(), *count, FacetValue::Sender(name.clone())))
                .collect(),
        },
        FilterControl {
            label: Msg::DocumentSearchFacetRecipient,
            facet: Facet::Recipient,
            chosen: selection.recipient.clone(),
            items: counts
                .recipients
                .iter()
                .map(|(name, count)| {
                    item(name.clone(), *count, FacetValue::Recipient(name.clone()))
                })
                .collect(),
        },
    ]
}

/// The five filters, always: one without values is shown dimmed.
fn filter_row<'a>(
    filters: &'a SearchFilters,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    Row::with_children(filter_controls(filters, lang).into_iter().map(|control| {
        hermes_kit::filter(
            lang.text(control.label),
            control.chosen.as_deref(),
            &control.items,
            Length::FillPortion(1),
            to_search(SearchMessage::ClearFacet(control.facet)),
            palette,
        )
    }))
    .spacing(hermes_kit::GAP_S)
    .align_y(alignment::Vertical::Center)
    .into()
}

/// The width the two labels of the file name fields need in `lang`, and a
/// gap.
fn label_column_width(lang: Language) -> f32 {
    [Msg::SearchFilesLabel, Msg::SearchExcludeLabel]
        .into_iter()
        .map(|label| format::estimated_width(lang.text(label), font_size::COLUMN_HEADER))
        .fold(0.0, f32::max)
        + hermes_kit::GAP_S
}

/// That the filters look at the index, and while values are chosen that they
/// apply together.
fn filter_hint<'a>(
    filters: &SearchFilters,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let selection = filters.selection();
    if selection.is_empty() {
        return hermes_kit::hint(String::new(), palette);
    }
    let kinds = filters.kinds();
    let chosen: Vec<String> = selection
        .year
        .map(|year| year.to_string())
        .into_iter()
        .chain(
            selection
                .kind
                .as_deref()
                .map(|kind| lang.kind_label(kinds, kind).into_owned()),
        )
        .chain(selection.recipient.clone())
        .chain(selection.sender.clone())
        .chain(selection.tags.iter().map(|tag| lang.tag_label(kinds, tag)))
        .collect();
    let together = fill_template(
        lang.text(Msg::DocumentSearchFiltersTogether),
        &[(
            "{values}",
            &chosen.join(&format!(" {} ", lang.text(Msg::DocumentSearchFiltersAnd))),
        )],
    );
    hermes_kit::hint(
        format!("{} {together}", lang.text(Msg::SearchFiltersHint)),
        palette,
    )
}

/// The summary line with, at its right, the link that hands the search to the
/// reports: to a new one, or over the one the search was opened from.
fn summary_row<'a>(
    search: &SearchView,
    filters: &SearchFilters,
    status: iced::widget::Text<'a>,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut line = row![status].align_y(alignment::Vertical::Center);
    if search.is_filtered() && !search.running {
        if let Some(total) = filters.total_amount() {
            let sum = fill_template(
                lang.text(Msg::DocumentSearchSum),
                &[("{amount}", &amount_text(&total))],
            );
            line = line.push(
                text(format!(" · {sum}"))
                    .size(font_size::COLUMN_HEADER)
                    .color(palette.dim_text)
                    .wrapping(Wrapping::None),
            );
        }
    }
    line = line.push(iced::widget::Space::new().width(Length::Fill));
    let link = match filters.report() {
        Some(_) => Some((Msg::SearchUpdateReport, SearchMessage::UpdateReport)),
        None => filters
            .can_save_as_report()
            .then_some((Msg::DocumentSearchSaveAsReport, SearchMessage::SaveAsReport)),
    };
    if let Some((label, message)) = link {
        line = line.push(hermes_kit::link(
            lang.text(label),
            Some(to_search(message)),
            palette,
        ));
    }
    container(line)
        .height(hermes_kit::CONTROL_HEIGHT)
        .align_y(alignment::Vertical::Center)
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
    if let Some(problem) = search.filters.as_ref().and_then(SearchFilters::problem) {
        return (problem.to_owned(), palette.error);
    }
    if search.is_filtered() && search.running {
        return (String::new(), palette.dim_text);
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
    if search.not_shown > 0 {
        line.push_str(&fill_template(
            lang.text(Msg::SearchIndexCut),
            &[("{count}", &search.not_shown.to_string())],
        ));
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

/// One hit as a row of the panel's file list: name, folder below the search
/// root, size and date, and under them what the hit matched.
fn result_row<'a>(
    search: &'a SearchView,
    result: &'a FileResult,
    index: usize,
    panel_active: bool,
    lang: Language,
    palette: &Palette,
) -> Option<Element<'a, Message>> {
    let mark = Selection {
        tagged: search.is_tagged(index),
        cursor: search.cursor == Some(index),
    };
    let height = search.row_height();
    let third_line = search
        .third_line
        .then(|| third_line(search, result, mark.cursor && panel_active, lang, palette))
        .flatten();
    let prefix = Row::new().push(panel::tag_star(
        mark,
        panel_active,
        palette,
        height,
        to_search(SearchMessage::ToggleTagAt(index)),
    ));
    Some(panel::hit_row(
        HitRow {
            entry: result.entry()?,
            mark,
            scope: &search.root,
            height,
            third_line,
        },
        panel_active,
        palette,
        |click| {
            to_search(match click {
                RowClick::Tag => SearchMessage::ToggleTagAt(index),
                RowClick::Cursor => SearchMessage::RowClicked(index),
                RowClick::Open => SearchMessage::RowActivated(index),
            })
        },
        prefix,
    ))
}

/// What the third line of a hit says: the tags and facts of a document of the
/// index, else the first matching line.
fn third_line<'a>(
    search: &'a SearchView,
    result: &'a FileResult,
    highlighted: bool,
    lang: Language,
    palette: &Palette,
) -> Option<Element<'a, Message>> {
    let dim = if highlighted {
        palette.cursor_text
    } else {
        palette.dim_text
    };
    if let Some((filters, hit)) = search
        .filters
        .as_ref()
        .filter(|_| search.is_filtered())
        .and_then(|filters| filters.hit_at(&result.path).map(|hit| (filters, hit)))
    {
        return Some(index_details(filters, hit, dim, lang, palette));
    }
    let first = result.first_match()?;
    let more = more_matches_text(result, lang).map(|count| {
        text(count)
            .size(font_size::SMALL)
            .font(Font::MONOSPACE)
            .color(dim)
            .wrapping(Wrapping::None)
            .into()
    });
    Some(
        row![rich_text(match_spans(first, highlighted, palette))
            .size(font_size::SMALL)
            .font(Font::MONOSPACE)
            .wrapping(Wrapping::None)]
        .extend(more)
        .spacing(8.0)
        .align_y(alignment::Vertical::Center)
        .into(),
    )
}

/// The "+3" behind the first matching line; nothing for a file with one.
fn more_matches_text(result: &FileResult, lang: Language) -> Option<String> {
    let more = result.more_matches();
    (more > 0).then(|| {
        fill_template(
            lang.text(Msg::SearchMoreMatches),
            &[("{count}", &more.to_string())],
        )
    })
}

/// A document of the index: the tags it has, and the date and the amount.
fn index_details<'a>(
    filters: &SearchFilters,
    hit: &QueryHit,
    dim: iced::Color,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let kinds = filters.kinds();
    let fields = &hit.fields;
    let own = fields.own_tags();
    let automatic: Vec<String> = fields
        .automatic_tags(kinds)
        .into_iter()
        .filter(|tag| !own.contains(tag))
        .collect();
    let chips = automatic
        .into_iter()
        .map(|tag| (tag, ChipKind::Automatic))
        .chain(own.into_iter().map(|tag| (tag, ChipKind::Own)));
    let facts = [
        fields.date.map(|day| day.format("%Y-%m-%d").to_string()),
        fields.amount.as_ref().map(amount_text),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    row![
        Row::with_children(chips.map(|(tag, kind)| {
            hermes_kit::chip(&lang.tag_label(kinds, &tag), kind, None, palette)
        }))
        .spacing(hermes_kit::GAP_XS)
        .align_y(alignment::Vertical::Center),
        text(facts)
            .size(font_size::SMALL)
            .color(dim)
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
        palette.dim_text
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

// reason: a failing assertion is the signal in a test
#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::search::LineMatch;

    fn result_with(lines: usize) -> FileResult {
        FileResult {
            path: std::path::PathBuf::from("/root/a.txt"),
            lines: (1..=lines as u64)
                .map(|number| LineMatch {
                    number,
                    text: "needle".to_owned(),
                    ranges: std::iter::once(0..6).collect(),
                })
                .collect(),
            size: 0,
            modified: None,
        }
    }

    #[test]
    fn more_matching_lines_are_counted_behind_the_first() {
        for (lang, expected) in [(Language::English, "+3"), (Language::German, "+3")] {
            assert_eq!(
                more_matches_text(&result_with(4), lang).as_deref(),
                Some(expected)
            );
        }
    }

    #[test]
    fn a_file_with_one_matching_line_or_none_has_no_count() {
        assert_eq!(more_matches_text(&result_with(1), Language::English), None);
        assert_eq!(more_matches_text(&result_with(0), Language::English), None);
    }
}
