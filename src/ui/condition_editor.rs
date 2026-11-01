//! The rows of a [`ConditionEditor`]: kind, has / has not, value and a cross,
//! and the link that adds a condition. The combinator is offered
//! apart by [`combinator_choice`]. The host (the
//! rule form) lays the rows out in its own row type.

use iced::widget::row;
use iced::{alignment, Element};
use ncrs_index::{Combinator, Condition, ConditionKind};

use super::hermes_kit::{self as page_kit, MenuItem};
use super::kit;
use super::theme::spacing;
use crate::app::fill_template;
use crate::condition_editor::{
    ConditionEdit, ConditionEditor, ConditionProblem, MAX_CONDITION_ROWS,
};
use crate::i18n::{Language, Msg};
use crate::messages::Message;
use crate::palette::Palette;

pub const CONDITION_VALUE_FIELD_IDS: [&str; MAX_CONDITION_ROWS] = [
    "condition-value-0",
    "condition-value-1",
    "condition-value-2",
    "condition-value-3",
    "condition-value-4",
    "condition-value-5",
    "condition-value-6",
    "condition-value-7",
    "condition-value-8",
    "condition-value-9",
    "condition-value-10",
    "condition-value-11",
];

const REMOVE_ICON: &str = "×";

/// The room of the kind dropdown in a row, in pixels.
const KIND_WIDTH: f32 = 128.0;

/// The room of the has / has not dropdown in a row, in pixels.
const NEGATION_WIDTH: f32 = 104.0;

/// What is wrong with the row, and the control.
pub type ConditionRow<'a> = (Option<String>, Element<'a, Message>);

/// How the rows send their edits and what Enter in a value field does.
pub struct ConditionHost<'p> {
    pub wrap: fn(ConditionEdit) -> Message,
    pub on_submit: &'p Message,
    pub lang: Language,
    pub palette: &'p Palette,
}

pub fn condition_editor_rows<'a>(
    editor: &ConditionEditor,
    host: &ConditionHost<'_>,
) -> Vec<ConditionRow<'a>> {
    editor
        .rows
        .iter()
        .zip(editor.problems())
        .zip(CONDITION_VALUE_FIELD_IDS)
        .enumerate()
        .map(|(index, ((condition, problem), id))| {
            (
                problem.map(|found| problem_text(&found, host.lang)),
                condition_row(editor, index, condition, id, host),
            )
        })
        .collect()
}

/// The link under the rows that adds a condition.
pub fn add_condition_link<'a>(
    editor: &ConditionEditor,
    host: &ConditionHost<'_>,
) -> Element<'a, Message> {
    page_kit::link(
        host.lang.text(Msg::ConditionEditorAdd),
        editor.can_add().then(|| (host.wrap)(ConditionEdit::Add)),
        host.palette,
    )
}

fn condition_row<'a>(
    editor: &ConditionEditor,
    index: usize,
    condition: &Condition,
    id: &'static str,
    host: &ConditionHost<'_>,
) -> Element<'a, Message> {
    let ConditionHost {
        wrap,
        on_submit,
        lang,
        palette,
    } = *host;
    let choices = editor.kind_choices(index);
    let kinds: Vec<MenuItem<Message>> = choices
        .iter()
        .map(|&kind| {
            MenuItem::new(
                lang.text(kit::condition_kind_name(kind)),
                wrap(ConditionEdit::Kind(index, kind)),
            )
        })
        .collect();
    let selected = choices
        .iter()
        .position(|&kind| kind == condition.kind)
        .unwrap_or_default();
    let on_input = move |typed| wrap(ConditionEdit::Value(index, typed));
    let value_field = if condition.kind == ConditionKind::Regex {
        page_kit::mono_text_field(
            "",
            &condition.value,
            id,
            Some(on_input),
            on_submit.clone(),
            palette,
        )
    } else {
        page_kit::text_field(
            "",
            &condition.value,
            id,
            Some(on_input),
            on_submit.clone(),
            palette,
        )
    };
    let mut cells = row![page_kit::dropdown(KIND_WIDTH, &kinds, selected, palette)]
        .spacing(spacing::S)
        .align_y(alignment::Vertical::Center);
    if editor.negation {
        let has = [
            (lang.text(Msg::ConditionEditorHas), false),
            (lang.text(Msg::ConditionEditorHasNot), true),
        ];
        let operators: Vec<MenuItem<Message>> = has
            .iter()
            .map(|&(label, negated)| {
                MenuItem::new(label, wrap(ConditionEdit::Negated(index, negated)))
            })
            .collect();
        let has_selected = has
            .iter()
            .position(|&(_, negated)| negated == condition.negated)
            .unwrap_or_default();
        cells = cells.push(page_kit::dropdown(
            NEGATION_WIDTH,
            &operators,
            has_selected,
            palette,
        ));
    }
    cells
        .push(value_field)
        .push(page_kit::icon_button(
            REMOVE_ICON,
            wrap(ConditionEdit::Remove(index)),
            palette,
        ))
        .into()
}

pub fn combinator_choice<'a>(chosen: Combinator, host: &ConditionHost<'_>) -> Element<'a, Message> {
    let choices = [
        (Msg::ConditionEditorCombinatorAll, Combinator::AllOf),
        (Msg::ConditionEditorCombinatorAny, Combinator::AnyOf),
    ];
    let options: Vec<(&str, Message)> = choices
        .iter()
        .map(|&(label, combinator)| {
            (
                host.lang.text(label),
                (host.wrap)(ConditionEdit::Combinator(combinator)),
            )
        })
        .collect();
    let selected = choices
        .iter()
        .position(|&(_, combinator)| combinator == chosen)
        .unwrap_or_default();
    page_kit::segmented(&options, selected, host.palette)
}

fn problem_text(problem: &ConditionProblem, lang: Language) -> String {
    match problem {
        ConditionProblem::Empty => lang.text(Msg::ConditionEditorProblemEmpty).to_owned(),
        ConditionProblem::InvalidRegex(reason) => fill_template(
            lang.text(Msg::ConditionEditorProblemRegex),
            &[("{reason}", reason)],
        ),
        ConditionProblem::InvalidIban => lang.text(Msg::ConditionEditorProblemIban).to_owned(),
    }
}
