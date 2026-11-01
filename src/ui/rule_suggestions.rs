//! The rule suggestions page, a tab: one row per suggested rule and, right of
//! it, the condition, target, kind and evidence of the selected one with the
//! documents it matches. Pure view over [`RuleSuggestions`].

use iced::widget::Column;
use iced::Element;
use ncrs_index::{ConditionKind, HermesCounts};
use ncrs_intake::{MIN_FOLDER_DOCUMENTS, MIN_SHARE_IN_FOLDER_PERCENT};

use super::filing_pieces::{file_name_path_parts, folder_path_parts};
use super::format;
use super::hermes::{self, Badges};
use super::hermes_kit::{self as page_kit, RowTwo};
use super::kit;
use crate::app::fill_template;
use crate::filing_pieces::FilingTarget;
use crate::i18n::{FuturamaName, Language, Msg};
use crate::messages::{HermesPage, Message, RuleSuggestionsMessage};
use crate::palette::Palette;
use crate::rule_suggestions::{Matches, Problem, RuleSuggestions, VISIBLE_ROWS};
use crate::suggestion_work::{condition_text, SuggestedRule};

/// Most matching documents listed; the rest is counted.
pub const SHOWN_DOCUMENTS: usize = 12;

/// What fits a line of the list, in characters.
const NAME_CHARS: usize = 30;
const TARGET_CHARS: usize = 38;
/// What fits a line of the detail, in characters.
const DOCUMENT_CHARS: usize = 76;

fn send(message: RuleSuggestionsMessage) -> Message {
    Message::RuleSuggestions(message)
}

pub fn view<'a>(
    state: &'a RuleSuggestions,
    badges: Badges,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let body: Element<'a, Message> = if state.rows().is_empty() {
        empty(state, badges.counts, lang, palette)
    } else {
        page_kit::list_and_detail(list(state, lang, palette), detail(state, lang, palette))
    };
    page_kit::tab_page(
        hermes::tab_nav(HermesPage::RuleSuggestions, badges, lang, palette),
        header(state, lang, palette),
        body,
        Some(action_bar(state, lang, palette)),
        palette,
    )
}

fn header<'a>(state: &RuleSuggestions, lang: Language, palette: &Palette) -> Element<'a, Message> {
    page_kit::page_header(
        lang.text(Msg::ActionRuleSuggestions),
        lang.futurama_name(FuturamaName::ActionRuleSuggestions),
        Some(lang.text(Msg::RuleSuggestionsExplain).to_owned()),
        None,
        vec![page_kit::secondary(
            lang.text(Msg::RuleSuggestionsReanalyse),
            (!state.is_busy()).then_some(send(RuleSuggestionsMessage::Reanalyse)),
            palette,
        )],
        palette,
    )
}

fn empty<'a>(
    state: &RuleSuggestions,
    counts: Option<HermesCounts>,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut lines = Column::new().spacing(page_kit::GAP_S);
    lines = lines.push(page_kit::hint(
        empty_text(state.is_busy(), counts, lang),
        palette,
    ));
    if let Some(problem) = state.problem() {
        lines = lines.push(page_kit::error_line(problem_text(problem, lang), palette));
    }
    page_kit::centered_note(lines.into())
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

/// How many of the documents that fit the condition lie in the folder.
fn share_text(suggestion: &SuggestedRule, lang: Language) -> String {
    let evidence = &suggestion.evidence;
    let total = if is_sender_suggestion(suggestion) {
        evidence
            .matching_in_folder
            .saturating_add(evidence.matching_elsewhere)
    } else {
        evidence.folder_documents
    };
    fill_template(
        lang.text(Msg::RuleSuggestionsShare),
        &[
            ("{matching}", &evidence.matching_in_folder.to_string()),
            ("{total}", &total.to_string()),
        ],
    )
}

fn list<'a>(state: &'a RuleSuggestions, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let rows = state
        .rows()
        .iter()
        .enumerate()
        .skip(state.first_visible())
        .take(VISIBLE_ROWS)
        .map(|(index, suggestion)| {
            let target = FilingTarget::parse(&suggestion.spec.filing_pattern).in_words(lang);
            page_kit::list_row_two(
                RowTwo {
                    primary: format::ellipsize_end(&condition_label(suggestion, lang), NAME_CHARS),
                    secondary: format::ellipsize_end(&format!("→ {target}"), TARGET_CHARS),
                    right: Some(share_text(suggestion, lang)),
                    lead: None,
                    muted: false,
                },
                index == state.cursor(),
                Some(send(RuleSuggestionsMessage::Select(index))),
                palette,
            )
        })
        .collect();
    page_kit::list_pane(
        rows,
        |delta| send(RuleSuggestionsMessage::Scrolled(delta)),
        palette,
    )
}

fn detail<'a>(
    state: &'a RuleSuggestions,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let Some(suggestion) = state.selected() else {
        return page_kit::detail(Vec::new());
    };
    let target = FilingTarget::parse(&suggestion.spec.filing_pattern);
    let mut children = vec![
        page_kit::form_row(
            lang.text(Msg::RuleSuggestionsColumnCondition),
            page_kit::value(condition_label(suggestion, lang), palette),
            None,
            palette,
        ),
        page_kit::form_row(
            lang.text(Msg::RuleSuggestionsColumnPattern),
            page_kit::path(folder_path_parts(&target, lang), palette),
            None,
            palette,
        ),
        page_kit::form_row(
            lang.text(Msg::FilingPiecesFileNameLabel),
            page_kit::path(file_name_path_parts(&target, lang), palette),
            None,
            palette,
        ),
        page_kit::form_row(
            lang.text(Msg::RuleSuggestionsColumnKind),
            page_kit::value(suggestion.spec.kind.clone().unwrap_or_default(), palette),
            None,
            palette,
        ),
        page_kit::form_row(
            lang.text(Msg::RuleSuggestionsColumnEvidence),
            page_kit::sentence(evidence_text(suggestion, lang), palette),
            None,
            palette,
        ),
    ];
    children.push(documents_heading(suggestion, lang, palette));
    children.extend(documents(state, lang, palette));
    if let Some(problem) = state.problem() {
        children.push(page_kit::error_line(problem_text(problem, lang), palette));
    }
    page_kit::detail(children)
}

fn documents_heading<'a>(
    suggestion: &SuggestedRule,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let heading = if is_sender_suggestion(suggestion) {
        Msg::RuleSuggestionsDocumentsHeadingSender
    } else {
        Msg::RuleSuggestionsDocumentsHeading
    };
    page_kit::section(lang.text(heading), palette)
}

fn documents<'a>(
    state: &RuleSuggestions,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let note = |sentence: String| vec![page_kit::hint(sentence, palette)];
    match state.matches() {
        Matches::Unknown | Matches::Looking => {
            note(lang.text(Msg::RuleSuggestionsDocumentsLooking).to_owned())
        }
        Matches::Found(paths) if paths.is_empty() => {
            note(lang.text(Msg::RuleSuggestionsDocumentsNone).to_owned())
        }
        Matches::Found(paths) => {
            let mut lines: Vec<Element<'a, Message>> = paths
                .iter()
                .take(SHOWN_DOCUMENTS)
                .map(|path| {
                    page_kit::list_row_one(
                        page_kit::mono(
                            format::ellipsize_middle(&path.display().to_string(), DOCUMENT_CHARS),
                            page_kit::TEXT_SMALL,
                            palette.text,
                        ),
                        None,
                        false,
                        false,
                        None,
                        palette,
                    )
                })
                .collect();
            let rest = paths.len().saturating_sub(SHOWN_DOCUMENTS);
            if rest > 0 {
                lines.push(page_kit::hint(
                    fill_template(
                        lang.text(Msg::RuleSuggestionsDocumentsMore),
                        &[("{count}", &rest.to_string())],
                    ),
                    palette,
                ));
            }
            lines
        }
    }
}

fn action_bar<'a>(
    state: &RuleSuggestions,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let decidable = !state.is_busy() && state.selected().is_some();
    let on_press = |message| decidable.then_some(send(message));
    page_kit::action_bar(
        Some(lang.text(Msg::RuleSuggestionsNote).to_owned()),
        vec![
            page_kit::secondary(
                lang.text(Msg::RuleSuggestionsDiscard),
                on_press(RuleSuggestionsMessage::Discard),
                palette,
            ),
            page_kit::secondary(
                lang.text(Msg::RuleSuggestionsEdit),
                on_press(RuleSuggestionsMessage::Edit),
                palette,
            ),
            page_kit::primary(
                lang.text(Msg::RuleSuggestionsConfirm),
                Some(kit::ENTER_KEY),
                on_press(RuleSuggestionsMessage::Confirm),
                palette,
            ),
        ],
        palette,
    )
}

fn problem_text(problem: &Problem, lang: Language) -> String {
    match problem {
        Problem::Failed(detail) => fill_template(
            lang.text(Msg::RuleSuggestionsProblemFailed),
            &[("{detail}", detail)],
        ),
    }
}
