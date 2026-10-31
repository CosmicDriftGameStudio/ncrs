//! The rows of a [`ConditionEditor`]: kind, value, "not", remove, and under
//! them the combinator and the button that adds a condition. The hosts (the
//! rule form, the tax contexts of the settings) lay the rows out in their own
//! row type.

use iced::widget::{container, row};
use iced::{alignment, Element};
use ncrs_index::{Combinator, Condition, ConditionKind};

use super::kit::{self, dim, Check, Tone};
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

pub const FIRST_CONDITION_FIELD_ID: &str = "condition-value-0";

/// The room of the kind dropdown in a row, in pixels.
const KIND_WIDTH: f32 = 160.0;

/// A label, what is wrong with the row and the control.
pub type ConditionRow<'a> = (String, Option<String>, Element<'a, Message>);

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
    let ConditionHost { wrap, lang, .. } = *host;
    let mut rows: Vec<ConditionRow<'a>> = editor
        .rows
        .iter()
        .zip(editor.problems())
        .zip(CONDITION_VALUE_FIELD_IDS)
        .enumerate()
        .map(|(index, ((condition, problem), id))| {
            (
                fill_template(
                    lang.text(Msg::ConditionEditorRowLabel),
                    &[("{number}", &(index + 1).to_string())],
                ),
                problem.map(|found| problem_text(&found, lang)),
                condition_row(editor, index, condition, id, host),
            )
        })
        .collect();
    if editor.rows.is_empty() {
        rows.push((
            lang.text(Msg::ConditionEditorEmpty).to_owned(),
            None,
            container("").into(),
        ));
    }
    if editor.rows.len() > 1 {
        rows.push((
            lang.text(Msg::ConditionEditorCombinatorLabel).to_owned(),
            None,
            combinator_choice(editor.combinator, host),
        ));
    }
    rows.push((
        String::new(),
        None,
        kit::button(
            lang.text(Msg::ConditionEditorAdd),
            editor.can_add().then(|| wrap(ConditionEdit::Add)),
            Tone::Secondary,
            host.palette,
        ),
    ));
    rows
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
    let options: Vec<(&str, Message)> = choices
        .iter()
        .map(|&kind| {
            (
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
        kit::mono_text_field(
            "",
            &condition.value,
            id,
            Some(on_input),
            on_submit.clone(),
            palette,
        )
    } else {
        kit::text_field(
            "",
            &condition.value,
            id,
            Some(on_input),
            on_submit.clone(),
            palette,
        )
    };
    let mut cells = row![
        container(kit::dropdown(&options, selected, true, palette)).width(KIND_WIDTH),
        value_field,
    ]
    .spacing(spacing::S)
    .align_y(alignment::Vertical::Center);
    if editor.negation {
        cells = cells.push(
            row![
                kit::checkbox(
                    if condition.negated {
                        Check::On
                    } else {
                        Check::Off
                    },
                    Some(wrap(ConditionEdit::Negated(index, !condition.negated))),
                    palette,
                ),
                dim(lang.text(Msg::ConditionEditorNot), palette),
            ]
            .spacing(spacing::XS)
            .align_y(alignment::Vertical::Center),
        );
    }
    cells
        .push(kit::button(
            lang.text(Msg::ConditionEditorRemove),
            Some(wrap(ConditionEdit::Remove(index))),
            Tone::Secondary,
            palette,
        ))
        .into()
}

fn combinator_choice<'a>(chosen: Combinator, host: &ConditionHost<'_>) -> Element<'a, Message> {
    let choices = [
        (Msg::ConditionEditorCombinatorAny, Combinator::AnyOf),
        (Msg::ConditionEditorCombinatorAll, Combinator::AllOf),
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
    kit::segmented(&options, selected, true, host.palette)
}

fn problem_text(problem: &ConditionProblem, lang: Language) -> String {
    match problem {
        ConditionProblem::Empty => lang.text(Msg::ConditionEditorProblemEmpty).to_owned(),
        ConditionProblem::InvalidRegex(reason) => fill_template(
            lang.text(Msg::ConditionEditorProblemRegex),
            &[("{reason}", reason)],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_id_is_the_first_of_the_fixed_ids() {
        assert_eq!(
            CONDITION_VALUE_FIELD_IDS.first().copied(),
            Some(FIRST_CONDITION_FIELD_ID)
        );
    }
}
