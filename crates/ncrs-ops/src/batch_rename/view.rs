//! The batch rename dialog.

use iced::widget::{column, container, row, Space};
use iced::{alignment, Element, Length, Size};

use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_ui_kit::format;
use ncrs_ui_kit::frame::{content_chars, DialogSize, Frame, KeyHints};

use ncrs_ui_kit::kit::{self, Check, Columns, ListView, Tone};
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::theme::spacing;

use super::pattern::{Case, Field, Problem};
use super::state::{BatchRename, BatchRenameMessage, Token, VISIBLE_ROWS};
use crate::message::OpsMessage;

pub const PATTERN_FIELD_ID: &str = "batch-rename-pattern";
pub const START_FIELD_ID: &str = "batch-rename-start";
pub const DIGITS_FIELD_ID: &str = "batch-rename-digits";
pub const FIND_FIELD_ID: &str = "batch-rename-find";
pub const REPLACE_FIELD_ID: &str = "batch-rename-replace";

/// Old name, new name, problem.
const PORTIONS: [u16; 3] = [4, 4, 3];

const NUMBER_FIELD_WIDTH: f32 = 72.0;

fn send(message: BatchRenameMessage) -> OpsMessage {
    OpsMessage::BatchRename(message)
}

fn field_label(field: Field, lang: Language) -> &'static str {
    lang.text(match field {
        Field::Modified => Msg::BatchRenameFieldModified,
        Field::Sender => Msg::BatchRenameFieldSender,
        Field::DocumentDate => Msg::BatchRenameFieldDocumentDate,
        Field::Kind => Msg::BatchRenameFieldKind,
        Field::Amount => Msg::BatchRenameFieldAmount,
    })
}

/// What is wrong with a row, in words.
pub fn problem_text(problem: &Problem, lang: Language) -> String {
    match problem {
        Problem::Missing(field) => fill_template(
            lang.text(Msg::BatchRenameProblemMissing),
            &[("{field}", field_label(*field, lang))],
        ),
        Problem::Name(name_problem) => name_problem.text(lang),
        Problem::SameAs(line) => fill_template(
            lang.text(Msg::BatchRenameProblemSame),
            &[("{n}", &line.to_string())],
        ),
        Problem::Exists => lang.text(Msg::BatchRenameProblemExists).to_owned(),
    }
}

fn token_label(token: Token, lang: Language) -> &'static str {
    lang.text(match token {
        Token::Name => Msg::BatchRenameTokenName,
        Token::PartOfName => Msg::BatchRenameTokenPart,
        Token::Counter => Msg::BatchRenameTokenCounter,
        Token::Modified => Msg::BatchRenameTokenModified,
        Token::Taken => Msg::BatchRenameTokenTaken,
        Token::Extension => Msg::BatchRenameTokenExtension,
        Token::Sender => Msg::BatchRenameTokenSender,
        Token::DocumentDate => Msg::BatchRenameTokenDocumentDate,
        Token::Kind => Msg::BatchRenameTokenKind,
        Token::Amount => Msg::BatchRenameTokenAmount,
    })
}

fn token_row<'a>(
    label: &str,
    tokens: &[Token],
    lang: Language,
    palette: &Palette,
) -> iced::widget::Row<'a, OpsMessage> {
    let mut line = row![kit::dim(label.to_owned(), palette)]
        .spacing(spacing::LINE_GAP)
        .align_y(alignment::Vertical::Center);
    for token in tokens {
        line = line.push(kit::button(
            token_label(*token, lang),
            Some(send(BatchRenameMessage::Insert(*token))),
            Tone::Secondary,
            palette,
        ));
    }
    line
}

fn labelled<'a>(
    label: &str,
    field: Element<'a, OpsMessage>,
    palette: &Palette,
) -> iced::widget::Column<'a, OpsMessage> {
    column![kit::dim(label.to_owned(), palette), field].spacing(spacing::XS)
}

fn regex_check<'a>(state: Check, lang: Language, palette: &Palette) -> Element<'a, OpsMessage> {
    row![
        kit::checkbox(state, Some(send(BatchRenameMessage::ToggleRegex)), palette),
        kit::paragraph(lang.text(Msg::BatchRenameRegex).to_owned(), palette),
    ]
    .spacing(spacing::XS)
    .into()
}

fn hermes_line<'a>(
    dialog: &BatchRename,
    lang: Language,
    palette: &Palette,
) -> Option<Element<'a, OpsMessage>> {
    let pattern = dialog.suggestion.as_deref()?;
    let text = fill_template(lang.text(Msg::BatchRenameHermes), &[("{pattern}", pattern)]);
    Some(
        row![
            kit::dim(text, palette),
            kit::button(
                lang.text(Msg::BatchRenameHermesUse),
                Some(send(BatchRenameMessage::UseHermes)),
                Tone::Secondary,
                palette,
            ),
        ]
        .spacing(spacing::LINE_GAP)
        .align_y(alignment::Vertical::Center)
        .into(),
    )
}

fn settings_row<'a>(
    dialog: &BatchRename,
    lang: Language,
    palette: &Palette,
) -> Element<'a, OpsMessage> {
    let number =
        |label: Msg, value: &str, id: &'static str, typed: fn(String) -> BatchRenameMessage| {
            container(labelled(
                lang.text(label),
                kit::mono_text_field(
                    "",
                    value,
                    id,
                    Some(move |text| send(typed(text))),
                    send(BatchRenameMessage::Confirm),
                    palette,
                ),
                palette,
            ))
            .width(NUMBER_FIELD_WIDTH)
        };
    let text =
        |label: Msg, value: &str, id: &'static str, typed: fn(String) -> BatchRenameMessage| {
            container(labelled(
                lang.text(label),
                kit::mono_text_field(
                    "",
                    value,
                    id,
                    Some(move |entered| send(typed(entered))),
                    send(BatchRenameMessage::Confirm),
                    palette,
                ),
                palette,
            ))
            .width(Length::Fill)
        };
    let selected = match dialog.case {
        Case::Keep => 0,
        Case::Lower => 1,
        Case::Upper => 2,
    };
    let case = labelled(
        lang.text(Msg::BatchRenameCase),
        kit::segmented(
            &[
                (
                    lang.text(Msg::BatchRenameCaseKeep),
                    send(BatchRenameMessage::SetCase(Case::Keep)),
                ),
                (
                    lang.text(Msg::BatchRenameCaseLower),
                    send(BatchRenameMessage::SetCase(Case::Lower)),
                ),
                (
                    lang.text(Msg::BatchRenameCaseUpper),
                    send(BatchRenameMessage::SetCase(Case::Upper)),
                ),
            ],
            selected,
            true,
            palette,
        ),
        palette,
    );
    row![
        number(
            Msg::BatchRenameStart,
            &dialog.start,
            START_FIELD_ID,
            BatchRenameMessage::StartInput
        ),
        number(
            Msg::BatchRenameDigits,
            &dialog.digits,
            DIGITS_FIELD_ID,
            BatchRenameMessage::DigitsInput
        ),
        text(
            Msg::BatchRenameFind,
            &dialog.find,
            FIND_FIELD_ID,
            BatchRenameMessage::FindInput
        ),
        text(
            Msg::BatchRenameReplace,
            &dialog.replace,
            REPLACE_FIELD_ID,
            BatchRenameMessage::ReplaceInput
        ),
        case,
    ]
    .spacing(spacing::LINE_GAP)
    .align_y(alignment::Vertical::Top)
    .into()
}

fn table<'a>(
    dialog: &BatchRename,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, OpsMessage> {
    let columns = Columns {
        portions: &PORTIONS,
        line_chars: content_chars(DialogSize::Large, window, 0.0),
    };
    let header = columns.header(
        &[
            lang.text(Msg::BatchRenameColumnOld),
            lang.text(Msg::BatchRenameColumnNew),
            lang.text(Msg::BatchRenameColumnProblem),
        ],
        palette,
    );
    let len = dialog.preview.rows.len();
    let offset = dialog.window.offset(len, VISIBLE_ROWS);
    let rows = dialog
        .preview
        .rows
        .iter()
        .skip(offset)
        .take(VISIBLE_ROWS)
        .map(|shown| {
            let problem = shown
                .problem
                .as_ref()
                .map(|found| problem_text(found, lang))
                .unwrap_or_default();
            columns.row_colored(&[
                (shown.old_name.as_str(), palette.dim_text),
                (shown.new_name.as_str(), palette.text),
                (problem.as_str(), palette.error),
            ])
        })
        .collect();
    kit::scroll_list(
        header,
        rows,
        ListView {
            len,
            visible: VISIBLE_ROWS,
            offset,
        },
        |delta| send(BatchRenameMessage::Scrolled(delta)),
        palette,
    )
}

pub fn view<'a>(
    dialog: &'a BatchRename,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, OpsMessage> {
    let pattern = labelled(
        lang.text(Msg::BatchRenameNewName),
        kit::mono_text_field(
            "",
            &dialog.pattern,
            PATTERN_FIELD_ID,
            Some(|text| send(BatchRenameMessage::PatternInput(text))),
            send(BatchRenameMessage::Confirm),
            palette,
        ),
        palette,
    );
    let add = token_row(
        lang.text(Msg::BatchRenameAdd),
        &[
            Token::Name,
            Token::PartOfName,
            Token::Counter,
            Token::Modified,
            Token::Taken,
            Token::Extension,
        ],
        lang,
        palette,
    );
    let mut document = token_row(
        lang.text(Msg::BatchRenameDocument),
        &[
            Token::Sender,
            Token::DocumentDate,
            Token::Kind,
            Token::Amount,
        ],
        lang,
        palette,
    )
    .push(Space::new().width(Length::Fill));
    if let Some(hermes) = hermes_line(dialog, lang, palette) {
        document = document.push(hermes);
    }
    let regex_state = if dialog.regex { Check::On } else { Check::Off };
    let mut body = column![
        pattern,
        add,
        document,
        settings_row(dialog, lang, palette),
        regex_check(regex_state, lang, palette),
        table(dialog, window, lang, palette),
    ]
    .spacing(spacing::LINE_GAP);
    let clashes = dialog.preview.clashes();
    if let Some(reason) = dialog.preview.regex_error.as_deref() {
        body = body.push(kit::error_line(
            fill_template(
                lang.text(Msg::BatchRenameRegexError),
                &[("{reason}", reason)],
            ),
            palette,
        ));
    } else if clashes > 0 {
        body = body.push(kit::error_line(
            fill_template(
                lang.text(Msg::BatchRenameClashes),
                &[("{count}", &clashes.to_string())],
            ),
            palette,
        ));
    }
    let title = fill_template(
        lang.text(Msg::BatchRenameTitle),
        &[("{count}", &dialog.entries.len().to_string())],
    );
    let folder_room =
        content_chars(DialogSize::Large, window, 0.0).saturating_sub(title.chars().count() + 14);
    let folder = format::ellipsize_middle(&dialog.folder.display().to_string(), folder_room);
    Frame::new(title, body, DialogSize::Large, window, lang, palette)
        .subtitle(folder)
        .buttons(vec![
            kit::button(
                lang.text(Msg::DialogCancel),
                Some(send(BatchRenameMessage::Cancel)),
                Tone::Secondary,
                palette,
            ),
            kit::enter_button(
                lang.text(Msg::BatchRenameConfirm),
                dialog
                    .preview
                    .can_apply()
                    .then(|| send(BatchRenameMessage::Confirm)),
                Tone::Primary,
                palette,
            ),
        ])
        .on_close(send(BatchRenameMessage::Cancel))
        .hints(KeyHints {
            enter: true,
            tab: false,
        })
        .into_element()
}
