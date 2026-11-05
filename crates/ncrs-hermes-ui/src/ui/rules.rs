//! The rules page of the settings: every confirmed rule as a row of the list,
//! in the order Hermes tries them in, and the small dialog in which one rule
//! is made or changed. Pure view over [`RulesPage`].

use iced::widget::{Column, Row};
use iced::{alignment, Element, Length, Size};
use ncrs_config::palette::Palette;
use ncrs_filing::review_work::TextState;
use ncrs_filing::rules_work::{ListedRule, TriedDocument, TryOutcome, TryResult, TrySection};
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_ui_kit::format;
use ncrs_ui_kit::frame::{DialogSize, Frame, KeyHints};
use ncrs_ui_kit::hermes_kit::{self as page_kit, ChipKind, RowTwo, GAP_S, GAP_XS};
use ncrs_ui_kit::kit::{self, Check, Tone};
use ncrs_ui_kit::setting_rows::{Group, SettingRow};

use super::condition_editor::{
    add_condition_link, combinator_choice, condition_editor_rows, ConditionHost,
};
use super::filing_pieces::{target_view, ExampleLine, TargetHost};
use super::review_reason::reason_text;
use crate::message::{Message, RuleFormMessage, RulesMessage};
use crate::rule_form::{RuleForm, RuleFormTarget, TryStatus};
use crate::rules_page::{hits_text, origin_text, rule_sentence, RulesPage};

/// Characters of a document of the try and of each side of its change, so
/// that the target stays readable in the wide dialog.
const TRY_PATH_CHARS: usize = 64;
const TRY_CHANGE_SIDE_CHARS: usize = 36;

/// Documents of the try listed at once.
const TRY_VISIBLE_ROWS: usize = 8;

pub const NAME_FIELD_ID: &str = "rule-form-name";
pub const TAG_FIELD_ID: &str = "rule-form-tag";
const TARGET_NAME_FIELD_ID: &str = "rule-form-folder-name";

fn send(message: RulesMessage) -> Message {
    Message::Rules(message)
}

fn send_form(message: RuleFormMessage) -> Message {
    send(RulesMessage::Form(message))
}

/// The sections of the page: the list with the button under it.
pub fn groups<'a>(
    state: &'a RulesPage,
    lang: Language,
    palette: &Palette,
) -> Vec<Group<'a, Message>> {
    let idle = !state.is_busy();
    let count = state.rows().len();
    let mut rows: Vec<SettingRow<'a, Message>> = state
        .rows()
        .iter()
        .enumerate()
        .map(|(index, listed)| rule_row(index, count, listed, state, lang, palette))
        .collect();
    let empty = if count > 0 {
        None
    } else if state.is_busy() {
        Some(lang.text(Msg::RulesLoading).to_owned())
    } else {
        Some(lang.text(Msg::RulesEmpty).to_owned())
    };
    let problem = state
        .problem()
        .filter(|_| !state.dialog_is_open())
        .map(|detail| problem_text(detail, lang));
    rows.push(SettingRow::adding(
        empty,
        problem,
        vec![kit::button(
            lang.text(Msg::HermesPagesRuleAdd),
            idle.then(|| send(RulesMessage::New)),
            Tone::Secondary,
            palette,
        )],
    ));
    vec![Group {
        intro: Some(lang.text(Msg::RulesExplain).to_owned()),
        title: String::new(),
        rows,
    }]
}

fn rule_row<'a>(
    index: usize,
    count: usize,
    listed: &ListedRule,
    state: &RulesPage,
    lang: Language,
    palette: &Palette,
) -> SettingRow<'a, Message> {
    let spec = &listed.rule.spec;
    let sentence = rule_sentence(spec, lang, state.kinds());
    let named = spec.name.as_deref().filter(|name| !name.is_empty());
    let (label, second) = match named {
        Some(name) => (name.to_owned(), sentence),
        None => (sentence, origin_text(listed.rule.origin, lang).to_owned()),
    };
    let idle = !state.is_busy();
    let status = if spec.enabled {
        hits_text(listed.hits, lang)
    } else {
        lang.text(Msg::RulesOff).to_owned()
    };
    let actions = vec![
        kit::checkbox(
            if spec.enabled { Check::On } else { Check::Off },
            idle.then(|| send(RulesMessage::ToggleAt(index))),
            palette,
        ),
        kit::button(
            "↑",
            (idle && index > 0).then(|| send(RulesMessage::ReorderAt { index, delta: -1 })),
            Tone::Secondary,
            palette,
        ),
        kit::button(
            "↓",
            (idle && index + 1 < count).then(|| send(RulesMessage::ReorderAt { index, delta: 1 })),
            Tone::Secondary,
            palette,
        ),
        kit::button(
            lang.text(Msg::HermesPagesChange),
            idle.then(|| send(RulesMessage::Edit(index))),
            Tone::Secondary,
            palette,
        ),
        kit::button(
            lang.text(Msg::SettingsRemove),
            idle.then(|| send(RulesMessage::AskDelete(index))),
            Tone::Secondary,
            palette,
        ),
    ];
    SettingRow::listed(label, Some(second), Some(status), actions, palette)
}

fn problem_text(detail: &str, lang: Language) -> String {
    fill_template(lang.text(Msg::RulesProblemFailed), &[("{detail}", detail)])
}

/// The dialog that is up: the question before a rule is deleted, or the
/// form of a rule.
pub fn dialog<'a>(
    state: &'a RulesPage,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Option<Element<'a, Message>> {
    if state.is_confirming_delete() {
        return Some(delete_dialog(state, window, lang, palette));
    }
    state
        .form()
        .map(|form| form_dialog(form, state.problem(), window, lang, palette))
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
    form: &RuleForm,
    problem: Option<&str>,
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
    if let Some(detail) = problem {
        body = body.push(page_kit::error_line(problem_text(detail, lang), palette));
    }
    for element in form_rows(form, lang, palette) {
        body = body.push(element);
    }
    for element in try_rows(form, lang, palette) {
        body = body.push(element);
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

fn form_rows<'a>(form: &RuleForm, lang: Language, palette: &Palette) -> Vec<Element<'a, Message>> {
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
        rows.push(page_kit::error_line(
            fill_template(lang.text(Msg::RulesProblemFailed), &[("{detail}", detail)]),
            palette,
        ));
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

    rows.push(page_kit::section(lang.text(Msg::RulesFormThen), palette));
    rows.push(target_view(
        &form.filing,
        &TargetHost {
            wrap: |edit| send_form(RuleFormMessage::Target(edit)),
            name_field_id: TARGET_NAME_FIELD_ID,
            folder_hint: None,
            example: ExampleLine::UnderFileName,
            lang,
            palette,
        },
    ));
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

/// The documents the rule as it stands in the form would match.
fn try_rows<'a>(form: &RuleForm, lang: Language, palette: &Palette) -> Vec<Element<'a, Message>> {
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
        TryStatus::Ready(result) if result.documents.is_empty() => {
            rows.push(page_kit::hint(lang.text(Msg::RulesTryEmpty), palette));
        }
        TryStatus::Ready(result) => {
            rows.push(page_kit::value(try_summary(result, lang), palette));
            for tried in result.documents.iter().take(TRY_VISIBLE_ROWS) {
                rows.push(try_row(tried, result, lang, palette));
            }
            let hidden = result
                .documents
                .len()
                .saturating_sub(TRY_VISIBLE_ROWS)
                .saturating_add(result.more);
            if hidden > 0 {
                rows.push(page_kit::hint(
                    fill_template(
                        lang.text(Msg::RulesTryMore),
                        &[("{count}", &hidden.to_string())],
                    ),
                    palette,
                ));
            }
        }
    }
    rows
}

/// "Read folders 2 · Review 1 · Read only 11".
fn try_summary(result: &TryResult, lang: Language) -> String {
    let count = |section: TrySection| {
        result
            .documents
            .iter()
            .filter(|tried| tried.section == section)
            .count()
    };
    format!(
        "{} {} · {} {} · {} {}",
        lang.text(Msg::RulesTrySectionInbox),
        count(TrySection::Inbox),
        lang.text(Msg::RulesTrySectionReview),
        count(TrySection::Review),
        lang.text(Msg::RulesTrySectionArchive),
        count(TrySection::Archive),
    )
}

fn try_row<'a>(
    tried: &TriedDocument,
    result: &TryResult,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let after = outcome_text(&tried.after, lang);
    // Where a document lies is its path, which is the first line of the row.
    let change = if matches!(tried.now, TryOutcome::Lies(_)) {
        format!(
            "→ {}",
            format::ellipsize_middle(&after, TRY_CHANGE_SIDE_CHARS.saturating_mul(2))
        )
    } else {
        format!(
            "{} → {}",
            format::ellipsize_middle(&outcome_text(&tried.now, lang), TRY_CHANGE_SIDE_CHARS),
            format::ellipsize_middle(&after, TRY_CHANGE_SIDE_CHARS)
        )
    };
    let places: Vec<String> = tried
        .conflicts
        .iter()
        .filter_map(|id| result.conflict_names.get(id))
        .map(ToString::to_string)
        .collect();
    let conflicts = (!places.is_empty()).then(|| {
        fill_template(
            lang.text(Msg::RulesTryAlso),
            &[("{rules}", &places.join(", "))],
        )
    });
    page_kit::list_row_two(
        RowTwo {
            primary: format::ellipsize_middle(&tried.path.display().to_string(), TRY_PATH_CHARS),
            secondary: change,
            right: conflicts,
            lead: None,
            muted: false,
        },
        false,
        None,
        palette,
    )
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
