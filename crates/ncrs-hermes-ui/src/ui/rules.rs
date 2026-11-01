//! The rules page: every confirmed rule as a row of the list, the form of the
//! selected one beside it. The order is the order Hermes tries the rules in.
//! A page of the Ablage tab. Pure view over [`RulesPage`].

use iced::widget::{row, Row};
use iced::{alignment, Element};
use ncrs_index::KindCatalog;

use super::condition_editor::{
    add_condition_link, combinator_choice, condition_editor_rows, ConditionHost,
};
use super::filing_pieces::{target_view, ExampleLine, TargetHost};
use super::hermes::{self, Badges};
use super::review_list::reason_text;
use crate::message::{HermesPage, Message, RuleFormMessage, RulesMessage};
use crate::rule_form::{RuleForm, RuleFormTarget, TryStatus};
use crate::rules_page::{hits_text, origin_text, rule_sentence, RulesPage, VISIBLE_ROWS};
use ncrs_config::palette::Palette;
use ncrs_filing::review_work::TextState;
use ncrs_filing::rules_work::{ListedRule, TriedDocument, TryOutcome, TryResult, TrySection};
use ncrs_i18n::fill_template;
use ncrs_i18n::{FuturamaName, Language, Msg};
use ncrs_ui_kit::format;
use ncrs_ui_kit::hermes_kit::{
    self as page_kit, ChipKind, MenuItem, MenuLook, RowTwo, GAP_M, GAP_S, GAP_XS,
};
use ncrs_ui_kit::kit::{self, Check};

/// Characters of the name and of the second line that fit a row of the list.
const NAME_CHARS: usize = 32;
const SENTENCE_CHARS: usize = 40;

/// Characters of a document of the try and of its change in the list.
const TRY_PATH_CHARS: usize = 56;
const TRY_CHANGE_CHARS: usize = 44;

/// Documents of the try listed at once.
const TRY_VISIBLE_ROWS: usize = 12;

pub const NAME_FIELD_ID: &str = "rule-form-name";
pub const TAG_FIELD_ID: &str = "rule-form-tag";
const TARGET_NAME_FIELD_ID: &str = "rule-form-folder-name";

const SAVE_KEY: &str = if cfg!(target_os = "macos") {
    "⌘↵"
} else {
    "Ctrl+↵"
};

fn send(message: RulesMessage) -> Message {
    Message::Rules(message)
}

fn send_form(message: RuleFormMessage) -> Message {
    send(RulesMessage::Form(message))
}

pub fn view<'a>(
    state: &'a RulesPage,
    badges: Badges,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let body: Element<'a, Message> = if state.rows().is_empty() && state.form().is_none() {
        let sentence = if state.is_busy() {
            Msg::RulesLoading
        } else {
            Msg::RulesEmpty
        };
        page_kit::centered_note(page_kit::hint(lang.text(sentence), palette))
    } else {
        page_kit::list_and_detail(list(state, lang, palette), detail(state, lang, palette))
    };
    page_kit::tab_page(
        hermes::tab_nav(HermesPage::Rules, badges, lang, palette),
        header(state, lang, palette),
        body,
        Some(action_bar(state, lang, palette)),
        palette,
    )
}

fn header<'a>(state: &RulesPage, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let listed_selected =
        !state.is_busy() && state.selected().is_some() && !state.has_unlisted_form();
    let can_move = |delta: isize| {
        listed_selected
            && state
                .cursor()
                .checked_add_signed(delta)
                .is_some_and(|neighbour| neighbour < state.rows().len())
    };
    let mut items = Vec::new();
    if can_move(-1) {
        items.push(MenuItem {
            key_hint: Some("Alt ↑".to_owned()),
            ..MenuItem::new(lang.text(Msg::RulesMoveUp), send(RulesMessage::Reorder(-1)))
        });
    }
    if can_move(1) {
        items.push(MenuItem {
            key_hint: Some("Alt ↓".to_owned()),
            ..MenuItem::new(
                lang.text(Msg::RulesMoveDown),
                send(RulesMessage::Reorder(1)),
            )
        });
    }
    if listed_selected {
        items.push(MenuItem {
            key_hint: Some("Del".to_owned()),
            ..MenuItem::new(lang.text(Msg::RulesDelete), send(RulesMessage::AskDelete))
        });
    }
    let mut actions = vec![page_kit::secondary(
        lang.text(Msg::RulesNew),
        (!state.is_busy() && !state.has_unsaved_changes()).then(|| send(RulesMessage::New)),
        palette,
    )];
    if !items.is_empty() {
        actions.push(page_kit::menu_button("", &items, MenuLook::Icon, palette));
    }
    page_kit::page_header(
        lang.text(Msg::ActionRules),
        lang.futurama_name(FuturamaName::ActionRules),
        Some(lang.text(Msg::RulesExplain).to_owned()),
        None,
        actions,
        palette,
    )
}

fn list<'a>(state: &RulesPage, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    let unlisted = state.has_unlisted_form();
    if let Some(form) = state.form().filter(|_| unlisted) {
        rows.push(page_kit::list_row_two(
            unlisted_row(form, lang),
            true,
            None,
            palette,
        ));
    }
    let room = VISIBLE_ROWS.saturating_sub(rows.len());
    let first = state.first_visible();
    for (index, listed) in state.rows().iter().enumerate().skip(first).take(room) {
        rows.push(page_kit::list_row_two(
            rule_row(index, listed, lang, state.kinds()),
            index == state.cursor() && !unlisted,
            Some(send(RulesMessage::Select(index))),
            palette,
        ));
    }
    page_kit::list_pane(rows, |delta| send(RulesMessage::Scrolled(delta)), palette)
}

fn unlisted_row(form: &RuleForm, lang: Language) -> RowTwo<Message> {
    let name = form.name.trim();
    RowTwo {
        primary: if name.is_empty() {
            lang.text(Msg::RulesFormTitleNew).to_owned()
        } else {
            format::ellipsize_end(name, NAME_CHARS)
        },
        secondary: match form.target {
            RuleFormTarget::FromSuggestion(_) => lang.text(Msg::RulesFormTitleConfirm).to_owned(),
            RuleFormTarget::New | RuleFormTarget::FromDocument | RuleFormTarget::Edit(_) => {
                String::new()
            }
        },
        right: None,
        lead: None,
        muted: false,
    }
}

fn rule_row(
    index: usize,
    listed: &ListedRule,
    lang: Language,
    kinds: &KindCatalog,
) -> RowTwo<Message> {
    let spec = &listed.rule.spec;
    let sentence = rule_sentence(spec, lang, kinds);
    let named = spec.name.as_deref().filter(|name| !name.is_empty());
    let (primary, secondary) = match named {
        Some(name) => (name.to_owned(), sentence),
        None => (sentence, origin_text(listed.rule.origin, lang).to_owned()),
    };
    RowTwo {
        primary: format::ellipsize_end(&primary, NAME_CHARS),
        secondary: format::ellipsize_end(&secondary, SENTENCE_CHARS),
        right: Some(if spec.enabled {
            hits_text(listed.hits, lang)
        } else {
            lang.text(Msg::RulesOff).to_owned()
        }),
        lead: Some((
            if spec.enabled { Check::On } else { Check::Off },
            send(RulesMessage::ToggleAt(index)),
        )),
        muted: !spec.enabled,
    }
}

fn detail<'a>(state: &RulesPage, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let mut children: Vec<Element<'a, Message>> = Vec::new();
    if state.is_confirming_delete() {
        let rule = state
            .selected()
            .map(|listed| rule_sentence(&listed.rule.spec, lang, state.kinds()))
            .unwrap_or_default();
        children.push(page_kit::error_line(
            fill_template(lang.text(Msg::RulesDeleteQuestion), &[("{rule}", &rule)]),
            palette,
        ));
    }
    if let Some(problem) = state.problem() {
        children.push(page_kit::error_line(
            fill_template(lang.text(Msg::RulesProblemFailed), &[("{detail}", problem)]),
            palette,
        ));
    }
    match state.form() {
        Some(form) if state.is_try_list_open() => children.extend(try_rows(form, lang, palette)),
        Some(form) => children.extend(form_rows(form, lang, palette)),
        None => {}
    }
    page_kit::detail(children)
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
        rows.push(page_kit::form_row(
            "",
            combinator_choice(form.conditions.combinator, &host),
            None,
            palette,
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
        rows.push(page_kit::form_row("", control, note, palette));
    }
    if form.conditions.rows.is_empty() {
        rows.push(page_kit::form_row(
            "",
            page_kit::hint(lang.text(Msg::ConditionEditorEmpty), palette),
            problems.no_condition.then(|| {
                page_kit::error_line(lang.text(Msg::RulesFormProblemNoCondition), palette)
            }),
            palette,
        ));
    }
    rows.push(page_kit::form_row(
        "",
        add_condition_link(&form.conditions, &host),
        Some(page_kit::hint(lang.text(Msg::ConditionEditorHelp), palette)),
        palette,
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
        items.push(page_kit::link(
            lang.text(Msg::RulesFormTagsAdd),
            Some(send_form(RuleFormMessage::OpenTagInput)),
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
        page_kit::link(
            lang.text(Msg::RulesTryBack),
            Some(send(RulesMessage::ToggleTryList)),
            palette,
        ),
        page_kit::section(lang.text(Msg::RulesTryHeading), palette),
        page_kit::hint(lang.text(Msg::RulesTryExplain), palette),
    ];
    match &form.live_try.status {
        TryStatus::Idle => {}
        TryStatus::Waiting | TryStatus::Running => {
            rows.push(page_kit::hint(lang.text(Msg::RulesTryLooking), palette));
        }
        TryStatus::Failed(detail) => rows.push(page_kit::error_line(
            fill_template(lang.text(Msg::RulesTryFailed), &[("{detail}", detail)]),
            palette,
        )),
        TryStatus::Ready(result) if result.documents.is_empty() => {
            rows.push(page_kit::hint(lang.text(Msg::RulesTryEmpty), palette));
        }
        TryStatus::Ready(result) => {
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

fn try_row<'a>(
    tried: &TriedDocument,
    result: &TryResult,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
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
        format!(
            "  {}",
            fill_template(
                lang.text(Msg::RulesTryAlso),
                &[("{rules}", &places.join(", "))],
            )
        )
    };
    page_kit::list_row_one(
        page_kit::mono(
            format::ellipsize_middle(&tried.path.display().to_string(), TRY_PATH_CHARS),
            12.0,
            palette.text,
        ),
        Some(format!(
            "{}{conflicts}",
            format::ellipsize_end(&change, TRY_CHANGE_CHARS)
        )),
        false,
        false,
        None,
        palette,
    )
}

fn action_bar<'a>(state: &RulesPage, lang: Language, palette: &Palette) -> Element<'a, Message> {
    if state.is_confirming_delete() {
        return page_kit::action_bar(
            None,
            vec![
                page_kit::secondary(
                    lang.text(Msg::RulesCancel),
                    Some(send(RulesMessage::CancelDelete)),
                    palette,
                ),
                page_kit::primary(
                    lang.text(Msg::RulesDeleteConfirm),
                    Some(kit::ENTER_KEY),
                    Some(send(RulesMessage::ConfirmDelete)),
                    palette,
                ),
            ],
            palette,
        );
    }
    let Some(form) = state.form() else {
        return page_kit::action_bar(None, Vec::new(), palette);
    };
    let changed = form.is_changed() && !form.saving;
    let save_label = lang.text(match form.target {
        RuleFormTarget::FromSuggestion(_) => Msg::RuleSuggestionsConfirm,
        RuleFormTarget::New | RuleFormTarget::FromDocument | RuleFormTarget::Edit(_) => {
            Msg::RulesFormSave
        }
    });
    let left: Element<'a, Message> = if state.has_unsaved_changes() {
        row![
            page_kit::hint(lang.text(Msg::RulesUnsavedChanges), palette),
            try_summary(form, lang, palette),
        ]
        .spacing(GAP_M)
        .align_y(alignment::Vertical::Center)
        .into()
    } else {
        try_summary(form, lang, palette)
    };
    page_kit::action_bar_with(
        left,
        vec![
            page_kit::secondary(
                lang.text(Msg::RulesFormCancel),
                form.is_changed()
                    .then(|| send_form(RuleFormMessage::Cancel)),
                palette,
            ),
            page_kit::primary(
                save_label,
                Some(SAVE_KEY),
                changed.then(|| send_form(RuleFormMessage::Save)),
                palette,
            ),
        ],
        palette,
    )
}

/// "Try: Inbox 2 · Review 1 · Archive 11 ·" and the link to the list.
fn try_summary<'a>(form: &RuleForm, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let sentence = match &form.live_try.status {
        TryStatus::Idle => return Row::new().into(),
        TryStatus::Waiting | TryStatus::Running => lang.text(Msg::RulesTryLooking).to_owned(),
        TryStatus::Failed(detail) => fill_template(
            lang.text(Msg::RulesTryFailed),
            &[("{detail}", &format::ellipsize_end(detail, TRY_PATH_CHARS))],
        ),
        TryStatus::Ready(result) if result.documents.is_empty() => {
            lang.text(Msg::RulesTryEmpty).to_owned()
        }
        TryStatus::Ready(result) => {
            let count = |section: TrySection| {
                result
                    .documents
                    .iter()
                    .filter(|tried| tried.section == section)
                    .count()
            };
            format!(
                "{}: {} {} · {} {} · {} {} ·",
                lang.text(Msg::RulesTryHeading),
                lang.text(Msg::RulesTrySectionInbox),
                count(TrySection::Inbox),
                lang.text(Msg::RulesTrySectionReview),
                count(TrySection::Review),
                lang.text(Msg::RulesTrySectionArchive),
                count(TrySection::Archive),
            )
        }
    };
    let mut line = row![page_kit::hint(sentence, palette)]
        .spacing(GAP_S)
        .align_y(alignment::Vertical::Center);
    if matches!(&form.live_try.status, TryStatus::Ready(result) if !result.documents.is_empty()) {
        line = line.push(page_kit::link(
            lang.text(Msg::RulesTryShowList),
            Some(send(RulesMessage::ToggleTryList)),
            palette,
        ));
    }
    line.into()
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
