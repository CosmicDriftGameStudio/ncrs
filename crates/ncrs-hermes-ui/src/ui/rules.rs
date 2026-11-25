//! The dialog in which one rule is made or changed, and the question before
//! one is removed. Pure view over [`RulesPage`].

use chrono::NaiveDate;
use iced::widget::{Column, Row};
use iced::{alignment, Element, Length, Size};
use ncrs_config::config::{Filing, TargetSetting};
use ncrs_config::palette::Palette;
use ncrs_filing::rules_work::{SortTryRow, SortTryState};
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_index::{DocumentFields, Kinds};
use ncrs_intake::{render_pattern_in, FILING_PLACEHOLDERS};
use ncrs_ui_kit::format;
use ncrs_ui_kit::frame::{DialogSize, Frame, KeyHints};
use ncrs_ui_kit::hermes_kit::{self as page_kit, ChipKind, MenuItem, RowTwo, GAP_S, GAP_XS};
use ncrs_ui_kit::kit::{self, PatternField, Tone};

use super::condition_editor::{
    add_condition_link, combinator_choice, condition_editor_rows, ConditionHost,
};
use crate::message::{Message, RuleFormMessage, RulesMessage};
use crate::rule_form::{chip_text, RuleForm, RuleFormTarget, TryStatus};
use crate::rules_page::{rule_sentence, RulesPage};

/// Characters of a path of the try, so that it stays readable in the wide
/// dialog.
const TRY_PATH_CHARS: usize = 64;

/// Files of the try listed at once.
const TRY_VISIBLE_ROWS: usize = 8;

const TARGET_DROPDOWN_WIDTH: f32 = 320.0;

pub const NAME_FIELD_ID: &str = "rule-form-name";
pub const TAG_FIELD_ID: &str = "rule-form-tag";
const PATTERN_FIELD_ID: &str = "rule-form-pattern";

fn send(message: RulesMessage) -> Message {
    Message::Rules(message)
}

fn send_form(message: RuleFormMessage) -> Message {
    send(RulesMessage::Form(message))
}

fn problem_text(raw_detail: &str, lang: Language) -> String {
    let detail = if raw_detail == ncrs_core::INDEX_NOT_OPEN {
        lang.text(Msg::IndexNotOpen)
    } else {
        raw_detail
    };
    fill_template(lang.text(Msg::RulesProblemFailed), &[("{detail}", detail)])
}

/// The dialog that is up: the question before a rule is deleted, or the
/// form of a rule.
pub fn dialog<'a>(
    state: &'a RulesPage,
    filing: &Filing,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Option<Element<'a, Message>> {
    if state.is_confirming_delete() {
        return Some(delete_dialog(state, window, lang, palette));
    }
    state
        .form()
        .map(|form| form_dialog(state, form, filing, window, lang, palette))
}

/// The question whether the rule in the form is removed; `None` while that
/// cannot be asked.
fn remove_message(state: &RulesPage, form: &RuleForm) -> Option<RulesMessage> {
    let index = state.edited_index()?;
    (!state.is_busy() && !form.saving).then_some(RulesMessage::AskDelete(index))
}

fn delete_dialog<'a>(
    state: &RulesPage,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let rule = state
        .selected()
        .map(|listed| rule_sentence(&listed.rule.spec, lang, state.kinds()))
        .unwrap_or_default();
    let question = fill_template(lang.text(Msg::RulesDeleteQuestion), &[("{rule}", &rule)]);
    let mut body = Column::new().spacing(GAP_S);
    if let Some(detail) = state.problem() {
        body = body.push(page_kit::error_line(problem_text(detail, lang), palette));
    }
    body = body.push(kit::paragraph(question, palette));
    Frame::new(
        lang.text(Msg::RulesDeleteConfirm),
        body,
        DialogSize::Small,
        window,
        lang,
        palette,
    )
    .buttons(vec![
        kit::button(
            lang.text(Msg::RulesCancel),
            Some(send(RulesMessage::CancelDelete)),
            Tone::Secondary,
            palette,
        ),
        kit::button(
            lang.text(Msg::RulesDeleteConfirm),
            Some(send(RulesMessage::ConfirmDelete)),
            Tone::Danger,
            palette,
        ),
    ])
    .on_close(send(RulesMessage::CancelDelete))
    .hints(KeyHints {
        enter: false,
        tab: false,
    })
    .into_element()
}

fn form_dialog<'a>(
    state: &'a RulesPage,
    form: &'a RuleForm,
    filing: &Filing,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let title = match form.target {
        RuleFormTarget::FromSuggestion(_) => Msg::RulesFormTitleConfirm,
        RuleFormTarget::New | RuleFormTarget::FromDocument => Msg::RulesFormTitleNew,
        RuleFormTarget::Edit(_) => Msg::HermesPagesRuleTitleEdit,
    };
    let save_label = match form.target {
        RuleFormTarget::FromSuggestion(_) => Msg::RuleSuggestionsConfirm,
        RuleFormTarget::New | RuleFormTarget::FromDocument | RuleFormTarget::Edit(_) => {
            Msg::RulesFormSave
        }
    };
    let mut body = Column::new().spacing(GAP_S);
    if let Some(detail) = state.problem() {
        body = body.push(page_kit::error_line(problem_text(detail, lang), palette));
    }
    for element in form_rows(form, filing, lang, palette) {
        body = body.push(element);
    }
    for element in try_rows(form, filing, lang, palette) {
        body = body.push(element);
    }
    if state.edited_index().is_some() {
        body = body.push(kit::button(
            lang.text(Msg::SortingRemove),
            remove_message(state, form).map(send),
            Tone::Danger,
            palette,
        ));
    }
    Frame::new(
        lang.text(title),
        body,
        DialogSize::Wide,
        window,
        lang,
        palette,
    )
    .buttons(vec![
        kit::button(
            lang.text(Msg::RulesFormCancel),
            Some(send_form(RuleFormMessage::Cancel)),
            Tone::Secondary,
            palette,
        ),
        kit::button(
            lang.text(save_label),
            (form.is_changed() && !form.saving).then(|| send_form(RuleFormMessage::Save)),
            Tone::Primary,
            palette,
        ),
    ])
    .on_close(send_form(RuleFormMessage::Cancel))
    .hints(KeyHints {
        enter: false,
        tab: false,
    })
    .into_element()
}

fn form_rows<'a>(
    form: &'a RuleForm,
    filing: &Filing,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let problems = form.problems();
    let save = send_form(RuleFormMessage::Save);
    let host = ConditionHost {
        wrap: |edit| send_form(RuleFormMessage::Condition(edit)),
        on_submit: &save,
        lang,
        palette,
    };
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    if let Some(detail) = &form.failure {
        rows.push(page_kit::error_line(problem_text(detail, lang), palette));
    }
    rows.push(page_kit::form_row(
        lang.text(Msg::RulesFormName),
        page_kit::text_field(
            "",
            &form.name,
            NAME_FIELD_ID,
            Some(|typed| send_form(RuleFormMessage::Name(typed))),
            save.clone(),
            palette,
        ),
        None,
        palette,
    ));

    rows.push(page_kit::section(
        lang.text(Msg::ConditionEditorHeading),
        palette,
    ));
    if form.conditions.rows.len() > 1 {
        rows.push(full_width(
            combinator_choice(form.conditions.combinator, &host),
            None,
        ));
    }
    let condition_rows = condition_editor_rows(&form.conditions, &host);
    let last = condition_rows.len().saturating_sub(1);
    for (position, (problem, control)) in condition_rows.into_iter().enumerate() {
        let missing = (position == last && problems.no_condition)
            .then(|| lang.text(Msg::RulesFormProblemNoCondition).to_owned());
        let note = problem
            .or(missing)
            .map(|text| page_kit::error_line(text, palette));
        rows.push(full_width(control, note));
    }
    if form.conditions.rows.is_empty() {
        rows.push(full_width(
            page_kit::hint(lang.text(Msg::ConditionEditorEmpty), palette),
            problems.no_condition.then(|| {
                page_kit::error_line(lang.text(Msg::RulesFormProblemNoCondition), palette)
            }),
        ));
    }
    rows.push(full_width(
        add_condition_link(&form.conditions, &host),
        Some(page_kit::hint(lang.text(Msg::ConditionEditorHelp), palette)),
    ));

    rows.push(page_kit::section(lang.text(Msg::SortingRuleThen), palette));
    rows.push(destination_row(form, filing, lang, palette));
    rows.push(pattern_row(form, lang, palette));
    rows.push(tags_row(form, lang, palette));
    rows
}

/// A control over the whole width of the dialog, with its note under it: the
/// conditions need the room that a label column would take.
fn full_width<'a>(
    control: Element<'a, Message>,
    note: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut column = Column::new()
        .push(control)
        .spacing(GAP_XS)
        .width(Length::Fill);
    if let Some(extra) = note {
        column = column.push(extra);
    }
    column.into()
}

fn tags_row<'a>(form: &RuleForm, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let mut items: Vec<Element<'a, Message>> = form
        .tags
        .iter()
        .map(|tag| {
            page_kit::chip(
                tag,
                ChipKind::Own,
                Some(send_form(RuleFormMessage::RemoveTag(tag.clone()))),
                palette,
            )
        })
        .collect();
    if form.tag_input_open {
        items.push(page_kit::text_field(
            lang.text(Msg::RulesFormTagsAdd),
            &form.tag_input,
            TAG_FIELD_ID,
            Some(|typed| send_form(RuleFormMessage::TagInput(typed))),
            send_form(RuleFormMessage::AddTag),
            palette,
        ));
    } else {
        items.push(kit::button(
            lang.text(Msg::RulesFormTagsAdd),
            Some(send_form(RuleFormMessage::OpenTagInput)),
            Tone::Secondary,
            palette,
        ));
        items.push(page_kit::value_dim(
            lang.text(Msg::RulesFormTagsOptional),
            palette,
        ));
    }
    page_kit::form_row(
        lang.text(Msg::RulesFormTags),
        Row::with_children(items)
            .spacing(GAP_S)
            .align_y(alignment::Vertical::Center)
            .wrap()
            .vertical_spacing(GAP_XS)
            .into(),
        None,
        palette,
    )
}

/// The files the rule as it stands in the form would sort, in the target it is
/// set to.
fn try_rows<'a>(
    form: &RuleForm,
    filing: &Filing,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let chip = filing
        .targets
        .iter()
        .position(|setting| setting.target.id == form.destination)
        .map(|index| format!("T{}", index + 1));
    let mut rows: Vec<Element<'a, Message>> = vec![
        page_kit::section(lang.text(Msg::RulesTryHeading), palette),
        page_kit::hint(lang.text(Msg::RulesTryExplain), palette),
    ];
    match &form.live_try.status {
        TryStatus::Idle => {}
        TryStatus::Waiting | TryStatus::Running => {
            rows.push(page_kit::hint(lang.text(Msg::RulesTryLooking), palette));
        }
        TryStatus::Failed(detail) => rows.push(page_kit::error_line(
            fill_template(
                lang.text(Msg::RulesTryFailed),
                &[("{detail}", &format::ellipsize_end(detail, TRY_PATH_CHARS))],
            ),
            palette,
        )),
        TryStatus::Ready(tried) if tried.rows.is_empty() && tried.no_text == 0 => {
            if form.is_valid() {
                rows.push(page_kit::hint(lang.text(Msg::RulesTryEmpty), palette));
            }
        }
        TryStatus::Ready(tried) => {
            for row in tried.rows.iter().take(TRY_VISIBLE_ROWS) {
                rows.push(try_row(row, chip.clone(), lang, palette));
            }
            let hidden = tried
                .rows
                .len()
                .saturating_sub(TRY_VISIBLE_ROWS)
                .saturating_add(tried.more);
            if hidden > 0 {
                rows.push(page_kit::hint(
                    fill_template(
                        lang.text(Msg::RulesTryMore),
                        &[("{count}", &hidden.to_string())],
                    ),
                    palette,
                ));
            }
            if tried.no_text > 0 {
                rows.push(page_kit::hint(
                    fill_template(
                        lang.text(Msg::SortingTryNoText),
                        &[("{count}", &tried.no_text.to_string())],
                    ),
                    palette,
                ));
            }
        }
    }
    rows
}

fn try_row<'a>(
    row: &SortTryRow,
    chip: Option<String>,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    page_kit::list_row_two(
        RowTwo {
            primary: format::ellipsize_middle(&row.relative.display().to_string(), TRY_PATH_CHARS),
            secondary: try_state_text(&row.state, lang),
            right: chip,
            lead: None,
            muted: false,
        },
        false,
        None,
        palette,
    )
}

fn try_state_text(state: &SortTryState, lang: Language) -> String {
    match state {
        SortTryState::Filed(path) => format!(
            "→ {}",
            format::ellipsize_middle(&path.display().to_string(), TRY_PATH_CHARS)
        ),
        SortTryState::MissingValue(field) => fill_template(
            lang.text(Msg::SortingStateNoDate),
            &[("{block}", &chip_text(field.as_str()))],
        ),
        SortTryState::BadName => lang.text(Msg::SortingTryBadName).to_owned(),
        SortTryState::NameTaken(path) => fill_template(
            lang.text(Msg::SortingTryNameTaken),
            &[("{path}", &path.display().to_string())],
        ),
    }
}

/// The path the pattern makes of the first file the trial really sorted, or of
/// an example document when there is none.
fn pattern_result(form: &RuleForm, lang: Language) -> Option<String> {
    let from_trial = match &form.live_try.status {
        TryStatus::Ready(tried) => tried.rows.iter().find_map(|row| match &row.state {
            SortTryState::Filed(path) => Some(path.display().to_string()),
            _ => None,
        }),
        _ => None,
    };
    from_trial.or_else(|| example_path(&form.pattern, lang))
}

fn example_path(pattern: &str, lang: Language) -> Option<String> {
    let fields = DocumentFields {
        sender: Some("Telekom".to_owned()),
        recipient: Some(lang.text(Msg::FilingPiecesExampleRecipient).to_owned()),
        kind: Some(lang.text(Msg::FilingPiecesExampleKind).to_owned()),
        date: NaiveDate::from_ymd_opt(2026, 3, 9),
        number: Some("RE-42".to_owned()),
        ..DocumentFields::default()
    };
    let path = render_pattern_in(pattern, &fields, &Kinds::built_in(lang.kind_language()))?;
    let parts: Vec<_> = path
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect();
    Some(format!("{}.pdf", parts.join("/")))
}

/// "T2 · Company": the chip's number and what the target is called.
fn target_label(index: usize, setting: &TargetSetting) -> String {
    let target = &setting.target;
    let name = if target.name.is_empty() {
        target
            .path
            .file_name()
            .map_or_else(String::new, |last| last.to_string_lossy().into_owned())
    } else {
        target.name.clone()
    };
    format!("T{} · {name}", index + 1)
}

fn destination_row<'a>(
    form: &RuleForm,
    filing: &Filing,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let items: Vec<MenuItem<Message>> = filing
        .targets
        .iter()
        .enumerate()
        .map(|(index, setting)| {
            MenuItem::new(
                target_label(index, setting),
                send_form(RuleFormMessage::Destination(setting.target.id.clone())),
            )
        })
        .collect();
    let selected = filing
        .targets
        .iter()
        .position(|setting| setting.target.id == form.destination)
        .unwrap_or(0);
    page_kit::form_row(
        lang.text(Msg::SortingRuleTarget),
        page_kit::field_dropdown(TARGET_DROPDOWN_WIDTH, &items, selected, palette),
        None,
        palette,
    )
}

fn pattern_row<'a>(form: &'a RuleForm, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let tokens: Vec<(String, Message)> = FILING_PLACEHOLDERS
        .iter()
        .map(|&name| {
            (
                chip_text(name),
                send_form(RuleFormMessage::PatternChip(name)),
            )
        })
        .collect();
    let labelled: Vec<(&str, Message)> = tokens
        .iter()
        .map(|(label, message)| (label.as_str(), message.clone()))
        .collect();
    let problem = form.problems().unknown_placeholder.map(|name| {
        fill_template(
            lang.text(Msg::SortingPatternUnknownBlock),
            &[("{block}", &chip_text(&name))],
        )
    });
    let preview = if problem.is_none() {
        pattern_result(form, lang).map(|path| {
            fill_template(lang.text(Msg::SortingPatternResult), &[("{path}", &path)])
        })
    } else {
        None
    };
    page_kit::form_row(
        lang.text(Msg::SortingRulePattern),
        kit::pattern_field(
            PatternField {
                value: &form.pattern,
                id: PATTERN_FIELD_ID,
                on_input: Box::new(|typed| send_form(RuleFormMessage::Pattern(typed))),
                on_submit: send_form(RuleFormMessage::Save),
                tokens: &labelled,
                preview,
                problem,
            },
            palette,
        ),
        None,
        palette,
    )
}

#[cfg(test)]
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
mod tests {
    use super::*;
    use crate::rules_page::fixtures::page;
    use ncrs_index::TargetId;

    #[test]
    fn the_dialog_of_a_listed_rule_asks_before_it_removes_that_rule() {
        let mut rules = page();
        rules.begin_edit(1);
        let form = rules.form().unwrap();

        assert_eq!(
            remove_message(&rules, form),
            Some(RulesMessage::AskDelete(1))
        );

        rules.form_mut().unwrap().saving = true;
        let saving = rules.form().unwrap();
        assert_eq!(remove_message(&rules, saving), None);
    }

    #[test]
    fn the_dialog_of_a_new_rule_removes_nothing() {
        let mut rules = page();
        rules.begin_new_in(TargetId::main());
        let form = rules.form().unwrap();

        assert_eq!(remove_message(&rules, form), None);
    }
}
