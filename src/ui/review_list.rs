//! The review list: the documents no confirmed rule could file, each with the
//! reason and the fields, and a form to change the fields before the document
//! is filed. A page of the Hermes card. Pure view over [`ReviewList`].
//!
//! The text of a document never shows up here as anything but a value in a
//! field the user can read and change.

use std::ops::Range;
use std::path::Path;

use iced::widget::text::Wrapping;
use iced::widget::{column, container, rich_text, row, text, Space};
use iced::{alignment, Element, Length, Size};
use ncrs_index::{DocumentFields, Field, FieldOrigin, ReviewReason};
use ncrs_intake::{PatternPart, PatternSource};

use super::document_preview;
use super::format;
use super::frame::{self, DialogSize, Frame, KeyHints};
use super::hermes::{self, Badges};
use super::kit::{self, dim, error_line, paragraph, Columns, Tone};
use super::picker::highlight_spans;
use super::theme::{font_size, size, spacing};
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
    }
}

fn group_heading<'a>(heading: Msg, lang: Language, palette: &Palette) -> Element<'a, Message> {
    text(lang.text(heading))
        .size(font_size::SMALL)
        .color(palette.dim_text)
        .into()
}

/// A label with the origin of its value below, and the control on the right.
fn field_row<'a>(
    label: &str,
    origin_line: Option<String>,
    control: Element<'a, Message>,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut names = column![paragraph(label.to_owned(), palette)].spacing(spacing::XS / 2.0);
    if let Some(line) = origin_line {
        names = names.push(dim(line, palette));
    }
    row![
        container(names).width(Length::Fill),
        container(control)
            .width(size::CONTROL_WIDTH)
            .align_x(alignment::Horizontal::Right),
    ]
    .spacing(spacing::XL)
    .align_y(alignment::Vertical::Center)
    .into()
}

/// The fields in groups: who, what, when, where.
fn form<'a>(state: &'a ReviewList, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let values = state.form();
    let text_field = |field: FormField, id: &'static str, hint: Option<Msg>, value: &'a str| {
        kit::text_field(
            hint.map_or("", |message| lang.text(message)),
            value,
            id,
            Some(move |typed| send(ReviewListMessage::FieldChanged(field, typed))),
            send(ReviewListMessage::Confirm),
            palette,
        )
    };
    let origin_of = |field: Field| {
        let item = state.selected()?;
        origin_text(&item.origins.of(field), state.is_edited(field), lang)
    };
    column![
        group_heading(Msg::ReviewListGroupWho, lang, palette),
        field_row(
            lang.text(Msg::ReviewListFieldSender),
            origin_of(Field::Sender),
            text_field(FormField::Sender, SENDER_ID, None, &values.sender),
            palette,
        ),
        group_heading(Msg::ReviewListGroupWhat, lang, palette),
        field_row(
            lang.text(Msg::ReviewListFieldKind),
            origin_of(Field::Kind),
            text_field(FormField::Kind, KIND_ID, None, &values.kind),
            palette,
        ),
        field_row(
            lang.text(Msg::ReviewListFieldNumber),
            origin_of(Field::Number),
            text_field(FormField::Number, NUMBER_ID, None, &values.number),
            palette,
        ),
        field_row(
            lang.text(Msg::ReviewListFieldAmount),
            origin_of(Field::Amount),
            text_field(
                FormField::Amount,
                AMOUNT_ID,
                Some(Msg::ReviewListHintAmount),
                &values.amount,
            ),
            palette,
        ),
        field_row(
            lang.text(Msg::ReviewListFieldTags),
            None,
            text_field(
                FormField::Tags,
                TAGS_ID,
                Some(Msg::ReviewListHintTags),
                &values.tags,
            ),
            palette,
        ),
        group_heading(Msg::ReviewListGroupWhen, lang, palette),
        field_row(
            lang.text(Msg::ReviewListFieldDate),
            origin_of(Field::Date),
            text_field(
                FormField::Date,
                DATE_ID,
                Some(Msg::ReviewListHintDate),
                &values.date,
            ),
            palette,
        ),
        group_heading(Msg::ReviewListGroupWhere, lang, palette),
        field_row(
            lang.text(Msg::ReviewListFieldPattern),
            None,
            text_field(FormField::Pattern, PATTERN_ID, None, &values.pattern),
            palette,
        ),
    ]
    .spacing(spacing::XS)
    .into()
}

/// "Will be filed as" with the path; the parts of it that are values of a
/// field in the accent color. Below it the placeholders a pattern may use.
fn target_line<'a>(
    state: &ReviewList,
    root: PreviewRoot<'_>,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let hint = dim(lang.text(Msg::ReviewListHintPattern), palette);
    let line = match shown_target(state, root) {
        Some((shown, marked)) => {
            let template = lang.text(Msg::ReviewListPreview);
            let (before, after) = template.split_once("{path}").unwrap_or((template, ""));
            let path = rich_text(highlight_spans(
                &shown,
                &marked,
                false,
                palette.text,
                palette,
            ))
            .size(font_size::BODY)
            .wrapping(Wrapping::WordOrGlyph)
            .width(Length::Fill);
            let mut line =
                row![paragraph(before.trim_end().to_owned(), palette), path].spacing(spacing::XS);
            if !after.trim().is_empty() {
                line = line.push(paragraph(after.trim_start().to_owned(), palette));
            }
            line.into()
        }
        None => match state.missing_field() {
            Some(field) => dim(
                fill_template(
                    lang.text(Msg::ReviewListPreviewMissingField),
                    &[("{field}", lang.text(field_name(field)))],
                ),
                palette,
            ),
            None => dim(lang.text(Msg::ReviewListPreviewNone), palette),
        },
    };
    column![line, hint].spacing(spacing::XS).into()
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
            ["~", "Archive", "Company A", "2024-03-09_invoice.pdf"]
                .join(std::path::MAIN_SEPARATOR_STR)
        );
        let texts: Vec<&str> = marked.into_iter().map(|range| &shown[range]).collect();
        assert_eq!(texts, ["Company A", "2024-03-09", "invoice"]);
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
