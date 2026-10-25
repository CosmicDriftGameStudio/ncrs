//! The rule suggestions view: one row per suggested rule with its condition,
//! target, kind and evidence, the documents the selected one matches, and the
//! form to edit it. Modal, like the Inspection. Pure view over
//! [`RuleSuggestions`].

use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, text, text_input, Column};
use iced::{alignment, Color, Element, Length, Size};

use super::filing::{error_line, paragraph};
use super::format;
use super::frame::{self, DialogSize, Frame, KeyHints};
use super::layout::ROW_HEIGHT;
use super::theme::{self, font_size, spacing};
use crate::app::fill_template;
use crate::i18n::{Language, Msg};
use crate::messages::{Message, RuleSuggestionsMessage};
use crate::palette::Palette;
use crate::rule_suggestions::{Edit, Matches, Problem, RuleSuggestions};
use crate::suggestion_work::{condition_text, SuggestedRule};

pub const CONDITION_ID: &str = "rule-suggestions-condition";
pub const KIND_ID: &str = "rule-suggestions-kind";
pub const PATTERN_ID: &str = "rule-suggestions-pattern";

/// Most suggestions listed at once; the list follows the cursor beyond that.
pub const VISIBLE_ROWS: usize = 8;
/// Most matching documents listed; the rest is counted.
pub const SHOWN_DOCUMENTS: usize = 6;

const SIZE: DialogSize = DialogSize::Card;

/// How the width of a row is shared: condition, target, kind, evidence.
const PORTIONS: [u16; 4] = [5, 5, 3, 5];
const LABEL_WIDTH: f32 = 170.0;

fn send(message: RuleSuggestionsMessage) -> Message {
    Message::RuleSuggestions(message)
}

pub fn view<'a>(
    state: &'a RuleSuggestions,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let line_chars = frame::content_chars(SIZE, window, 0.0);
    let mut body = column![paragraph(lang.text(Msg::RuleSuggestionsExplain), palette)]
        .spacing(spacing::SECTION_GAP);

    if state.rows().is_empty() {
        body = body.push(dim(
            lang.text(if state.is_busy() {
                Msg::RuleSuggestionsAnalysing
            } else {
                Msg::RuleSuggestionsEmpty
            }),
            palette,
        ));
    } else {
        body = body.push(list(state, line_chars, lang, palette));
        if state.is_busy() {
            body = body.push(dim(lang.text(Msg::RuleSuggestionsAnalysing), palette));
        }
    }

    match state.edit() {
        Some(edit) => body = body.push(form(edit, lang, palette)),
        None if state.selected().is_some() => {
            body = body.push(documents(state, line_chars, lang, palette));
        }
        None => {}
    }
    if let Some(problem) = state.problem() {
        body = body.push(error_line(problem_text(problem, lang), palette));
    }

    let editing = state.edit().is_some();
    Frame::new(
        lang.text(Msg::ActionRuleSuggestions),
        body,
        SIZE,
        window,
        lang,
        palette,
    )
    .title_theme(lang.text(Msg::ActionRuleSuggestionsTheme))
    .buttons(buttons(state, lang, palette))
    .on_close(send(RuleSuggestionsMessage::Close))
    .hints(KeyHints {
        enter: true,
        tab: editing,
    })
    .into_element()
}

fn dim<'a>(content: &'a str, palette: &Palette) -> Element<'a, Message> {
    text(content)
        .size(font_size::STATUS)
        .color(palette.dim_text)
        .wrapping(Wrapping::Word)
        .into()
}

fn cell<'a>(content: &str, portion: u16, chars: usize, color: Color) -> Element<'a, Message> {
    container(
        text(format::ellipsize_end(content, chars))
            .size(font_size::ROW)
            .color(color)
            .wrapping(Wrapping::None),
    )
    .width(Length::FillPortion(portion))
    .clip(true)
    .into()
}

/// The characters a column of `portion` holds in a line of `line_chars`.
fn column_chars(line_chars: usize, portion: u16) -> usize {
    let total: u16 = PORTIONS.iter().sum();
    line_chars
        .saturating_mul(usize::from(portion))
        .checked_div(usize::from(total))
        .unwrap_or_default()
        .saturating_sub(2)
}

fn evidence_text(suggestion: &SuggestedRule, lang: Language) -> String {
    let evidence = suggestion.evidence;
    fill_template(
        lang.text(Msg::RuleSuggestionsEvidence),
        &[
            ("{matching}", &evidence.matching_in_folder.to_string()),
            ("{folder}", &evidence.folder_documents.to_string()),
            ("{elsewhere}", &evidence.matching_elsewhere.to_string()),
        ],
    )
}

fn list<'a>(
    state: &'a RuleSuggestions,
    line_chars: usize,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let [condition, target, kind, evidence] = PORTIONS;
    let chars = |portion| column_chars(line_chars, portion);
    let header = row![
        cell(
            lang.text(Msg::RuleSuggestionsColumnCondition),
            condition,
            chars(condition),
            palette.dim_text
        ),
        cell(
            lang.text(Msg::RuleSuggestionsColumnPattern),
            target,
            chars(target),
            palette.dim_text
        ),
        cell(
            lang.text(Msg::RuleSuggestionsColumnKind),
            kind,
            chars(kind),
            palette.dim_text
        ),
        cell(
            lang.text(Msg::RuleSuggestionsColumnEvidence),
            evidence,
            chars(evidence),
            palette.dim_text
        ),
    ]
    .padding([0.0, spacing::CELL_PADDING_X])
    .height(ROW_HEIGHT)
    .align_y(alignment::Vertical::Center);

    let first = (state.cursor() + 1).saturating_sub(VISIBLE_ROWS);
    let rows = state
        .rows()
        .iter()
        .enumerate()
        .skip(first)
        .take(VISIBLE_ROWS)
        .map(|(index, suggestion)| {
            let selected = index == state.cursor();
            let color = if selected {
                palette.cursor_text
            } else {
                palette.text
            };
            let content = row![
                cell(
                    condition_text(&suggestion.spec).unwrap_or_default(),
                    condition,
                    chars(condition),
                    color
                ),
                cell(
                    &suggestion.spec.filing_pattern,
                    target,
                    chars(target),
                    color
                ),
                cell(&suggestion.spec.kind, kind, chars(kind), color),
                cell(
                    &evidence_text(suggestion, lang),
                    evidence,
                    chars(evidence),
                    color
                ),
            ]
            .align_y(alignment::Vertical::Center);
            button(content)
                .on_press(send(RuleSuggestionsMessage::Select(index)))
                .width(Length::Fill)
                .padding([0.0, spacing::CELL_PADDING_X])
                .height(ROW_HEIGHT)
                .style(theme::row(palette, false, selected, true))
                .into()
        });
    column![
        header,
        container(Column::with_children(rows))
            .height(ROW_HEIGHT * state.rows().len().min(VISIBLE_ROWS) as f32)
            .clip(true)
    ]
    .into()
}

fn documents<'a>(
    state: &'a RuleSuggestions,
    line_chars: usize,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut lines = column![text(lang.text(Msg::RuleSuggestionsDocumentsHeading))
        .size(font_size::STATUS)
        .color(palette.accent)]
    .spacing(spacing::CELL_PADDING_X);
    match state.matches() {
        Matches::Unknown | Matches::Looking => {
            lines = lines.push(dim(
                lang.text(Msg::RuleSuggestionsDocumentsLooking),
                palette,
            ));
        }
        Matches::Found(paths) if paths.is_empty() => {
            lines = lines.push(dim(lang.text(Msg::RuleSuggestionsDocumentsNone), palette));
        }
        Matches::Found(paths) => {
            for path in paths.iter().take(SHOWN_DOCUMENTS) {
                lines = lines.push(
                    text(format::ellipsize_middle(
                        &path.display().to_string(),
                        line_chars,
                    ))
                    .size(font_size::STATUS)
                    .color(palette.text)
                    .wrapping(Wrapping::None),
                );
            }
            let rest = paths.len().saturating_sub(SHOWN_DOCUMENTS);
            if rest > 0 {
                lines = lines.push(
                    text(fill_template(
                        lang.text(Msg::RuleSuggestionsDocumentsMore),
                        &[("{count}", &rest.to_string())],
                    ))
                    .size(font_size::STATUS)
                    .color(palette.dim_text),
                );
            }
        }
    }
    lines.into()
}

fn form<'a>(edit: &'a Edit, lang: Language, palette: &Palette) -> Element<'a, Message> {
    column![
        field(
            lang.text(Msg::RuleSuggestionsFieldCondition),
            &edit.condition,
            CONDITION_ID,
            RuleSuggestionsMessage::ConditionChanged,
            palette,
        ),
        field(
            lang.text(Msg::RuleSuggestionsFieldKind),
            &edit.kind,
            KIND_ID,
            RuleSuggestionsMessage::KindChanged,
            palette,
        ),
        field(
            lang.text(Msg::RuleSuggestionsFieldPattern),
            &edit.pattern,
            PATTERN_ID,
            RuleSuggestionsMessage::PatternChanged,
            palette,
        ),
        dim(lang.text(Msg::RuleSuggestionsFormHint), palette),
    ]
    .spacing(spacing::SECTION_GAP)
    .into()
}

fn field<'a>(
    label: &'a str,
    value: &'a str,
    id: &'static str,
    on_input: fn(String) -> RuleSuggestionsMessage,
    palette: &Palette,
) -> Element<'a, Message> {
    row![
        text(label)
            .size(font_size::STATUS)
            .color(palette.text)
            .width(LABEL_WIDTH),
        text_input("", value)
            .id(id)
            .on_input(move |typed| send(on_input(typed)))
            .on_submit(send(RuleSuggestionsMessage::Confirm))
            .size(font_size::ROW)
            .padding([3.0, 6.0])
            .style(theme::search_input(palette))
            .width(Length::Fill),
    ]
    .spacing(spacing::CELL_PADDING_X)
    .align_y(alignment::Vertical::Center)
    .into()
}

fn buttons<'a>(
    state: &RuleSuggestions,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let action = |label: Msg, on_press: Option<RuleSuggestionsMessage>, primary: bool| {
        frame::action(lang.text(label), on_press.map(send), primary, palette)
    };
    let decidable = !state.is_busy() && state.selected().is_some();
    let confirm = action(
        Msg::RuleSuggestionsConfirm,
        decidable.then_some(RuleSuggestionsMessage::Confirm),
        true,
    );
    if state.edit().is_some() {
        return vec![
            action(
                Msg::RuleSuggestionsCancelEdit,
                Some(RuleSuggestionsMessage::CancelEdit),
                false,
            ),
            confirm,
        ];
    }
    vec![
        action(
            Msg::RuleSuggestionsClose,
            Some(RuleSuggestionsMessage::Close),
            false,
        ),
        action(
            Msg::RuleSuggestionsReanalyse,
            (!state.is_busy()).then_some(RuleSuggestionsMessage::Reanalyse),
            false,
        ),
        action(
            Msg::RuleSuggestionsDiscard,
            decidable.then_some(RuleSuggestionsMessage::Discard),
            false,
        ),
        action(
            Msg::RuleSuggestionsEdit,
            decidable.then_some(RuleSuggestionsMessage::Edit),
            false,
        ),
        confirm,
    ]
}

fn problem_text(problem: &Problem, lang: Language) -> String {
    match problem {
        Problem::NoCondition => lang.text(Msg::RuleSuggestionsProblemNoCondition).to_owned(),
        Problem::NoKind => lang.text(Msg::RuleSuggestionsProblemNoKind).to_owned(),
        Problem::InvalidPattern => lang
            .text(Msg::RuleSuggestionsProblemInvalidPattern)
            .to_owned(),
        Problem::Failed(detail) => fill_template(
            lang.text(Msg::RuleSuggestionsProblemFailed),
            &[("{detail}", detail)],
        ),
    }
}
