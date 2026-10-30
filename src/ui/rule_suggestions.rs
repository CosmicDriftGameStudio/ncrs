//! The rule suggestions view: one row per suggested rule with its condition,
//! target, kind and evidence, the documents the selected one matches, and the
//! form to edit it. Modal, like the Inspection. Pure view over
//! [`RuleSuggestions`].

use iced::widget::{column, container, row};
use iced::{alignment, Element, Size};
use ncrs_index::{ConditionKind, HermesCounts};
use ncrs_intake::{MIN_FOLDER_DOCUMENTS, MIN_SHARE_IN_FOLDER_PERCENT};

use super::format;
use super::frame::{self, DialogSize, Frame, KeyHints};
use super::hermes::{self, Badges};
use super::kit::{self, dim, error_line, paragraph, section, Columns, Tone};
use super::theme::spacing;
use crate::app::fill_template;
use crate::i18n::{FuturamaName, Language, Msg};
use crate::messages::{HermesFocus, HermesPage, Message, RuleSuggestionsMessage};
use crate::palette::Palette;
use crate::rule_suggestions::{Edit, Matches, Problem, RuleSuggestions, VISIBLE_ROWS};
use crate::suggestion_work::{condition_text, SuggestedRule};

pub const CONDITION_ID: &str = "rule-suggestions-condition";
pub const KIND_ID: &str = "rule-suggestions-kind";
pub const PATTERN_ID: &str = "rule-suggestions-pattern";

/// Most matching documents listed; the rest is counted.
pub const SHOWN_DOCUMENTS: usize = 6;

const SIZE: DialogSize = DialogSize::Card;

/// How the width of a row is shared: condition, target, kind, evidence.
const PORTIONS: [u16; 4] = [4, 4, 2, 9];
const LABEL_WIDTH: f32 = 170.0;

fn send(message: RuleSuggestionsMessage) -> Message {
    Message::RuleSuggestions(message)
}

pub fn view<'a>(
    state: &'a RuleSuggestions,
    window: Size,
    badges: Badges,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let line_chars = frame::content_chars(SIZE, window, frame::SIDEBAR_ROOM);
    let mut body = column![paragraph(lang.text(Msg::RuleSuggestionsExplain), palette)]
        .spacing(spacing::LINE_GAP);

    if state.rows().is_empty() {
        body = body.push(dim(
            empty_text(state.is_busy(), badges.counts, lang),
            palette,
        ));
    } else {
        body = body.push(list(state, line_chars, badges.focus, lang, palette));
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
    .title_theme(lang.futurama_name(FuturamaName::ActionRuleSuggestions))
    .buttons(buttons(state, lang, palette))
    .sidebar(hermes::nav(
        HermesPage::RuleSuggestions,
        badges,
        lang,
        palette,
    ))
    .on_close(send(RuleSuggestionsMessage::Close))
    .hints(KeyHints {
        enter: true,
        tab: editing,
    })
    .into_element()
}

/// Why the list is empty: the analysis is still running, or what a
/// suggestion needs and how many archive documents have text right now.
fn empty_text(busy: bool, counts: Option<HermesCounts>, lang: Language) -> String {
    if busy {
        return lang.text(Msg::RuleSuggestionsAnalysing).to_owned();
    }
    let mut text = fill_template(
        lang.text(Msg::RuleSuggestionsEmpty),
        &[
            ("{min}", &MIN_FOLDER_DOCUMENTS.to_string()),
            ("{share}", &MIN_SHARE_IN_FOLDER_PERCENT.to_string()),
        ],
    );
    if let Some(known) = counts {
        text.push(' ');
        text.push_str(&fill_template(
            lang.text(Msg::RuleSuggestionsEmptyCounts),
            &[
                (
                    "{with_text}",
                    &format::grouped(known.archive_documents_with_text),
                ),
                ("{total}", &format::grouped(known.archive_documents)),
            ],
        ));
    }
    text
}

fn is_sender_suggestion(suggestion: &SuggestedRule) -> bool {
    suggestion
        .spec
        .conditions
        .single_condition()
        .is_some_and(|condition| condition.kind == ConditionKind::Sender)
}

fn condition_label(suggestion: &SuggestedRule, lang: Language) -> String {
    let message = if is_sender_suggestion(suggestion) {
        Msg::RuleSuggestionsConditionSender
    } else {
        Msg::RuleSuggestionsConditionText
    };
    fill_template(
        lang.text(message),
        &[(
            "{value}",
            condition_text(&suggestion.spec).unwrap_or_default(),
        )],
    )
}

fn evidence_text(suggestion: &SuggestedRule, lang: Language) -> String {
    let evidence = &suggestion.evidence;
    let Some(folder) = evidence.subtree.as_deref() else {
        return fill_template(
            lang.text(Msg::RuleSuggestionsEvidence),
            &[
                ("{matching}", &evidence.matching_in_folder.to_string()),
                ("{folder}", &evidence.folder_documents.to_string()),
                ("{elsewhere}", &evidence.matching_elsewhere.to_string()),
            ],
        );
    };
    if is_sender_suggestion(suggestion) {
        let total = evidence
            .matching_in_folder
            .saturating_add(evidence.matching_elsewhere);
        return fill_template(
            lang.text(Msg::RuleSuggestionsEvidenceSender),
            &[
                ("{matching}", &evidence.matching_in_folder.to_string()),
                ("{total}", &total.to_string()),
                ("{folder}", folder),
            ],
        );
    }
    fill_template(
        lang.text(Msg::RuleSuggestionsEvidenceText),
        &[
            ("{matching}", &evidence.matching_in_folder.to_string()),
            ("{folder_count}", &evidence.folder_documents.to_string()),
            ("{folder}", folder),
            ("{elsewhere}", &evidence.matching_elsewhere.to_string()),
        ],
    )
}

fn list<'a>(
    state: &'a RuleSuggestions,
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
            lang.text(Msg::RuleSuggestionsColumnCondition),
            lang.text(Msg::RuleSuggestionsColumnPattern),
            lang.text(Msg::RuleSuggestionsColumnKind),
            lang.text(Msg::RuleSuggestionsColumnEvidence),
        ],
        palette,
    );

    let first = state.first_visible();
    let rows = state
        .rows()
        .iter()
        .enumerate()
        .skip(first)
        .take(VISIBLE_ROWS)
        .map(|(index, suggestion)| {
            let selected = index == state.cursor();
            let color = if selected && focus == HermesFocus::Content {
                palette.cursor_text
            } else {
                palette.text
            };
            let content = row![
                columns.cell(0, &condition_label(suggestion, lang), color),
                columns.path_cell(1, &suggestion.spec.filing_pattern, color),
                columns.cell(2, &suggestion.spec.kind, color),
                columns.cell(3, &evidence_text(suggestion, lang), color),
            ]
            .align_y(alignment::Vertical::Center);
            kit::list_row(
                content.into(),
                send(RuleSuggestionsMessage::Select(index)),
                selected,
                focus == HermesFocus::Content,
                palette,
            )
        })
        .collect();
    kit::scroll_list(
        header,
        rows,
        kit::ListView {
            len: state.rows().len(),
            visible: VISIBLE_ROWS,
            offset: first,
        },
        |delta| send(RuleSuggestionsMessage::Scrolled(delta)),
        palette,
    )
}

fn documents<'a>(
    state: &'a RuleSuggestions,
    line_chars: usize,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut lines = column![].spacing(spacing::LINE_GAP);
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
                lines = lines.push(paragraph(
                    format::ellipsize_middle(&path.display().to_string(), line_chars),
                    palette,
                ));
            }
            let rest = paths.len().saturating_sub(SHOWN_DOCUMENTS);
            if rest > 0 {
                lines = lines.push(dim(
                    fill_template(
                        lang.text(Msg::RuleSuggestionsDocumentsMore),
                        &[("{count}", &rest.to_string())],
                    ),
                    palette,
                ));
            }
        }
    }
    let heading = if state.selected().is_some_and(is_sender_suggestion) {
        Msg::RuleSuggestionsDocumentsHeadingSender
    } else {
        Msg::RuleSuggestionsDocumentsHeading
    };
    section(lang.text(heading), lines, palette)
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
    .spacing(spacing::LINE_GAP)
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
        container(paragraph(label, palette)).width(LABEL_WIDTH),
        kit::text_field(
            "",
            value,
            id,
            Some(move |typed| send(on_input(typed))),
            send(RuleSuggestionsMessage::Confirm),
            palette,
        ),
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
        if primary {
            kit::enter_button(lang.text(label), on_press.map(send), Tone::Primary, palette)
        } else {
            kit::button(
                lang.text(label),
                on_press.map(send),
                Tone::Secondary,
                palette,
            )
        }
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
