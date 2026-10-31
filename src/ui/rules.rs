//! The rules page: every confirmed rule as one readable sentence, with where
//! it came from, how many documents it has filed and a switch. The order is
//! the order Hermes tries the rules in. A page of the Hermes card. Pure view
//! over [`RulesPage`].

use iced::widget::{column, container, row, Column, Space};
use iced::{alignment, Element, Length, Size};
use ncrs_intake::FILING_PLACEHOLDERS;

use super::condition_editor::{condition_editor_rows, ConditionHost, ConditionRow};
use super::frame::{self, DialogSize, Frame, KeyHints, PageHint};
use super::hermes::{self, Badges};
use super::kit::{self, dim, error_line, paragraph, Check, Columns, Tone};
use super::layout::SETTINGS_WIDE_CONTROL_WIDTH;
use super::review_list::reason_text;
use super::theme::spacing;
use crate::app::fill_template;
use crate::i18n::{FuturamaName, Language, Msg};
use crate::messages::{HermesFocus, HermesPage, Message, RuleFormMessage, RulesMessage};
use crate::palette::Palette;
use crate::review_work::TextState;
use crate::rule_form::{PatternProblem, RuleForm, RuleFormProblems, RuleFormTarget, TryStatus};
use crate::rules_page::{hits_text, origin_text, rule_sentence, RulesPage, VISIBLE_ROWS};
use crate::rules_work::{ListedRule, TriedDocument, TryOutcome, TryResult, TrySection};

const SIZE: DialogSize = DialogSize::Card;

/// How the width of a row is shared: rule, origin, hits.
const PORTIONS: [u16; 3] = [12, 4, 3];

/// The room of the switch left of a row, in pixels and in characters.
const SWITCH_WIDTH: f32 = 28.0;
const SWITCH_CHARS: usize = 4;

/// How the width of a row of the try is shared: place, document, change,
/// conflicts.
const TRY_PORTIONS: [u16; 4] = [3, 7, 13, 4];

/// Documents of the try listed at once; the form has to fit with them.
const TRY_VISIBLE_ROWS: usize = 6;

/// Blocks of the filing pattern offered side by side in one row.
const BLOCKS_PER_ROW: usize = 4;

const KIND_FIELD_ID: &str = "rule-form-kind";
const TAGS_FIELD_ID: &str = "rule-form-tags";
const PATTERN_FIELD_ID: &str = "rule-form-pattern";

const PAGE_HINTS: &[PageHint] = &[
    PageHint {
        key: "N",
        verb: Msg::RulesHintNew,
    },
    PageHint {
        key: "Enter",
        verb: Msg::RulesHintEdit,
    },
    PageHint {
        key: "Space",
        verb: Msg::RulesHintToggle,
    },
    PageHint {
        key: "Alt ↑↓",
        verb: Msg::RulesHintOrder,
    },
    PageHint {
        key: "Del",
        verb: Msg::RulesHintDelete,
    },
];

fn send(message: RulesMessage) -> Message {
    Message::Rules(message)
}

fn send_form(message: RuleFormMessage) -> Message {
    send(RulesMessage::Form(message))
}

pub fn view<'a>(
    state: &'a RulesPage,
    window: Size,
    badges: Badges,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    if let Some(form) = state.form() {
        return form_view(form, window, badges, lang, palette);
    }
    let line_chars = frame::content_chars(SIZE, window, frame::SIDEBAR_ROOM);
    let mut body = column![
        paragraph(lang.text(Msg::RulesExplain), palette),
        dim(
            fill_template(
                lang.text(Msg::RulesExample),
                &[("{rule}", lang.text(Msg::RulesExampleRule))],
            ),
            palette,
        ),
    ]
    .spacing(spacing::LINE_GAP);

    if state.rows().is_empty() {
        body = body.push(dim(
            lang.text(if state.is_busy() {
                Msg::RulesLoading
            } else {
                Msg::RulesEmpty
            }),
            palette,
        ));
    } else {
        body = body.push(list(state, line_chars, badges.focus, lang, palette));
        if let Some(selected) = state.selected().filter(|_| !state.is_confirming_delete()) {
            body = body.push(paragraph(sentence_of(selected, lang), palette));
        }
    }
    if state.is_confirming_delete() {
        body = body.push(delete_question(state, lang, palette));
    }
    if let Some(detail) = state.problem() {
        body = body.push(error_line(
            fill_template(lang.text(Msg::RulesProblemFailed), &[("{detail}", detail)]),
            palette,
        ));
    }

    Frame::new(
        lang.text(Msg::ActionRules),
        body,
        SIZE,
        window,
        lang,
        palette,
    )
    .title_theme(lang.futurama_name(FuturamaName::ActionRules))
    .buttons(buttons(state, lang, palette))
    .sidebar(hermes::nav(HermesPage::Rules, badges, lang, palette))
    .on_close(send(RulesMessage::Close))
    .hints(KeyHints {
        enter: false,
        tab: false,
    })
    .page_hints(PAGE_HINTS)
    .into_element()
}

fn sentence_of(listed: &ListedRule, lang: Language) -> String {
    rule_sentence(&listed.rule.spec, lang)
}

fn list<'a>(
    state: &'a RulesPage,
    line_chars: usize,
    focus: HermesFocus,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let columns = Columns {
        portions: &PORTIONS,
        line_chars: line_chars.saturating_sub(SWITCH_CHARS),
    };
    let header = row![
        Space::new().width(SWITCH_WIDTH),
        columns.header(
            &[
                lang.text(Msg::RulesColumnRule),
                lang.text(Msg::RulesColumnOrigin),
                lang.text(Msg::RulesColumnHits),
            ],
            palette,
        ),
    ];

    let first = state.first_visible();
    let rows = state
        .rows()
        .iter()
        .enumerate()
        .skip(first)
        .take(VISIBLE_ROWS)
        .map(|(index, listed)| {
            let selected = index == state.cursor();
            let enabled = listed.rule.spec.enabled;
            let color = if selected && focus == HermesFocus::Content {
                palette.cursor_text
            } else if enabled {
                palette.text
            } else {
                palette.dim_text
            };
            let switch = kit::checkbox(
                if enabled { Check::On } else { Check::Off },
                Some(send(RulesMessage::ToggleAt(index))),
                palette,
            );
            let content = row![
                container(switch).width(SWITCH_WIDTH),
                columns.cell(0, &sentence_of(listed, lang), color),
                columns.cell(1, origin_text(listed.rule.origin, lang), color),
                columns.cell(2, &hits_text(listed.hits, lang), color),
            ]
            .align_y(alignment::Vertical::Center);
            kit::list_row(
                content.into(),
                send(RulesMessage::Select(index)),
                selected,
                focus == HermesFocus::Content,
                palette,
            )
        })
        .collect();
    kit::scroll_list(
        header.into(),
        rows,
        kit::ListView {
            len: state.rows().len(),
            visible: VISIBLE_ROWS,
            offset: first,
        },
        |delta| send(RulesMessage::Scrolled(delta)),
        palette,
    )
}

fn delete_question<'a>(
    state: &RulesPage,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let rule = state
        .selected()
        .map(|listed| sentence_of(listed, lang))
        .unwrap_or_default();
    container(paragraph(
        fill_template(lang.text(Msg::RulesDeleteQuestion), &[("{rule}", &rule)]),
        palette,
    ))
    .width(Length::Fill)
    .into()
}

fn buttons<'a>(state: &RulesPage, lang: Language, palette: &Palette) -> Vec<Element<'a, Message>> {
    let button = |label: Msg, on_press: Option<RulesMessage>, tone: Tone| {
        kit::button(lang.text(label), on_press.map(send), tone, palette)
    };
    if state.is_confirming_delete() {
        return vec![
            button(
                Msg::RulesCancel,
                Some(RulesMessage::CancelDelete),
                Tone::Secondary,
            ),
            button(
                Msg::RulesDeleteConfirm,
                Some(RulesMessage::ConfirmDelete),
                Tone::Danger,
            ),
        ];
    }
    let selected = !state.is_busy() && state.selected().is_some();
    let can_move = |delta: isize| {
        selected
            && state
                .cursor()
                .checked_add_signed(delta)
                .is_some_and(|neighbour| neighbour < state.rows().len())
    };
    vec![
        button(Msg::RulesClose, Some(RulesMessage::Close), Tone::Secondary),
        button(
            Msg::RulesNew,
            (!state.is_busy()).then_some(RulesMessage::New),
            Tone::Primary,
        ),
        button(
            Msg::RulesEdit,
            selected.then_some(RulesMessage::Edit),
            Tone::Secondary,
        ),
        button(
            Msg::RulesDelete,
            selected.then_some(RulesMessage::AskDelete),
            Tone::Secondary,
        ),
        button(
            Msg::RulesMoveDown,
            can_move(1).then_some(RulesMessage::Reorder(1)),
            Tone::Secondary,
        ),
        button(
            Msg::RulesMoveUp,
            can_move(-1).then_some(RulesMessage::Reorder(-1)),
            Tone::Secondary,
        ),
    ]
}

fn form_view<'a>(
    form: &RuleForm,
    window: Size,
    badges: Badges,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let title = lang.text(match form.target {
        RuleFormTarget::New | RuleFormTarget::FromDocument => Msg::RulesFormTitleNew,
        RuleFormTarget::Edit(_) => Msg::RulesFormTitleEdit,
        RuleFormTarget::FromSuggestion(_) => Msg::RulesFormTitleConfirm,
    });
    let save_label = lang.text(match form.target {
        RuleFormTarget::FromSuggestion(_) => Msg::RuleSuggestionsConfirm,
        RuleFormTarget::New | RuleFormTarget::FromDocument | RuleFormTarget::Edit(_) => {
            Msg::RulesFormSave
        }
    });
    let mut body = Column::new()
        .push(paragraph(lang.text(Msg::RulesFormPurpose), palette))
        .spacing(spacing::LINE_GAP);
    if let Some(detail) = &form.failure {
        body = body.push(error_line(
            fill_template(lang.text(Msg::RulesProblemFailed), &[("{detail}", detail)]),
            palette,
        ));
    }
    for control in form_rows(form, lang, palette) {
        body = body.push(control);
    }
    let line_chars = frame::content_chars(SIZE, window, frame::SIDEBAR_ROOM);
    body = body.push(try_section(form, line_chars, lang, palette));

    let saving = form.saving;
    Frame::new(title, body, SIZE, window, lang, palette)
        .buttons(vec![
            kit::button(
                lang.text(Msg::RulesFormCancel),
                Some(send_form(RuleFormMessage::Cancel)),
                Tone::Secondary,
                palette,
            ),
            kit::button(
                save_label,
                (!saving).then(|| send_form(RuleFormMessage::Save)),
                Tone::Primary,
                palette,
            ),
        ])
        .sidebar(hermes::nav(HermesPage::Rules, badges, lang, palette))
        .on_close(send(RulesMessage::Close))
        .hints(KeyHints {
            enter: false,
            tab: false,
        })
        .into_element()
}

fn field_row<'a>(
    label: &str,
    description: Option<&str>,
    problem: Option<&str>,
    control: Element<'a, Message>,
    palette: &Palette,
) -> Element<'a, Message> {
    kit::form_row_with_problem(
        label,
        None,
        description,
        problem,
        control,
        SETTINGS_WIDE_CONTROL_WIDTH,
        palette,
    )
}

fn form_rows<'a>(form: &RuleForm, lang: Language, palette: &Palette) -> Vec<Element<'a, Message>> {
    let problems = form.problems();
    let save = send_form(RuleFormMessage::Save);
    let host = ConditionHost {
        wrap: |edit| send_form(RuleFormMessage::Condition(edit)),
        on_submit: &save,
        lang,
        palette,
    };
    let condition_rows = condition_editor_rows(&form.conditions, &host);
    let last = condition_rows.len().saturating_sub(1);
    let mut rows: Vec<Element<'a, Message>> = condition_rows
        .into_iter()
        .enumerate()
        .map(
            |(position, (label, problem, control)): (usize, ConditionRow<'a>)| {
                let missing = (position == last && problems.no_condition)
                    .then(|| lang.text(Msg::RulesFormProblemNoCondition).to_owned());
                field_row(
                    &label,
                    None,
                    problem.or(missing).as_deref(),
                    control,
                    palette,
                )
            },
        )
        .collect();

    let text_field = |value: &str, id: &'static str, on_input: fn(String) -> RuleFormMessage| {
        kit::text_field(
            "",
            value,
            id,
            Some(move |typed| send_form(on_input(typed))),
            save.clone(),
            palette,
        )
    };
    rows.push(field_row(
        lang.text(Msg::ReviewListFieldKind),
        None,
        problems
            .kind_blank
            .then(|| lang.text(Msg::RuleSuggestionsProblemNoKind)),
        text_field(&form.kind, KIND_FIELD_ID, RuleFormMessage::Kind),
        palette,
    ));
    rows.push(field_row(
        lang.text(Msg::ReviewListFieldTags),
        Some(lang.text(Msg::ReviewListHintTags)),
        None,
        text_field(&form.tags, TAGS_FIELD_ID, RuleFormMessage::Tags),
        palette,
    ));
    let example = form
        .example_path()
        .map(|path| fill_template(lang.text(Msg::RulesFormExample), &[("{path}", &path)]));
    let pattern_problem = pattern_problem_text(&problems, lang);
    let pattern_field = column![
        text_field(&form.pattern, PATTERN_FIELD_ID, RuleFormMessage::Pattern),
        block_buttons(palette),
    ]
    .spacing(spacing::S);
    rows.push(field_row(
        lang.text(Msg::ReviewListFieldPattern),
        example.as_deref(),
        pattern_problem.as_deref(),
        pattern_field.into(),
        palette,
    ));
    rows
}

fn block_buttons<'a>(palette: &Palette) -> Element<'a, Message> {
    FILING_PLACEHOLDERS
        .chunks(BLOCKS_PER_ROW)
        .fold(Column::new().spacing(spacing::XS), |lines, names| {
            lines.push(
                row(names.iter().map(|&name| {
                    kit::button(
                        &format!("{{{name}}}"),
                        Some(send_form(RuleFormMessage::InsertBlock(name))),
                        Tone::Secondary,
                        palette,
                    )
                }))
                .spacing(spacing::XS),
            )
        })
        .into()
}

fn pattern_problem_text(problems: &RuleFormProblems, lang: Language) -> Option<String> {
    Some(match problems.pattern.as_ref()? {
        PatternProblem::Blank => lang.text(Msg::RulesFormProblemPatternBlank).to_owned(),
        PatternProblem::UnknownBlock(name) => fill_template(
            lang.text(Msg::RulesFormProblemUnknownBlock),
            &[("{block}", name)],
        ),
        PatternProblem::Invalid => lang
            .text(Msg::RuleSuggestionsProblemInvalidPattern)
            .to_owned(),
    })
}

fn try_section<'a>(
    form: &RuleForm,
    line_chars: usize,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut content = Column::new()
        .push(dim(lang.text(Msg::RulesTryExplain), palette))
        .spacing(spacing::XS);
    match &form.live_try.status {
        TryStatus::Idle => {}
        TryStatus::Waiting | TryStatus::Running => {
            content = content.push(dim(lang.text(Msg::RulesTryLooking), palette));
        }
        TryStatus::Failed(detail) => {
            content = content.push(error_line(
                fill_template(lang.text(Msg::RulesTryFailed), &[("{detail}", detail)]),
                palette,
            ));
        }
        TryStatus::Ready(result) if result.documents.is_empty() => {
            content = content.push(dim(lang.text(Msg::RulesTryEmpty), palette));
        }
        TryStatus::Ready(result) => {
            content = try_rows(content, result, line_chars, lang, palette);
        }
    }
    kit::section(lang.text(Msg::RulesTryHeading), content, palette)
}

fn try_rows<'a>(
    intro: Column<'a, Message>,
    result: &TryResult,
    line_chars: usize,
    lang: Language,
    palette: &Palette,
) -> Column<'a, Message> {
    let columns = Columns {
        portions: &TRY_PORTIONS,
        line_chars,
    };
    let mut content = result
        .documents
        .iter()
        .take(TRY_VISIBLE_ROWS)
        .fold(intro, |lines, tried| {
            lines.push(try_row(&columns, tried, result, lang, palette))
        });
    let hidden = result
        .documents
        .len()
        .saturating_sub(TRY_VISIBLE_ROWS)
        .saturating_add(result.more);
    if hidden > 0 {
        content = content.push(dim(
            fill_template(
                lang.text(Msg::RulesTryMore),
                &[("{count}", &hidden.to_string())],
            ),
            palette,
        ));
    }
    content
}

fn try_row<'a>(
    columns: &Columns<'_>,
    tried: &TriedDocument,
    result: &TryResult,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let section = lang.text(match tried.section {
        TrySection::Inbox => Msg::RulesTrySectionInbox,
        TrySection::Review => Msg::RulesTrySectionReview,
        TrySection::Archive => Msg::RulesTrySectionArchive,
    });
    let change = format!(
        "{} → {}",
        outcome_text(&tried.now, lang),
        outcome_text(&tried.after, lang)
    );
    let places: Vec<String> = tried
        .conflicts
        .iter()
        .filter_map(|id| result.conflict_names.get(id))
        .map(ToString::to_string)
        .collect();
    let conflicts = if places.is_empty() {
        String::new()
    } else {
        fill_template(
            lang.text(Msg::RulesTryAlso),
            &[("{rules}", &places.join(", "))],
        )
    };
    row![
        columns.cell(0, section, palette.dim_text),
        columns.cell(1, &tried.path.display().to_string(), palette.text),
        columns.cell(2, &change, palette.text),
        columns.cell(3, &conflicts, palette.error),
    ]
    .into()
}

fn outcome_text(outcome: &TryOutcome, lang: Language) -> String {
    match outcome {
        TryOutcome::Filed(path) => path.display().to_string(),
        TryOutcome::Review(reason) => reason_text(*reason, TextState::Read, lang),
        TryOutcome::Lies(path) => fill_template(
            lang.text(Msg::RulesTryLiesAt),
            &[("{path}", &path.display().to_string())],
        ),
    }
}
