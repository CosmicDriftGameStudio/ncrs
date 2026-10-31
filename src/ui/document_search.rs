//! The search page of the Hermes card: the words of the text, the values
//! that narrow the hits and the list of what is left. Pure view over
//! [`DocumentSearch`].

use iced::widget::{column, container, row, text};
use iced::{alignment, Element, Length, Size};
use ncrs_index::DocumentFields;

use super::document_preview;
use super::frame::{self, DialogSize, Frame, KeyHints};
use super::hermes::{self, Badges};
use super::kit::{self, dim, error_line, Columns, Tone};
use super::picker;
use super::theme::{font_size, size, spacing};
use crate::app::fill_template;
use crate::document_search::{DocumentSearch, FacetValue, VISIBLE_ROWS};
use crate::i18n::{FuturamaName, Language, Msg};
use crate::messages::{DocumentSearchMessage, HermesFocus, HermesPage, Message};
use crate::palette::Palette;
use crate::review_work::amount_text;

pub const SEARCH_ID: &str = "document-search-text";

const SIZE: DialogSize = DialogSize::Card;

/// Most values shown in a row of a facet; chosen ones always are.
const MAX_CHIPS: usize = 6;

/// How the width of a row is shared: name, date, sender, amount.
const PORTIONS: [u16; 4] = [4, 3, 3, 3];

/// How the room below the facets is shared: the list, then the preview.
const LIST_PORTION: u16 = 3;
const PREVIEW_PORTION: u16 = 2;

/// The height of the list and the preview beside it.
const SPLIT_HEIGHT: f32 = 290.0;

/// A row that shows a snippet below the name.
const SNIPPET_ROW_HEIGHT: f32 = size::ROW + 20.0;

/// The width of the label of a facet row.
const LABEL_WIDTH: f32 = 110.0;

fn send(message: DocumentSearchMessage) -> Message {
    Message::DocumentSearch(message)
}

pub fn view<'a>(
    state: &'a DocumentSearch,
    window: Size,
    badges: Badges,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let line_chars = frame::content_chars(SIZE, window, frame::SIDEBAR_ROOM);
    let mut body = column![
        kit::paragraph(lang.text(Msg::DocumentSearchExplain), palette),
        kit::text_field(
            lang.text(Msg::DocumentSearchPlaceholder),
            state.text(),
            SEARCH_ID,
            Some(|text| send(DocumentSearchMessage::TextChanged(text))),
            send(DocumentSearchMessage::Activate),
            palette,
        ),
    ]
    .spacing(spacing::LINE_GAP);
    for facet_row in facet_rows(state, lang, palette) {
        body = body.push(facet_row);
    }
    if let Some(problem) = state.problem() {
        body = body.push(error_line(problem.to_owned(), palette));
    } else if state.is_busy() {
        body = body.push(dim(lang.text(Msg::DocumentSearchSearching), palette));
    } else if state.found().is_empty() {
        body = body.push(dim(lang.text(Msg::DocumentSearchEmptyNone), palette));
    } else {
        body = body.push(summary(state, lang, palette));
        if state.shown_len() == 0 {
            body = body.push(dim(lang.text(Msg::DocumentSearchEmptyNoMatch), palette));
        } else {
            body = body.push(split(state, line_chars, badges.focus, lang, palette));
        }
    }

    Frame::new(
        lang.text(Msg::ActionSearchDocuments),
        body,
        SIZE,
        window,
        lang,
        palette,
    )
    .title_theme(lang.futurama_name(FuturamaName::ActionSearchDocuments))
    .buttons(buttons(state, lang, palette))
    .sidebar(hermes::nav(HermesPage::Search, badges, lang, palette))
    .on_close(send(DocumentSearchMessage::Close))
    .hints(KeyHints {
        enter: true,
        tab: false,
    })
    .into_element()
}

/// One value of a facet row: what it is called, how many hits carry it and
/// what choosing it means.
struct Chip {
    label: String,
    count: usize,
    value: FacetValue,
}

/// A row per facet that has values, with the label first.
fn facet_rows<'a>(
    state: &DocumentSearch,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let counts = state.counts();
    let years = counts.years.iter().map(|(year, count)| Chip {
        label: year.to_string(),
        count: *count,
        value: FacetValue::Year(*year),
    });
    let recipients = counts.recipients.iter().map(|(name, count)| Chip {
        label: name.clone(),
        count: *count,
        value: FacetValue::Recipient(name.clone()),
    });
    let senders = counts.senders.iter().map(|(name, count)| Chip {
        label: name.clone(),
        count: *count,
        value: FacetValue::Sender(name.clone()),
    });
    let kinds = counts.kinds.iter().map(|(kind, count)| Chip {
        label: lang.kind_label(state.kinds(), kind).into_owned(),
        count: *count,
        value: FacetValue::Kind(kind.clone()),
    });
    let tags = counts.tags.iter().map(|(tag, count)| Chip {
        label: lang.tag_label(state.kinds(), tag),
        count: *count,
        value: FacetValue::Tag(tag.clone()),
    });
    let mut rows = Vec::new();
    for (label, chips) in [
        (Msg::DocumentSearchFacetYear, years.collect::<Vec<_>>()),
        (Msg::DocumentSearchFacetKind, kinds.collect::<Vec<_>>()),
        (
            Msg::DocumentSearchFacetRecipient,
            recipients.collect::<Vec<_>>(),
        ),
        (Msg::DocumentSearchFacetSender, senders.collect::<Vec<_>>()),
        (Msg::DocumentSearchFacetTags, tags.collect::<Vec<_>>()),
    ] {
        if !chips.is_empty() {
            rows.push(facet_row(state, lang.text(label), chips, lang, palette));
        }
    }
    rows
}

fn facet_row<'a>(
    state: &DocumentSearch,
    label: &str,
    chips: Vec<Chip>,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let chosen = |chip: &Chip| state.selection().holds(&chip.value, state.kinds());
    let total = chips.len();
    // The chosen values always show; the others fill what is left of the room.
    let mut room = MAX_CHIPS.saturating_sub(chips.iter().filter(|chip| chosen(chip)).count());
    let shown: Vec<Chip> = chips
        .into_iter()
        .filter(|chip| {
            chosen(chip) || {
                let fits = room > 0;
                room = room.saturating_sub(1);
                fits
            }
        })
        .collect();
    let left_out = total - shown.len();
    let mut buttons: Vec<Element<'a, Message>> = shown
        .into_iter()
        .map(|chip| {
            let tone = if chosen(&chip) {
                Tone::Primary
            } else {
                Tone::Secondary
            };
            kit::button(
                &format!("{} ({})", chip.label, chip.count),
                Some(send(DocumentSearchMessage::ToggleFacet(chip.value))),
                tone,
                palette,
            )
        })
        .collect();
    if left_out > 0 {
        buttons.push(dim(
            fill_template(
                lang.text(Msg::DocumentSearchMoreValues),
                &[("{count}", &left_out.to_string())],
            ),
            palette,
        ));
    }
    row![
        text(label.to_owned())
            .size(font_size::BODY)
            .color(palette.dim_text)
            .width(Length::Fixed(LABEL_WIDTH)),
        row(buttons)
            .spacing(spacing::S)
            .align_y(alignment::Vertical::Center)
            .wrap(),
    ]
    .spacing(spacing::S)
    .into()
}

/// The number of documents with the sum of their amounts and the button that
/// saves the question as a report; below it, while values are chosen, what
/// they are.
fn summary<'a>(state: &DocumentSearch, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let count = state.shown_len();
    let mut line = fill_template(
        lang.text(if count == 1 {
            Msg::DocumentSearchCountOne
        } else {
            Msg::DocumentSearchCountOther
        }),
        &[("{count}", &count.to_string())],
    );
    if let Some(total) = state.total_amount() {
        line.push_str(" · ");
        line.push_str(&fill_template(
            lang.text(Msg::DocumentSearchSum),
            &[("{amount}", &amount_text(&total))],
        ));
    }
    let top = row![
        dim(line, palette),
        iced::widget::space::horizontal(),
        kit::button(
            lang.text(Msg::DocumentSearchSaveAsReport),
            state
                .can_save_as_report()
                .then(|| send(DocumentSearchMessage::SaveAsReport)),
            Tone::Secondary,
            palette,
        ),
    ]
    .align_y(alignment::Vertical::Center);
    let chosen = chosen_values(state, lang);
    if chosen.is_empty() {
        return top.into();
    }
    column![
        top,
        dim(
            fill_template(
                lang.text(Msg::DocumentSearchFiltersTogether),
                &[(
                    "{values}",
                    &chosen.join(&format!(" {} ", lang.text(Msg::DocumentSearchFiltersAnd)))
                )],
            ),
            palette
        )
    ]
    .spacing(spacing::XS)
    .into()
}

/// The chosen values in the order of the rows.
fn chosen_values(state: &DocumentSearch, lang: Language) -> Vec<String> {
    let selection = state.selection();
    selection
        .year
        .map(|year| year.to_string())
        .into_iter()
        .chain(
            selection
                .kind
                .as_deref()
                .map(|kind| lang.kind_label(state.kinds(), kind).into_owned()),
        )
        .chain(selection.recipient.clone())
        .chain(selection.sender.clone())
        .chain(selection.tags.iter().cloned())
        .collect()
}

fn split<'a>(
    state: &'a DocumentSearch,
    line_chars: usize,
    focus: HermesFocus,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let portions = LIST_PORTION + PREVIEW_PORTION;
    let list_chars = (line_chars * usize::from(LIST_PORTION))
        .checked_div(usize::from(portions))
        .unwrap_or_default()
        .saturating_sub(frame::chars_that_fit(spacing::L));
    let preview: Element<'a, Message> = if state.selected().is_some_and(|hit| hit.missing) {
        container(dim(lang.text(Msg::DocumentSearchMissing), palette))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        document_preview::view(state.preview(), lang, palette)
    };
    row![
        container(list(state, list_chars, focus, lang, palette))
            .width(Length::FillPortion(LIST_PORTION)),
        container(preview)
            .width(Length::FillPortion(PREVIEW_PORTION))
            .height(Length::Fill),
    ]
    .spacing(spacing::L)
    .height(SPLIT_HEIGHT)
    .into()
}

fn list<'a>(
    state: &DocumentSearch,
    line_chars: usize,
    focus: HermesFocus,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let columns = Columns {
        portions: &PORTIONS,
        line_chars,
    };
    let header = columns.header(
        &[
            lang.text(Msg::ColumnName),
            lang.text(Msg::DocumentSearchColumnDate),
            lang.text(Msg::DocumentSearchColumnSender),
            lang.text(Msg::DocumentSearchColumnAmount),
        ],
        palette,
    );
    let first = state.first_visible();
    let missing_mark = lang.text(Msg::DocumentSearchMissing);
    let row_height = if state.has_snippets() {
        SNIPPET_ROW_HEIGHT
    } else {
        size::ROW
    };
    let rows = state
        .shown()
        .enumerate()
        .skip(first)
        .take(VISIBLE_ROWS)
        .map(|(index, hit)| {
            let selected = state.cursor() == index;
            let color = if selected && focus == HermesFocus::Content {
                palette.cursor_text
            } else {
                palette.text
            };
            let file_name = hit
                .path
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            let name = if hit.missing {
                format!("{file_name} ({missing_mark})")
            } else {
                file_name
            };
            let DocumentFields {
                date,
                sender,
                amount,
                ..
            } = &hit.fields;
            let date_text = date.map(|day| day.format("%Y-%m-%d").to_string());
            let fields = row![
                columns.cell(0, &name, color),
                columns.cell(1, date_text.as_deref().unwrap_or_default(), color),
                columns.cell(2, sender.as_deref().unwrap_or_default(), color),
                columns.cell(
                    3,
                    &amount.as_ref().map(amount_text).unwrap_or_default(),
                    color
                ),
            ]
            .align_y(alignment::Vertical::Center)
            .height(size::ROW);
            let mut lines = column![fields];
            if let Some(snippet) = state.snippet_of(hit) {
                // The snippet is set in the larger row font, which the width of a
                // cell is not measured for.
                let fitted = snippet.fitted((line_chars * 2).checked_div(3).unwrap_or_default());
                lines = lines.push(
                    row![picker::highlighted_dim(
                        &fitted.text,
                        &fitted.marks,
                        selected && focus == HermesFocus::Content,
                        usize::MAX,
                        palette,
                    )]
                    .padding([0.0, spacing::CELL_PADDING_X]),
                );
            }
            kit::list_row_of_height(
                lines.into(),
                send(DocumentSearchMessage::Select(index)),
                selected,
                focus == HermesFocus::Content,
                row_height,
                palette,
            )
        })
        .collect();
    kit::scroll_list_of_height(
        header,
        rows,
        kit::ListView {
            len: state.shown_len(),
            visible: VISIBLE_ROWS,
            offset: first,
        },
        row_height,
        |delta| send(DocumentSearchMessage::Scrolled(delta)),
        palette,
    )
}

fn buttons<'a>(
    state: &DocumentSearch,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let mut buttons = Vec::new();
    if !state.selection().is_empty() {
        buttons.push(kit::button(
            lang.text(Msg::DocumentSearchClear),
            Some(send(DocumentSearchMessage::ClearFacets)),
            Tone::Secondary,
            palette,
        ));
    }
    buttons.push(kit::button(
        lang.text(Msg::DocumentSearchClose),
        Some(send(DocumentSearchMessage::Close)),
        Tone::Secondary,
        palette,
    ));
    buttons.push(kit::enter_button(
        lang.text(Msg::DocumentSearchGoToFile),
        state
            .selected()
            .is_some_and(|hit| !hit.missing)
            .then(|| send(DocumentSearchMessage::Activate)),
        Tone::Primary,
        palette,
    ));
    buttons
}
