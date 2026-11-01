//! The review page, a tab: the documents no confirmed rule could file, each
//! with the reason, and right of the list the fields of the selected one to
//! change before it is filed. Pure view over [`ReviewList`].
//!
//! The text of a document never shows up here as anything but a value in a
//! field the user can read and change.

use std::ops::Range;
use std::path::Path;

use iced::widget::{column, container, Column, Row};
use iced::{alignment, Element};
use ncrs_index::{DocumentFields, Field, FieldOrigin, ReviewReason};
use ncrs_intake::{PatternPart, PatternSource};

use super::document_preview;
use super::filing_pieces::{target_view, ExampleLine, TargetHost};
use super::format;
use super::hermes::{self, Badges};
use super::hermes_kit::{self as page_kit, ChipKind, MenuItem, MenuLook, RowTwo};
use super::kit;
use crate::app::fill_template;
use crate::filing_setup::shown_path;
use crate::filing_work::target_of;
use crate::i18n::{FuturamaName, Language, Msg};
use crate::messages::{HermesPage, Message, ReviewListMessage};
use crate::palette::Palette;
use crate::replan_work::{ReplanAfter, ReplanItem};
use crate::review_list::{
    FiledNotice, Form, FormField, Problem, ReplanStep, ReviewList, VISIBLE_ROWS,
};
use crate::review_work::{ReviewItem, TextState};

pub const SENDER_ID: &str = "review-sender";
const RECIPIENT_ID: &str = "review-recipient";
const KIND_ID: &str = "review-kind";
const DATE_ID: &str = "review-date";
const NUMBER_ID: &str = "review-number";
const AMOUNT_ID: &str = "review-amount";
pub const TAG_INPUT_ID: &str = "review-tag-input";
pub const FOLDER_NAME_FIELD_ID: &str = "review-folder-name";

/// What fits a line of the list, in characters.
const NAME_CHARS: usize = 30;
const REASON_CHARS: usize = 38;
/// What fits beside the thumbnail and in a line of the preview, in characters.
const HEAD_NAME_CHARS: usize = 56;
const REPLAN_LINE_CHARS: usize = 84;
const NOTE_PATH_CHARS: usize = 60;
const CONTROL_WIDTH: f32 = 240.0;
/// Most rows of the preview shown; the count line covers all of them.
const REPLAN_VISIBLE_ROWS: usize = 12;

fn send(message: ReviewListMessage) -> Message {
    Message::ReviewList(message)
}

/// What the app knows that explains why the fields of a document come empty.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FieldGuide {
    /// `None` until the index has answered.
    pub confirmed_rules: Option<u64>,
    pub ai_is_ready: bool,
}

impl FieldGuide {
    fn explains_empty_fields(self) -> bool {
        self.confirmed_rules == Some(0) && !self.ai_is_ready
    }
}

/// Where documents are filed, to show the whole path a pattern gives.
#[derive(Debug, Clone, Copy)]
pub struct PreviewRoot<'a> {
    pub archive: Option<&'a Path>,
    pub home: &'a Path,
}

pub fn view<'a>(
    state: &'a ReviewList,
    badges: Badges,
    guide: FieldGuide,
    root: PreviewRoot<'_>,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let body: Element<'a, Message> = if let Some(step) = state.replan() {
        replan_body(state, step, lang, palette)
    } else if state.rows().is_empty() {
        let sentence = if state.is_busy() {
            Msg::ReviewListLoading
        } else {
            Msg::ReviewListEmpty
        };
        page_kit::centered_note(page_kit::hint(lang.text(sentence), palette))
    } else {
        page_kit::list_and_detail(
            list(state, lang, palette),
            detail(state, guide, root, lang, palette),
        )
    };
    page_kit::tab_page(
        hermes::tab_nav(HermesPage::Review, badges, lang, palette),
        header(state, guide, lang, palette),
        body,
        Some(action_bar(state, lang, palette)),
        palette,
    )
}

fn header<'a>(
    state: &ReviewList,
    guide: FieldGuide,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut items = Vec::new();
    if state.replan().is_none() && !state.is_busy() {
        if let Some(filed) = state.filed().filter(|filed| filed.offers_rule()) {
            items.push(MenuItem::new(
                create_rule_label(filed, lang),
                send(ReviewListMessage::CreateRule),
            ));
        }
        if guide.ai_is_ready && !state.rows().is_empty() {
            items.push(MenuItem::new(
                read_again_label(state.rows().len(), lang),
                send(ReviewListMessage::ReadAgainWithAi),
            ));
        }
    }
    let actions = if items.is_empty() {
        Vec::new()
    } else {
        vec![page_kit::menu_button("", &items, MenuLook::Icon, palette)]
    };
    page_kit::page_header(
        lang.text(Msg::ActionReviewList),
        lang.futurama_name(FuturamaName::ActionReviewList),
        Some(lang.text(Msg::ReviewListExplain).to_owned()),
        None,
        actions,
        palette,
    )
}

fn action_bar<'a>(state: &ReviewList, lang: Language, palette: &Palette) -> Element<'a, Message> {
    if state.replan().is_some() {
        let fileable = state.replan_fileable();
        return page_kit::action_bar(
            None,
            vec![
                page_kit::secondary(
                    lang.text(Msg::ReviewListReplanCancel),
                    Some(send(ReviewListMessage::ReplanCancel)),
                    palette,
                ),
                page_kit::primary(
                    &replan_file_label(fileable, lang),
                    Some(kit::ENTER_KEY),
                    (fileable > 0).then(|| send(ReviewListMessage::ReplanConfirm)),
                    palette,
                ),
            ],
            palette,
        );
    }
    let note = match state.filed() {
        Some(filed) => fill_template(
            lang.text(Msg::ReviewListFiledNote),
            &[(
                "{path}",
                &format::ellipsize_middle(&filed.path.display().to_string(), NOTE_PATH_CHARS),
            )],
        ),
        None => lang.text(Msg::ReviewListNote).to_owned(),
    };
    let mut buttons = Vec::new();
    if !state.rows().is_empty() {
        buttons.push(page_kit::secondary(
            lang.text(Msg::ReviewListReplanButton),
            (!state.is_busy()).then(|| send(ReviewListMessage::ReplanOpenCases)),
            palette,
        ));
    }
    let decidable = !state.is_busy() && state.selected().is_some();
    buttons.push(page_kit::primary(
        lang.text(Msg::ReviewListConfirm),
        Some(kit::ENTER_KEY),
        decidable.then(|| send(ReviewListMessage::Confirm)),
        palette,
    ));
    page_kit::action_bar(Some(note), buttons, palette)
}

/// Why a document is here, before it is put into words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cause {
    NoRule,
    NoText,
    ImageWithoutText,
    DefaultPattern,
    NothingRecognised,
    Unconfirmed,
    TargetExists,
    MissingField(Field),
    LowConfidence(Field),
}

impl Cause {
    fn of(reason: ReviewReason, text_state: TextState) -> Self {
        match reason {
            ReviewReason::NoMatchingRule => match text_state {
                TextState::NotReadable => Self::ImageWithoutText,
                TextState::NoneFound => Self::NoText,
                TextState::Read => Self::NoRule,
            },
            ReviewReason::UnconfirmedRule => Self::Unconfirmed,
            ReviewReason::TargetExists => Self::TargetExists,
            ReviewReason::MissingField(field) => Self::MissingField(field),
            ReviewReason::LowConfidence(field) => Self::LowConfidence(field),
        }
    }

    /// The cause of a row: where the pattern comes from and whether anything
    /// was recognised refine "no rule matches"; every other reason stays.
    fn of_row(item: &ReviewItem, source: Option<PatternSource>) -> Self {
        match (item.reason, item.text_state, source) {
            (ReviewReason::NoMatchingRule, _, Some(PatternSource::Default)) => Self::DefaultPattern,
            (ReviewReason::NoMatchingRule, TextState::Read, None)
                if fields_are_empty(&item.fields) =>
            {
                Self::NothingRecognised
            }
            _ => Self::of(item.reason, item.text_state),
        }
    }

    /// The sentence, the field a cause names spelled out.
    fn long_text(self, lang: Language) -> String {
        let message = match self {
            Self::NoRule => Msg::ReviewListReasonNoRule,
            Self::NoText => Msg::ReviewListReasonNoText,
            Self::ImageWithoutText => Msg::ReviewListReasonImageNoText,
            Self::DefaultPattern => Msg::ReviewListReasonDefaultPattern,
            Self::NothingRecognised => Msg::ReviewListReasonNothingRecognised,
            Self::Unconfirmed => Msg::ReviewListReasonUnconfirmed,
            Self::TargetExists => Msg::ReviewListReasonTargetExists,
            Self::MissingField(field) => {
                return named(Msg::ReviewListReasonMissingField, field, lang)
            }
            Self::LowConfidence(field) => {
                return named(Msg::ReviewListReasonLowConfidence, field, lang)
            }
        };
        lang.text(message).to_owned()
    }
}

fn named(template: Msg, field: Field, lang: Language) -> String {
    fill_template(
        lang.text(template),
        &[("{field}", lang.text(field_name(field)))],
    )
}

/// Why the document is here, in words; the field a reason names is spelled out.
pub fn reason_text(reason: ReviewReason, text_state: TextState, lang: Language) -> String {
    Cause::of(reason, text_state).long_text(lang)
}

fn fields_are_empty(fields: &DocumentFields) -> bool {
    *fields == DocumentFields::default()
}

fn field_name(field: Field) -> Msg {
    match field {
        Field::Sender => Msg::ReviewListFieldSender,
        Field::Recipient => Msg::ReviewListFieldRecipient,
        Field::Kind => Msg::ReviewListFieldKind,
        Field::Date => Msg::ReviewListFieldDate,
        Field::Number => Msg::ReviewListFieldNumber,
        Field::Amount => Msg::ReviewListFieldAmount,
    }
}

fn file_name(item: &ReviewItem) -> String {
    item.path.file_name().map_or_else(
        || item.path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn list<'a>(state: &'a ReviewList, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let rows = state
        .rows()
        .iter()
        .enumerate()
        .skip(state.first_visible())
        .take(VISIBLE_ROWS)
        .map(|(index, item)| {
            let cause = Cause::of_row(item, state.pattern_source(item));
            page_kit::list_row_two(
                RowTwo {
                    primary: format::ellipsize_end(&file_name(item), NAME_CHARS),
                    secondary: format::ellipsize_end(&cause.long_text(lang), REASON_CHARS),
                    right: item.fields.date.map(|date| date.to_string()),
                    lead: None,
                    muted: false,
                },
                index == state.cursor(),
                Some(send(ReviewListMessage::Select(index))),
                palette,
            )
        })
        .collect();
    page_kit::list_pane(
        rows,
        |delta| send(ReviewListMessage::Scrolled(delta)),
        palette,
    )
}

fn detail<'a>(
    state: &'a ReviewList,
    guide: FieldGuide,
    root: PreviewRoot<'_>,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let Some(item) = state.selected() else {
        return page_kit::detail(Vec::new());
    };
    let mut children = Vec::new();
    if guide.explains_empty_fields() {
        children.push(page_kit::hint(
            lang.text(Msg::ReviewListNoRulesHint),
            palette,
        ));
    }
    children.push(head_block(state, item, root, lang, palette));
    if state.is_preview_open() {
        children.push(
            container(document_preview::view(
                state.document_preview(),
                lang,
                palette,
            ))
            .height(PREVIEW_HEIGHT)
            .into(),
        );
    } else {
        children.extend(field_rows(state, lang, palette));
        children.push(page_kit::section(
            lang.text(Msg::ReviewListSectionFiling),
            palette,
        ));
        children.push(target_section(state, lang, palette));
    }
    if let Some(problem) = state.problem() {
        children.push(page_kit::error_line(problem_text(problem, lang), palette));
    }
    page_kit::detail(children)
}

/// The room of the page preview in the detail.
const PREVIEW_HEIGHT: f32 = 420.0;

/// The thumbnail with the name, and where the document will be filed.
fn head_block<'a>(
    state: &ReviewList,
    item: &ReviewItem,
    root: PreviewRoot<'_>,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let template = lang.text(Msg::ReviewListPreview);
    let label = template
        .split_once("{path}")
        .map_or(template, |(before, _)| before)
        .trim();
    let filed_as: Element<'a, Message> = match shown_target(state, root) {
        Some((shown, marked)) => page_kit::path(path_parts(&shown, &marked), palette),
        None => {
            let note = match state.missing_field() {
                Some(field) => fill_template(
                    lang.text(Msg::ReviewListPreviewMissingField),
                    &[("{field}", lang.text(field_name(field)))],
                ),
                None => lang.text(Msg::ReviewListPreviewNone).to_owned(),
            };
            page_kit::hint(note, palette)
        }
    };
    let texts = column![
        page_kit::value(
            format::ellipsize_end(&file_name(item), HEAD_NAME_CHARS),
            palette
        ),
        page_kit::hint(label.to_owned(), palette),
        filed_as,
    ]
    .spacing(page_kit::GAP_XS);
    column![
        Row::new()
            .push(page_kit::thumbnail(
                Some(send(ReviewListMessage::TogglePreview)),
                palette
            ))
            .push(texts)
            .spacing(page_kit::GAP_M)
            .align_y(alignment::Vertical::Center),
        page_kit::rule(palette),
    ]
    .spacing(page_kit::GAP_S)
    .into()
}

/// The shown path cut at the places that are values of a field, and for each
/// piece whether it is one.
fn path_parts(shown: &str, marked: &[Range<usize>]) -> Vec<(String, bool)> {
    let mut parts = Vec::new();
    let mut at = 0;
    for range in marked {
        if let Some(plain) = shown.get(at..range.start).filter(|plain| !plain.is_empty()) {
            parts.push((plain.to_owned(), false));
        }
        if let Some(value) = shown.get(range.clone()) {
            parts.push((value.to_owned(), true));
        }
        at = range.end;
    }
    if let Some(tail) = shown.get(at..).filter(|tail| !tail.is_empty()) {
        parts.push((tail.to_owned(), false));
    }
    parts
}

fn field_rows<'a>(
    state: &'a ReviewList,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let values = state.form();
    let origin_of = |field: Field| {
        let item = state.selected()?;
        origin_text(&item.origins.of(field), state.is_edited(field), lang)
    };
    let row_of = |field: Field, form_field: FormField, value: &'a str, placeholder: Option<Msg>| {
        let input = page_kit::text_field(
            placeholder.map_or("", |message| lang.text(message)),
            value,
            field_id(form_field),
            Some(move |typed| send(ReviewListMessage::FieldChanged(form_field, typed))),
            send(ReviewListMessage::Confirm),
            palette,
        );
        let mut control = Row::new()
            .push(container(input).width(CONTROL_WIDTH))
            .spacing(page_kit::GAP_S)
            .align_y(alignment::Vertical::Center);
        if let Some(origin) = origin_of(field) {
            control = control.push(page_kit::value_dim(origin, palette));
        }
        page_kit::form_row(lang.text(field_name(field)), control.into(), None, palette)
    };
    vec![
        row_of(Field::Sender, FormField::Sender, &values.sender, None),
        row_of(
            Field::Recipient,
            FormField::Recipient,
            &values.recipient,
            None,
        ),
        row_of(Field::Kind, FormField::Kind, &values.kind, None),
        row_of(Field::Number, FormField::Number, &values.number, None),
        row_of(
            Field::Amount,
            FormField::Amount,
            &values.amount,
            Some(Msg::ReviewListHintAmount),
        ),
        row_of(
            Field::Date,
            FormField::Date,
            &values.date,
            Some(Msg::ReviewListHintDate),
        ),
        tags_row(state, values, lang, palette),
    ]
}

/// The tags: those the fields give (gray, nothing to remove), those the user
/// added (outlined, with a cross), the link for a new one and a note.
fn tags_row<'a>(
    state: &ReviewList,
    values: &'a Form,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let automatic = state
        .automatic_tags()
        .into_iter()
        .filter(|tag| !values.tags.contains(tag))
        .map(|tag| {
            page_kit::chip(
                &lang.tag_label(&state.kinds().catalog, &tag),
                ChipKind::Automatic,
                None,
                palette,
            )
        });
    let own = values.tags.iter().map(|tag| {
        page_kit::chip(
            tag,
            ChipKind::Own,
            Some(send(ReviewListMessage::RemoveTag(tag.clone()))),
            palette,
        )
    });
    let mut items: Vec<Element<'a, Message>> = automatic.chain(own).collect();
    if !state.is_tag_input_open() {
        items.push(page_kit::link(
            lang.text(Msg::ReviewListTagsAdd),
            Some(send(ReviewListMessage::OpenTagInput)),
            palette,
        ));
    }
    let mut control = Column::new().push(
        Row::with_children(items)
            .spacing(page_kit::GAP_S)
            .align_y(alignment::Vertical::Center)
            .wrap()
            .vertical_spacing(page_kit::GAP_XS),
    );
    if state.is_tag_input_open() {
        control = control.push(
            container(page_kit::text_field(
                lang.text(Msg::ReviewListTagsAdd),
                &values.tag_input,
                TAG_INPUT_ID,
                Some(|typed| send(ReviewListMessage::FieldChanged(FormField::TagInput, typed))),
                send(ReviewListMessage::AddTag),
                palette,
            ))
            .width(CONTROL_WIDTH),
        );
    }
    page_kit::form_row(
        lang.text(Msg::ReviewListFieldTags),
        control.spacing(page_kit::GAP_XS).into(),
        Some(page_kit::hint(lang.text(Msg::ReviewListTagsNote), palette)),
        palette,
    )
}

/// "Move to" and "File name": the shared widget, with where the target comes
/// from under "Move to".
fn target_section<'a>(
    state: &'a ReviewList,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let origin = state
        .selected()
        .and_then(|item| state.pattern_source(item))
        .map(|source| match source {
            PatternSource::Default => Msg::ReviewListOriginDefaultTarget,
            PatternSource::RuleOrProfile => Msg::ReviewListOriginRule,
        })
        .map(|message| lang.text(message).to_owned());
    let Some(editor) = state.target_editor() else {
        return column![].into();
    };
    target_view(
        editor,
        &TargetHost {
            wrap: |edit| send(ReviewListMessage::TargetEdited(edit)),
            name_field_id: FOLDER_NAME_FIELD_ID,
            folder_hint: origin,
            example: ExampleLine::Hidden,
            lang,
            palette,
        },
    )
}

fn replan_body<'a>(
    state: &ReviewList,
    step: &ReplanStep,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut children = Vec::new();
    match step {
        ReplanStep::Looking(_) => {
            children.push(page_kit::hint(
                lang.text(Msg::ReviewListReplanLooking),
                palette,
            ));
        }
        ReplanStep::Preview { items, .. } => {
            children.push(page_kit::hint(
                lang.text(Msg::ReviewListReplanExplain),
                palette,
            ));
            if items.is_empty() {
                children.push(page_kit::hint(
                    lang.text(Msg::ReviewListReplanEmpty),
                    palette,
                ));
            }
            for item in items.iter().take(REPLAN_VISIBLE_ROWS) {
                let (now, after) = replan_texts(item, lang);
                children.push(page_kit::list_row_two(
                    RowTwo {
                        primary: format::ellipsize_end(&replan_file_name(item), HEAD_NAME_CHARS),
                        secondary: format::ellipsize_end(
                            &format!("{now} → {after}"),
                            REPLAN_LINE_CHARS,
                        ),
                        right: None,
                        lead: None,
                        muted: false,
                    },
                    false,
                    None,
                    palette,
                ));
            }
            let hidden = items.len().saturating_sub(REPLAN_VISIBLE_ROWS);
            if hidden > 0 {
                children.push(page_kit::hint(
                    fill_template(
                        lang.text(Msg::ReviewListReplanMore),
                        &[("{count}", &hidden.to_string())],
                    ),
                    palette,
                ));
            }
            if !items.is_empty() {
                let fileable = items.iter().filter(|item| item.would_be_filed()).count();
                children.push(page_kit::value(
                    fill_template(
                        lang.text(Msg::ReviewListReplanCount),
                        &[
                            ("{filed}", &fileable.to_string()),
                            ("{total}", &items.len().to_string()),
                        ],
                    ),
                    palette,
                ));
            }
        }
    }
    if let Some(problem) = state.problem() {
        children.push(page_kit::error_line(problem_text(problem, lang), palette));
    }
    page_kit::detail(children)
}

/// How the form shows the origin of a field: where the value came from, or
/// that the user changed it since. Nothing for a value of unknown origin.
pub fn origin_text(origin: &FieldOrigin, edited: bool, lang: Language) -> Option<String> {
    if edited {
        return Some(lang.text(Msg::ReviewListOriginChanged).to_owned());
    }
    match origin {
        FieldOrigin::Unknown => None,
        FieldOrigin::Rule(_) => Some(lang.text(Msg::ReviewListOriginRule).to_owned()),
        FieldOrigin::Ai { confidence_percent } => Some(fill_template(
            lang.text(Msg::ReviewListOriginAi),
            &[("{percent}", &confidence_percent.to_string())],
        )),
        FieldOrigin::Manual => Some(lang.text(Msg::ReviewListOriginManual).to_owned()),
        FieldOrigin::AddressBlock => Some(lang.text(Msg::ReviewListOriginAddress).to_owned()),
        FieldOrigin::Heading => Some(lang.text(Msg::ReviewListOriginHeading).to_owned()),
    }
}

/// The path the document would be filed under as shown, and the places in it
/// that are values of a field.
fn shown_target(state: &ReviewList, root: PreviewRoot<'_>) -> Option<(String, Vec<Range<usize>>)> {
    let relative = state.preview()?;
    let parts = state.preview_parts()?;
    let source = state
        .selected()
        .map(|item| item.path.as_path())
        .unwrap_or_else(|| Path::new(""));
    let tail = target_of(Path::new(""), &relative, source)
        .display()
        .to_string();
    let shown = match root.archive {
        Some(archive) => shown_path(&target_of(archive, &relative, source), root.home),
        None => tail.clone(),
    };
    let marked = path_ranges(&parts, shown.len().saturating_sub(tail.len()));
    Some((shown, marked))
}

/// The places in the shown path that are values of a field; `offset` is where
/// the first part starts in it.
fn path_ranges(parts: &[PatternPart], offset: usize) -> Vec<Range<usize>> {
    let mut at = offset;
    let mut ranges = Vec::new();
    for part in parts {
        let end = at + part.text.len();
        if part.field.is_some() {
            ranges.push(at..end);
        }
        at = end;
    }
    ranges
}

fn replan_file_label(count: usize, lang: Language) -> String {
    if count == 1 {
        lang.text(Msg::ReviewListReplanFileOne).to_owned()
    } else {
        fill_template(
            lang.text(Msg::ReviewListReplanFile),
            &[("{count}", &count.to_string())],
        )
    }
}

fn replan_file_name(item: &ReplanItem) -> String {
    item.path.file_name().map_or_else(
        || item.path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The `now` and `after` cells of a previewed document. A document the rules
/// could not read is not judged by its reason as if its text were empty.
fn replan_texts(item: &ReplanItem, lang: Language) -> (String, String) {
    let text_state = if item.after == ReplanAfter::Unread {
        TextState::NoneFound
    } else {
        TextState::Read
    };
    let now = item.now.map_or_else(
        || lang.text(Msg::ReviewListReplanNowNone).to_owned(),
        |reason| reason_text(reason, text_state, lang),
    );
    let after = match &item.after {
        ReplanAfter::Filed(path) => path.display().to_string(),
        ReplanAfter::Review(reason) => fill_template(
            lang.text(Msg::ReviewListReplanAfterReview),
            &[("{reason}", &reason_text(*reason, TextState::Read, lang))],
        ),
        ReplanAfter::Unread => lang.text(Msg::ReviewListReplanAfterUnread).to_owned(),
    };
    (now, after)
}

fn read_again_label(count: usize, lang: Language) -> String {
    if count == 1 {
        lang.text(Msg::ReviewListReadAgainWithAiOne).to_owned()
    } else {
        fill_template(
            lang.text(Msg::ReviewListReadAgainWithAi),
            &[("{count}", &count.to_string())],
        )
    }
}

fn create_rule_label(filed: &FiledNotice, lang: Language) -> String {
    fill_template(
        lang.text(Msg::ReviewListCreateRule),
        &[(
            "{sender}",
            &format::ellipsize_end(&filed.sender, CREATE_RULE_SENDER_CHARS),
        )],
    )
}

/// The sender in the button is cut: the footer has room for little else.
const CREATE_RULE_SENDER_CHARS: usize = 24;

fn problem_text(problem: &Problem, lang: Language) -> String {
    match problem {
        Problem::InvalidDate => lang.text(Msg::ReviewListProblemDate).to_owned(),
        Problem::InvalidAmount => lang.text(Msg::ReviewListProblemAmount).to_owned(),
        Problem::PatternDoesNotFit => lang.text(Msg::ReviewListProblemPattern).to_owned(),
        Problem::TargetExists => lang.text(Msg::ReviewListProblemTargetExists).to_owned(),
        Problem::FileChanged => lang.text(Msg::ReviewListProblemFileChanged).to_owned(),
        Problem::Failed(detail) => fill_template(
            lang.text(Msg::ReviewListProblemFailed),
            &[("{detail}", detail)],
        ),
    }
}

/// The id of a text field of the form.
pub fn field_id(field: FormField) -> &'static str {
    match field {
        FormField::Sender => SENDER_ID,
        FormField::Recipient => RECIPIENT_ID,
        FormField::Kind => KIND_ID,
        FormField::Date => DATE_ID,
        FormField::Number => NUMBER_ID,
        FormField::Amount => AMOUNT_ID,
        FormField::TagInput => TAG_INPUT_ID,
    }
}

/// The id of the field that takes the focus when the form opens.
pub const FIRST_FIELD_ID: &str = SENDER_ID;

// reason: a failing assertion is the signal in a test
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_origin_has_its_words_in_both_languages() {
        let rule = FieldOrigin::Rule(None);
        let ai = FieldOrigin::Ai {
            confidence_percent: 82,
        };
        let texts = |lang: Language| {
            [&FieldOrigin::Unknown, &rule, &ai, &FieldOrigin::Manual]
                .map(|origin| origin_text(origin, false, lang))
        };
        assert_eq!(
            texts(Language::English),
            [
                None,
                Some("From a rule".to_owned()),
                Some("AI, 82 % sure".to_owned()),
                Some("By hand".to_owned()),
            ]
        );
        assert_eq!(
            texts(Language::German)[1..3],
            [
                Some("Aus einer Regel".to_owned()),
                Some("KI, 82 % sicher".to_owned())
            ]
        );
    }

    #[test]
    fn a_value_the_user_changed_says_so_whatever_its_origin() {
        for origin in [
            FieldOrigin::Unknown,
            FieldOrigin::Rule(None),
            FieldOrigin::Manual,
        ] {
            assert_eq!(
                origin_text(&origin, true, Language::English).as_deref(),
                Some("Changed by hand")
            );
        }
    }

    #[test]
    fn the_filed_as_path_marks_the_values_of_the_fields_in_it() {
        let list = crate::review_list::tests::loaded_list();
        let root = PreviewRoot {
            archive: Some(Path::new("/home/me/Archive")),
            home: Path::new("/home/me"),
        };
        let (shown, marked) = shown_target(&list, root).unwrap();
        assert_eq!(
            shown,
            ["~", "Archive", "Company A", "2024-03-09_Invoice.pdf"]
                .join(std::path::MAIN_SEPARATOR_STR)
        );
        let texts: Vec<&str> = marked.into_iter().map(|range| &shown[range]).collect();
        assert_eq!(texts, ["Company A", "2024-03-09", "Invoice"]);
    }

    #[test]
    fn the_marked_places_of_a_path_are_the_values_of_fields() {
        let part = |text: &str, field: Option<Field>| PatternPart {
            text: text.to_owned(),
            field,
        };
        let parts = [
            part("Company A", Some(Field::Sender)),
            part("/", None),
            part("2024-03-09", Some(Field::Date)),
            part("_", None),
            part("invoice", Some(Field::Kind)),
        ];
        let shown = "~/Archive/Company A/2024-03-09_invoice.pdf";
        let offset = shown.find("Company A").unwrap();
        let marked: Vec<&str> = path_ranges(&parts, offset)
            .into_iter()
            .map(|range| &shown[range])
            .collect();
        assert_eq!(marked, ["Company A", "2024-03-09", "invoice"]);
    }
}
