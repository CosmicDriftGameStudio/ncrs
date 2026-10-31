//! The review list: the documents no confirmed rule could file, each with the
//! reason and the fields, and a form to change the fields before the document
//! is filed. A page of the Hermes card. Pure view over [`ReviewList`].
//!
//! The text of a document never shows up here as anything but a value in a
//! field the user can read and change.

use std::path::Path;

use iced::widget::{column, row, Space};
use iced::{alignment, Element, Length, Size};
use ncrs_index::{DocumentFields, Field, ReviewReason};
use ncrs_intake::PatternSource;

use super::document_preview;
use super::format;
use super::frame::{self, DialogSize, Frame, KeyHints};
use super::hermes::{self, Badges};
use super::kit::{self, dim, error_line, form_row, paragraph, Columns, Tone};
use super::theme::spacing;
use crate::app::fill_template;
use crate::filing_setup::shown_path;
use crate::filing_work::target_of;
use crate::i18n::{FuturamaName, Language, Msg};
use crate::messages::{HermesFocus, HermesPage, Message, ReviewListMessage};
use crate::palette::Palette;
use crate::replan_work::{ReplanAfter, ReplanItem};
use crate::review_list::{FiledNotice, FormField, Problem, ReplanStep, ReviewList, VISIBLE_ROWS};
use crate::review_work::{ReviewItem, TextState};

pub const SENDER_ID: &str = "review-sender";
const KIND_ID: &str = "review-kind";
const DATE_ID: &str = "review-date";
const NUMBER_ID: &str = "review-number";
const AMOUNT_ID: &str = "review-amount";
const TAGS_ID: &str = "review-tags";
const PATTERN_ID: &str = "review-pattern";

const SIZE: DialogSize = DialogSize::Card;

/// How the width of a row of the list is shared: file, reason.
const PORTIONS: [u16; 2] = [4, 5];

/// How the room below the explanation is shared: the list, then the preview
/// and the form.
const LIST_PORTION: u16 = 2;
const DETAIL_PORTION: u16 = 3;

/// What the lines above the list take of the page, to size the split below
/// them: the explanation, which may wrap; the warning; the problem line.
const EXPLAIN_HEIGHT: f32 = 44.0;
const HINT_HEIGHT: f32 = 36.0;
const PROBLEM_HEIGHT: f32 = 20.0;

/// The split never gets smaller than this; the page scrolls instead.
const MIN_SPLIT_HEIGHT: f32 = 480.0;

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
    window: Size,
    badges: Badges,
    guide: FieldGuide,
    root: PreviewRoot<'_>,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let line_chars = frame::content_chars(SIZE, window, frame::SIDEBAR_ROOM);
    let mut body = column![].spacing(spacing::LINE_GAP);
    if let Some(step) = state.replan() {
        body = body.push(replan_section(step, line_chars, lang, palette));
        if let Some(problem) = state.problem() {
            body = body.push(error_line(problem_text(problem, lang), palette));
        }
        return finish(state, body, window, badges, guide, lang, palette);
    }
    body = body.push(paragraph(lang.text(Msg::ReviewListExplain), palette));
    if guide.explains_empty_fields() {
        body = body.push(kit::warning_box(
            lang.text(Msg::ReviewListNoRulesHint),
            palette,
        ));
    }

    if state.rows().is_empty() {
        body = body.push(dim(
            lang.text(if state.is_busy() {
                Msg::ReviewListLoading
            } else {
                Msg::ReviewListEmpty
            }),
            palette,
        ));
    } else {
        let above = EXPLAIN_HEIGHT
            + if guide.explains_empty_fields() {
                HINT_HEIGHT
            } else {
                0.0
            }
            + if state.problem().is_some() {
                PROBLEM_HEIGHT
            } else {
                0.0
            };
        let height =
            (frame::content_height(SIZE, window) - above - spacing::LINE_GAP).max(MIN_SPLIT_HEIGHT);
        body = body.push(split(
            state,
            root,
            height,
            line_chars,
            badges.focus,
            lang,
            palette,
        ));
    }
    if let Some(problem) = state.problem() {
        body = body.push(error_line(problem_text(problem, lang), palette));
    }
    finish(state, body, window, badges, guide, lang, palette)
}

fn finish<'a>(
    state: &'a ReviewList,
    body: iced::widget::Column<'a, Message>,
    window: Size,
    badges: Badges,
    guide: FieldGuide,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let line_chars = frame::content_chars(SIZE, window, frame::SIDEBAR_ROOM);
    let mut frame = Frame::new(
        lang.text(Msg::ActionReviewList),
        body,
        SIZE,
        window,
        lang,
        palette,
    )
    .title_theme(lang.futurama_name(FuturamaName::ActionReviewList))
    .buttons(buttons(state, &guide, lang, palette))
    .sidebar(hermes::nav(HermesPage::Review, badges, lang, palette))
    .on_close(send(ReviewListMessage::Close))
    .hints(KeyHints {
        enter: state.replan().is_none() || state.replan_fileable() > 0,
        tab: state.is_editing(),
    });
    if let Some(filed) = state.filed() {
        frame = frame.footer_note(fill_template(
            lang.text(Msg::ReviewListFiledNote),
            &[(
                "{path}",
                &format::ellipsize_middle(&filed.path.display().to_string(), line_chars),
            )],
        ));
    }
    frame.into_element()
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
    RuleConflict,
    NoTaxContext,
    SeveralTaxContexts,
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
            ReviewReason::RuleConflict => Self::RuleConflict,
            ReviewReason::NoTaxContextMatches => Self::NoTaxContext,
            ReviewReason::SeveralTaxContextsMatch => Self::SeveralTaxContexts,
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
            Self::RuleConflict => Msg::ReviewListReasonRuleConflict,
            Self::NoTaxContext => Msg::ReviewListReasonNoTaxContext,
            Self::SeveralTaxContexts => Msg::ReviewListReasonSeveralTaxContexts,
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

    /// The few words of the chip in the list.
    fn short_text(self, lang: Language) -> String {
        let message = match self {
            Self::NoRule => Msg::ReviewListChipNoRule,
            Self::NoText => Msg::ReviewListChipNoText,
            Self::ImageWithoutText => Msg::ReviewListChipImageNoText,
            Self::DefaultPattern => Msg::ReviewListChipDefaultPattern,
            Self::NothingRecognised => Msg::ReviewListChipNothingRecognised,
            Self::Unconfirmed => Msg::ReviewListChipUnconfirmed,
            Self::RuleConflict => Msg::ReviewListChipRuleConflict,
            Self::NoTaxContext => Msg::ReviewListChipNoTaxContext,
            Self::SeveralTaxContexts => Msg::ReviewListChipSeveralTaxContexts,
            Self::TargetExists => Msg::ReviewListChipTargetExists,
            Self::MissingField(field) => {
                return named(Msg::ReviewListChipMissingField, field, lang)
            }
            Self::LowConfidence(field) => {
                return named(Msg::ReviewListChipLowConfidence, field, lang)
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
        Field::Kind => Msg::ReviewListFieldKind,
        Field::TaxContext => Msg::ReviewListFieldTaxContext,
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

fn split<'a>(
    state: &'a ReviewList,
    root: PreviewRoot<'_>,
    height: f32,
    line_chars: usize,
    focus: HermesFocus,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let portions = LIST_PORTION + DETAIL_PORTION;
    let list_chars = (line_chars * usize::from(LIST_PORTION))
        .checked_div(usize::from(portions))
        .unwrap_or_default()
        .saturating_sub(frame::chars_that_fit(spacing::L));
    let left = column![
        list(state, list_chars, focus, lang, palette),
        selected_cause(state, lang, palette),
        target_line(state, root, lang, palette),
    ]
    .spacing(spacing::LINE_GAP)
    .width(Length::FillPortion(LIST_PORTION));
    let right = column![
        document_preview::view(state.document_preview(), lang, palette),
        form(state, lang, palette),
    ]
    .spacing(spacing::LINE_GAP)
    .width(Length::FillPortion(DETAIL_PORTION))
    .height(Length::Fill);
    row![left, right].spacing(spacing::L).height(height).into()
}

fn selected_cause<'a>(
    state: &ReviewList,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    match state.selected() {
        Some(item) => paragraph(
            Cause::of_row(item, state.pattern_source(item)).long_text(lang),
            palette,
        ),
        None => Space::new().into(),
    }
}

fn list<'a>(
    state: &'a ReviewList,
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
            lang.text(Msg::ReviewListColumnFile),
            lang.text(Msg::ReviewListColumnReason),
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
        .map(|(index, item)| {
            let selected = index == state.cursor();
            let on_fill = selected && focus == HermesFocus::Content;
            let color = if on_fill {
                palette.cursor_text
            } else {
                palette.text
            };
            let cause = Cause::of_row(item, state.pattern_source(item));
            let content = row![
                columns.path_cell(0, &file_name(item), color),
                columns.chip_cell(1, &cause.short_text(lang), on_fill, palette),
            ]
            .align_y(alignment::Vertical::Center);
            kit::list_row(
                content.into(),
                send(ReviewListMessage::Select(index)),
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
        |delta| send(ReviewListMessage::Scrolled(delta)),
        palette,
    )
}

fn form<'a>(state: &'a ReviewList, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let values = state.form();
    let text_field = |field: FormField, id: &'static str, value: &'a str| {
        kit::text_field(
            "",
            value,
            id,
            Some(move |typed| send(ReviewListMessage::FieldChanged(field, typed))),
            send(ReviewListMessage::Confirm),
            palette,
        )
    };
    let row_of = |label: Msg, hint: Option<Msg>, control: Element<'a, Message>| {
        form_row(
            lang.text(label),
            None,
            hint.map(|message| lang.text(message)),
            control,
            palette,
        )
    };
    let tax_choices = kit::tax_context_choices(
        lang.text(Msg::ReviewListTaxOpen),
        lang.text(Msg::ReviewListTaxNone),
        state.tax_contexts(),
    );
    let tax_options: Vec<(&str, Message)> = tax_choices
        .iter()
        .map(|(label, choice)| {
            (
                *label,
                send(ReviewListMessage::TaxContextChosen(choice.clone())),
            )
        })
        .collect();
    let selected_tax = tax_choices
        .iter()
        .position(|(_, choice)| *choice == values.tax_context)
        .unwrap_or_default();
    let assigned_by = state
        .selected()
        .and_then(|item| item.tax_context_assigned_by.as_ref())
        .map(|assigned_by| {
            fill_template(
                lang.text(Msg::ReviewListTaxContextFromCondition),
                &[
                    (
                        "{kind}",
                        lang.text(kit::condition_kind_name(assigned_by.kind)),
                    ),
                    ("{value}", &assigned_by.value),
                ],
            )
        });
    column![
        row_of(
            Msg::ReviewListFieldSender,
            None,
            text_field(FormField::Sender, SENDER_ID, &values.sender),
        ),
        row_of(
            Msg::ReviewListFieldKind,
            None,
            text_field(FormField::Kind, KIND_ID, &values.kind),
        ),
        form_row(
            lang.text(Msg::ReviewListFieldTaxContext),
            None,
            assigned_by.as_deref(),
            kit::dropdown(&tax_options, selected_tax, true, palette),
            palette,
        ),
        row_of(
            Msg::ReviewListFieldDate,
            Some(Msg::ReviewListHintDate),
            text_field(FormField::Date, DATE_ID, &values.date),
        ),
        row_of(
            Msg::ReviewListFieldNumber,
            None,
            text_field(FormField::Number, NUMBER_ID, &values.number),
        ),
        row_of(
            Msg::ReviewListFieldAmount,
            Some(Msg::ReviewListHintAmount),
            text_field(FormField::Amount, AMOUNT_ID, &values.amount),
        ),
        row_of(
            Msg::ReviewListFieldTags,
            Some(Msg::ReviewListHintTags),
            text_field(FormField::Tags, TAGS_ID, &values.tags),
        ),
        row_of(
            Msg::ReviewListFieldPattern,
            Some(Msg::ReviewListHintPattern),
            text_field(FormField::Pattern, PATTERN_ID, &values.pattern),
        ),
    ]
    .into()
}

fn target_line<'a>(
    state: &ReviewList,
    root: PreviewRoot<'_>,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    if let Some(relative) = state.preview() {
        let source = state
            .selected()
            .map(|item| item.path.as_path())
            .unwrap_or_else(|| Path::new(""));
        let shown = match root.archive {
            Some(archive) => shown_path(&target_of(archive, &relative, source), root.home),
            None => target_of(Path::new(""), &relative, source)
                .display()
                .to_string(),
        };
        return paragraph(
            fill_template(lang.text(Msg::ReviewListPreview), &[("{path}", &shown)]),
            palette,
        );
    }
    match state.missing_field() {
        Some(field) => dim(
            fill_template(
                lang.text(Msg::ReviewListPreviewMissingField),
                &[("{field}", lang.text(field_name(field)))],
            ),
            palette,
        ),
        None => dim(lang.text(Msg::ReviewListPreviewNone), palette),
    }
}

fn buttons<'a>(
    state: &ReviewList,
    guide: &FieldGuide,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    if state.replan().is_some() {
        let fileable = state.replan_fileable();
        return vec![
            kit::button(
                lang.text(Msg::ReviewListReplanCancel),
                Some(send(ReviewListMessage::ReplanCancel)),
                Tone::Secondary,
                palette,
            ),
            kit::enter_button(
                &replan_file_label(fileable, lang),
                (fileable > 0).then(|| send(ReviewListMessage::ReplanConfirm)),
                Tone::Primary,
                palette,
            ),
        ];
    }
    let mut items = vec![kit::button(
        lang.text(Msg::ReviewListClose),
        Some(send(ReviewListMessage::Close)),
        Tone::Secondary,
        palette,
    )];
    if !state.rows().is_empty() {
        items.push(kit::button(
            lang.text(Msg::ReviewListReplanButton),
            (!state.is_busy()).then(|| send(ReviewListMessage::ReplanOpenCases)),
            Tone::Secondary,
            palette,
        ));
    }
    if guide.ai_is_ready && !state.rows().is_empty() {
        items.push(kit::button(
            &read_again_label(state.rows().len(), lang),
            (!state.is_busy()).then(|| send(ReviewListMessage::ReadAgainWithAi)),
            Tone::Secondary,
            palette,
        ));
    }
    if let Some(filed) = state.filed().filter(|filed| filed.offers_rule()) {
        items.push(kit::button(
            &create_rule_label(filed, lang),
            (!state.is_busy()).then(|| send(ReviewListMessage::CreateRule)),
            Tone::Secondary,
            palette,
        ));
    }
    let decidable = !state.is_busy() && state.selected().is_some();
    items.push(kit::enter_button(
        lang.text(Msg::ReviewListConfirm),
        decidable.then(|| send(ReviewListMessage::Confirm)),
        Tone::Primary,
        palette,
    ));
    items
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

/// How the width of a row of the preview is shared: file, now, after.
const REPLAN_PORTIONS: [u16; 3] = [4, 5, 6];

/// Most rows of the preview shown; the count line covers all of them.
const REPLAN_VISIBLE_ROWS: usize = 6;

fn replan_section<'a>(
    step: &'a ReplanStep,
    line_chars: usize,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let items = match step {
        ReplanStep::Looking(_) => {
            return dim(lang.text(Msg::ReviewListReplanLooking), palette);
        }
        ReplanStep::Preview { items, .. } => items,
    };
    let mut section = column![paragraph(lang.text(Msg::ReviewListReplanExplain), palette)]
        .spacing(spacing::LINE_GAP);
    if items.is_empty() {
        return section
            .push(dim(lang.text(Msg::ReviewListReplanEmpty), palette))
            .into();
    }
    let columns = Columns {
        portions: &REPLAN_PORTIONS,
        line_chars,
    };
    section = section.push(columns.header(
        &[
            lang.text(Msg::ReviewListColumnFile),
            lang.text(Msg::ReviewListReplanColumnNow),
            lang.text(Msg::ReviewListReplanColumnAfter),
        ],
        palette,
    ));
    for item in items.iter().take(REPLAN_VISIBLE_ROWS) {
        let (now, after) = replan_texts(item, lang);
        section = section.push(
            row![
                columns.path_cell(0, &replan_file_name(item), palette.text),
                columns.cell(1, &now, palette.text),
                columns.cell(2, &after, palette.text),
            ]
            .align_y(alignment::Vertical::Center),
        );
    }
    let hidden = items.len().saturating_sub(REPLAN_VISIBLE_ROWS);
    if hidden > 0 {
        section = section.push(dim(
            fill_template(
                lang.text(Msg::ReviewListReplanMore),
                &[("{count}", &hidden.to_string())],
            ),
            palette,
        ));
    }
    let fileable = items.iter().filter(|item| item.would_be_filed()).count();
    section
        .push(paragraph(
            fill_template(
                lang.text(Msg::ReviewListReplanCount),
                &[
                    ("{filed}", &fileable.to_string()),
                    ("{total}", &items.len().to_string()),
                ],
            ),
            palette,
        ))
        .into()
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
        FormField::Kind => KIND_ID,
        FormField::Date => DATE_ID,
        FormField::Number => NUMBER_ID,
        FormField::Amount => AMOUNT_ID,
        FormField::Tags => TAGS_ID,
        FormField::Pattern => PATTERN_ID,
    }
}

/// The id of the field that takes the focus when the form opens.
pub const FIRST_FIELD_ID: &str = SENDER_ID;
