//! The review list: the documents no confirmed rule could file, each with the
//! reason and the fields, and a form to change the fields before the document
//! is filed. A page of the Hermes card. Pure view over [`ReviewList`].
//!
//! The text of a document never shows up here as anything but a value in a
//! field the user can read and change.

use iced::widget::{column, row};
use iced::{alignment, Element, Size};
use ncrs_index::{Field, ReviewReason, TaxContext};

use super::format;
use super::frame::{self, DialogSize, Frame, KeyHints};
use super::hermes::{self, Badges};
use super::kit::{self, dim, error_line, form_row, paragraph, Columns, Tone};
use super::theme::spacing;
use crate::app::fill_template;
use crate::i18n::{Language, Msg};
use crate::messages::{HermesFocus, HermesPage, Message, ReviewListMessage};
use crate::palette::Palette;
use crate::review_list::{FiledNotice, FormField, Problem, ReviewList, VISIBLE_ROWS};
use crate::review_work::{ReviewItem, TextState};

pub const SENDER_ID: &str = "review-sender";
const KIND_ID: &str = "review-kind";
const DATE_ID: &str = "review-date";
const NUMBER_ID: &str = "review-number";
const AMOUNT_ID: &str = "review-amount";
const TAGS_ID: &str = "review-tags";
const PATTERN_ID: &str = "review-pattern";

const SIZE: DialogSize = DialogSize::Card;

/// How the width of a row is shared: file, reason, sender, kind, date.
const PORTIONS: [u16; 5] = [5, 6, 4, 3, 3];

/// The tax context choices, in the order of the segmented control.
const TAX_CHOICES: [(Msg, Option<TaxContext>); 4] = [
    (Msg::ReviewListTaxOpen, None),
    (Msg::ReviewListTaxCompany, Some(TaxContext::Company)),
    (Msg::ReviewListTaxPrivate, Some(TaxContext::Private)),
    (Msg::ReviewListTaxNone, Some(TaxContext::None)),
];

fn send(message: ReviewListMessage) -> Message {
    Message::ReviewList(message)
}

pub fn view<'a>(
    state: &'a ReviewList,
    window: Size,
    badges: Badges,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let line_chars = frame::content_chars(SIZE, window, frame::SIDEBAR_ROOM);
    let mut body =
        column![paragraph(lang.text(Msg::ReviewListExplain), palette)].spacing(spacing::LINE_GAP);

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
        body = body.push(list(state, line_chars, badges.focus, lang, palette));
        body = body.push(form(state, lang, palette));
        body = body.push(preview(state, lang, palette));
    }
    if let Some(problem) = state.problem() {
        body = body.push(error_line(problem_text(problem, lang), palette));
    }
    if let Some(filed) = state.filed().filter(|filed| filed.rule_created) {
        body = body.push(dim(
            fill_template(
                lang.text(Msg::ReviewListRuleCreated),
                &[("{sender}", &filed.sender)],
            ),
            palette,
        ));
    }

    let mut frame = Frame::new(
        lang.text(Msg::ActionReviewList),
        body,
        SIZE,
        window,
        lang,
        palette,
    )
    .title_theme(lang.text(Msg::ActionReviewListTheme))
    .buttons(buttons(state, lang, palette))
    .sidebar(hermes::nav(HermesPage::Review, badges, lang, palette))
    .on_close(send(ReviewListMessage::Close))
    .hints(KeyHints {
        enter: true,
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

/// Why the document is here, in words; the field a reason names is spelled out.
pub fn reason_text(reason: ReviewReason, text_state: TextState, lang: Language) -> String {
    let named = |template: Msg, field: Field| {
        fill_template(
            lang.text(template),
            &[("{field}", lang.text(field_name(field)))],
        )
    };
    match reason {
        ReviewReason::NoMatchingRule => lang
            .text(match text_state {
                TextState::NotReadable => Msg::ReviewListReasonImageNoText,
                TextState::NoneFound => Msg::ReviewListReasonNoText,
                TextState::Read => Msg::ReviewListReasonNoRule,
            })
            .to_owned(),
        ReviewReason::UnconfirmedRule => lang.text(Msg::ReviewListReasonUnconfirmed).to_owned(),
        ReviewReason::TargetExists => lang.text(Msg::ReviewListReasonTargetExists).to_owned(),
        ReviewReason::MissingField(field) => named(Msg::ReviewListReasonMissingField, field),
        ReviewReason::LowConfidence(field) => named(Msg::ReviewListReasonLowConfidence, field),
    }
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
            lang.text(Msg::ReviewListColumnSender),
            lang.text(Msg::ReviewListColumnKind),
            lang.text(Msg::ReviewListColumnDate),
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
            let color = if selected && focus == HermesFocus::Content {
                palette.cursor_text
            } else {
                palette.text
            };
            let content = row![
                columns.path_cell(0, &file_name(item), color),
                columns.cell(1, &reason_text(item.reason, item.text_state, lang), color),
                columns.cell(2, item.fields.sender.as_deref().unwrap_or_default(), color),
                columns.cell(
                    3,
                    item.fields
                        .kind
                        .as_deref()
                        .map_or_else(Default::default, |kind| lang.kind_label(kind))
                        .as_ref(),
                    color
                ),
                columns.cell(
                    4,
                    &item
                        .fields
                        .date
                        .map(|date| date.to_string())
                        .unwrap_or_default(),
                    color
                ),
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
    let tax_options: Vec<(&str, Message)> = TAX_CHOICES
        .iter()
        .map(|(label, choice)| {
            (
                lang.text(*label),
                send(ReviewListMessage::TaxContextChosen(*choice)),
            )
        })
        .collect();
    let selected_tax = TAX_CHOICES
        .iter()
        .position(|(_, choice)| *choice == values.tax_context)
        .unwrap_or_default();
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
        row_of(
            Msg::ReviewListFieldTaxContext,
            None,
            kit::segmented(&tax_options, selected_tax, true, palette),
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

fn preview<'a>(state: &ReviewList, lang: Language, palette: &Palette) -> Element<'a, Message> {
    match state.preview() {
        Some(path) => paragraph(
            fill_template(
                lang.text(Msg::ReviewListPreview),
                &[("{path}", &path.display().to_string())],
            ),
            palette,
        ),
        None => dim(lang.text(Msg::ReviewListPreviewNone), palette),
    }
}

fn buttons<'a>(state: &ReviewList, lang: Language, palette: &Palette) -> Vec<Element<'a, Message>> {
    let mut items = vec![kit::button(
        lang.text(Msg::ReviewListClose),
        Some(send(ReviewListMessage::Close)),
        Tone::Secondary,
        palette,
    )];
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
