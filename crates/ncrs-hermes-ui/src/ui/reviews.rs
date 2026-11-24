//! The reviews page of the settings: the documents that wait for a decision,
//! one row per question Hermes asks, with the answers as buttons. A group
//! unfolds to its files. Pure view over [`ReviewsPage`].

use std::path::Path;

use ncrs_config::palette::Palette;
use ncrs_filing::review_work::{AlreadyFiled, ReviewItem};
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_index::DocumentId;
use ncrs_ui_kit::format;
use ncrs_ui_kit::kit::{self, Tone};
use ncrs_ui_kit::setting_rows::{Group, RowButton, SettingRow};

use super::review_reason::field_name;
use crate::message::{Message, ReviewsMessage};
use crate::reviews_page::{Answer, Question, ReviewsPage, TakenBy};

/// A group lists this many files; the rest is a count.
pub const MAX_FILES_SHOWN: usize = 50;
const FOLDER_CHARS: usize = 60;
const PATH_CHARS: usize = 80;

fn send(message: ReviewsMessage) -> Message {
    Message::Reviews(message)
}

pub fn groups<'a>(
    state: &ReviewsPage,
    lang: Language,
    palette: &Palette,
) -> Vec<Group<'a, Message>> {
    let note = |text: String, problem: Option<String>| {
        vec![SettingRow::adding(Some(text), problem, Vec::new())]
    };
    let (title, mut rows) = if let Some(problem) = state.problem() {
        (String::new(), note(String::new(), Some(problem.to_owned())))
    } else if state.is_loading() && state.total() == 0 {
        (
            String::new(),
            note(lang.text(Msg::HermesPagesLoading).to_owned(), None),
        )
    } else if state.total() == 0 {
        (
            String::new(),
            note(lang.text(Msg::ReviewsPageNone).to_owned(), None),
        )
    } else {
        let title = fill_template(
            lang.text(Msg::ReviewsPageWaiting),
            &[("{count}", &format::grouped(state.total() as u64))],
        );
        let mut rows = Vec::new();
        for question in state.questions() {
            rows.push(group_row(state, question, lang, palette));
            if state.is_unfolded(question) {
                rows.extend(file_rows(state, question, lang, palette));
            }
        }
        (title, rows)
    };
    if let Some(files) = state.busy_with() {
        let working = fill_template(
            lang.text(Msg::ReviewsPageBusy),
            &[("{files}", &format::files_counted(files, lang))],
        );
        rows.insert(0, SettingRow::adding(Some(working), None, Vec::new()));
    }
    vec![Group {
        intro: None,
        title,
        rows,
    }]
}

fn group_row<'a>(
    state: &ReviewsPage,
    question: Question,
    lang: Language,
    palette: &Palette,
) -> SettingRow<'a, Message> {
    let fold = RowButton::new(
        if state.is_unfolded(question) {
            "\u{25be}"
        } else {
            "\u{25b8}"
        },
        Some(send(ReviewsMessage::ToggleGroup(question))),
        Tone::Secondary,
    );
    let answers = answer_buttons(state, question, None, lang);
    SettingRow::listed_buttons(
        question_text(question, lang),
        Some(detail_text(state, question, lang)),
        Some(format::files_counted(state.group_size(question), lang)),
        std::iter::once(fold).chain(answers).collect(),
        palette,
    )
}

/// The answers of a group, or of one document of it; only the first is
/// primary. A group's button says how many files it hits when that is not
/// all of them.
fn answer_buttons(
    state: &ReviewsPage,
    question: Question,
    document: Option<DocumentId>,
    lang: Language,
) -> Vec<RowButton<Message>> {
    let members = state.group_size(question);
    let offered: Vec<(Answer, usize)> = match document {
        Some(row_document) => state
            .file_answers(question, row_document)
            .into_iter()
            .map(|answer| (answer, members))
            .collect(),
        None => state.group_answers(question),
    };
    offered
        .into_iter()
        .enumerate()
        .map(|(position, (answer, fits))| {
            let message = match document {
                Some(_) if answer == Answer::FileTyped => document
                    .filter(|&candidate| state.typed_for(question, candidate).is_some())
                    .map(|candidate| ReviewsMessage::AnswerFile(question, answer, candidate)),
                Some(only) => Some(ReviewsMessage::AnswerFile(question, answer, only)),
                None => Some(ReviewsMessage::Answer(question, answer)),
            };
            let message = message.filter(|_| state.busy_with().is_none());
            let mut label = answer_text(question, answer, lang);
            if document.is_none() && fits < members {
                label = format!("{label} ({})", format::grouped(fits as u64));
            }
            RowButton::new(
                label,
                message.map(send),
                if position == 0 {
                    Tone::Primary
                } else {
                    Tone::Secondary
                },
            )
        })
        .collect()
}

/// The id of the input in the row of a document, to put the cursor there.
pub fn input_id(document: DocumentId) -> iced::widget::Id {
    iced::widget::Id::from(format!("review-input-{document}"))
}

fn field_text(field: ncrs_index::Field, template: Msg, lang: Language) -> String {
    fill_template(
        lang.text(template),
        &[("{field}", lang.text(field_name(field)))],
    )
}

fn question_text(question: Question, lang: Language) -> String {
    match question {
        Question::Sender => lang.text(Msg::ReviewsPageQuestionSender).to_owned(),
        Question::Date => lang.text(Msg::ReviewsPageQuestionDate).to_owned(),
        Question::TargetTaken => lang.text(Msg::ReviewsPageQuestionTargetTaken).to_owned(),
        Question::RuleUnconfirmed => lang.text(Msg::ReviewsPageQuestionRule).to_owned(),
        Question::NoRule => lang.text(Msg::ReviewsPageQuestionNoRule).to_owned(),
        Question::AlreadyFiled => lang.text(Msg::ReviewsPageQuestionAlreadyFiled).to_owned(),
        Question::Missing(field) => field_text(field, Msg::ReviewsPageQuestionMissing, lang),
        Question::Unsure(field) => field_text(field, Msg::ReviewsPageQuestionUnsure, lang),
    }
}

/// The line under the question: what Hermes read and where it would file for
/// the questions about the sender and the rule, else why Hermes asks.
fn detail_text(state: &ReviewsPage, question: Question, lang: Language) -> String {
    let detail_message = match question {
        Question::Sender => Some(Msg::ReviewsPageDetailSender),
        Question::RuleUnconfirmed => Some(Msg::ReviewsPageDetailRule),
        _ => None,
    };
    let known = detail_message
        .zip(state.single_sender(question))
        .zip(state.main_folder(question));
    if let Some(((template, sender), (folder, others))) = known {
        let mut text = fill_template(
            lang.text(template),
            &[
                ("{sender}", sender),
                (
                    "{folder}",
                    &format::ellipsize_middle(&folder.display().to_string(), FOLDER_CHARS),
                ),
            ],
        );
        if others > 0 {
            text = format!(
                "{text} \u{b7} {}",
                fill_template(
                    lang.text(Msg::ReviewsPageOtherTargets),
                    &[("{count}", &format::grouped(others as u64))],
                )
            );
        }
        return text;
    }
    match question {
        Question::Sender => lang.text(Msg::ReviewsPageHintSender).to_owned(),
        Question::Date => lang.text(Msg::ReviewsPageHintDate).to_owned(),
        Question::TargetTaken => lang.text(Msg::ReviewsPageHintTargetTaken).to_owned(),
        Question::RuleUnconfirmed => lang.text(Msg::ReviewsPageHintRule).to_owned(),
        Question::NoRule => lang.text(Msg::ReviewsPageHintNoRule).to_owned(),
        Question::AlreadyFiled => lang.text(Msg::ReviewsPageHintAlreadyFiled).to_owned(),
        Question::Missing(field) => field_text(field, Msg::ReviewsPageHintMissing, lang),
        Question::Unsure(field) => field_text(field, Msg::ReviewsPageHintUnsure, lang),
    }
}

fn answer_text(question: Question, answer: Answer, lang: Language) -> String {
    let msg = match (answer, question) {
        (Answer::Accept, Question::RuleUnconfirmed) => Msg::ReviewsPageAnswerOnce,
        (Answer::Accept, Question::Date) => Msg::ReviewsPageAnswerWithoutDate,
        (Answer::Accept, Question::TargetTaken) | (Answer::FileTyped, _) => {
            Msg::ReviewsPageAnswerFile
        }
        (Answer::Accept, Question::Missing(field)) => {
            return field_text(field, Msg::ReviewsPageAnswerWithoutField, lang);
        }
        (Answer::Accept, _) => Msg::ReviewsPageAnswerAccept,
        (Answer::EnterValue, Question::Sender) => Msg::ReviewsPageAnswerOtherSender,
        (Answer::EnterValue, _) => Msg::ReviewsPageAnswerEnterDate,
        (Answer::SetFields, _) => Msg::ReviewsPageAskChat,
        (Answer::CreateRule, _) => Msg::ReviewsPageAnswerCreateRule,
        (Answer::Exclude, _) => Msg::ReviewsPageAnswerIgnore,
        (Answer::DeleteDuplicate, _) => Msg::ReviewsPageAnswerDeleteDuplicate,
        (Answer::Overwrite, _) => Msg::ReviewsPageAnswerOverwrite,
        (Answer::KeepBoth, _) => Msg::ReviewsPageAnswerKeepBoth,
        (Answer::PickFolder, _) => Msg::ReviewsPageAnswerPickFolder,
    };
    lang.text(msg).to_owned()
}

/// The line under a file: how it compares with the file in its place when
/// that is taken, else where it would be filed.
fn file_line(
    state: &ReviewsPage,
    question: Question,
    item: &ReviewItem,
    target: Option<&Path>,
    lang: Language,
) -> String {
    if question == Question::TargetTaken {
        return taken_line(state, item, lang);
    }
    match target {
        Some(path) => {
            let extension = item
                .path
                .extension()
                .map_or_else(String::new, |found| format!(".{}", found.to_string_lossy()));
            format::ellipsize_middle(&format!("{}{extension}", path.display()), PATH_CHARS)
        }
        None => folder_of(&item.path),
    }
}

fn taken_line(state: &ReviewsPage, item: &ReviewItem, lang: Language) -> String {
    let free = || lang.text(Msg::ReviewsPageTakenFree).to_owned();
    match (
        state.taken_by(item.document_id),
        state.comparison(item.document_id),
    ) {
        (TakenBy::Unknown, _) => lang.text(Msg::ReviewsPageTakenComparing).to_owned(),
        (_, None) => free(),
        (_, Some(found)) => {
            let verdict = if found.same_content {
                Msg::ReviewsPageSameContent
            } else {
                Msg::ReviewsPageOtherContent
            };
            fill_template(
                lang.text(Msg::ReviewsPageTakenCompare),
                &[
                    ("{new_size}", &format::size_shown(found.incoming.size, lang)),
                    ("{new_date}", &format::day_in(found.incoming.modified, lang)),
                    ("{old_size}", &format::size_shown(found.existing.size, lang)),
                    ("{old_date}", &format::day_in(found.existing.modified, lang)),
                    ("{verdict}", lang.text(verdict)),
                ],
            )
        }
    }
}

fn file_rows<'a>(
    state: &ReviewsPage,
    question: Question,
    lang: Language,
    palette: &Palette,
) -> Vec<SettingRow<'a, Message>> {
    if question == Question::AlreadyFiled {
        return already_filed_rows(state, lang, palette);
    }
    let members = state.members_with_target(question);
    let mut rows: Vec<SettingRow<'a, Message>> = members
        .iter()
        .take(MAX_FILES_SHOWN)
        .map(|(item, target)| {
            let line = Some(file_line(state, question, item, *target, lang));
            let buttons = answer_buttons(state, question, Some(item.document_id), lang);
            let row = if question.is_typed() {
                typed_row(state, question, item, (line, buttons), lang, palette)
            } else {
                SettingRow::listed_buttons(file_name(&item.path), line, None, buttons, palette)
            };
            row.with_label_action(send(ReviewsMessage::OpenFile(item.path.clone())))
                .indented()
        })
        .collect();
    if members.len() > MAX_FILES_SHOWN {
        rows.push(
            SettingRow::adding(
                Some(fill_template(
                    lang.text(Msg::HermesPagesMoreFiles),
                    &[(
                        "{count}",
                        &format::grouped((members.len() - MAX_FILES_SHOWN) as u64),
                    )],
                )),
                None,
                Vec::new(),
            )
            .indented(),
        );
    }
    rows
}

/// The inbox files the archive already holds: the line under each says where
/// the same content lies.
fn already_filed_rows<'a>(
    state: &ReviewsPage,
    lang: Language,
    palette: &Palette,
) -> Vec<SettingRow<'a, Message>> {
    let found = state.already_filed();
    let mut rows: Vec<SettingRow<'a, Message>> = found
        .iter()
        .take(MAX_FILES_SHOWN)
        .map(|file| {
            let line = already_filed_line(file, lang);
            let buttons = Question::AlreadyFiled
                .repeat_answers()
                .iter()
                .enumerate()
                .map(|(position, &answer)| {
                    RowButton::new(
                        answer_text(Question::AlreadyFiled, answer, lang),
                        Some(send(ReviewsMessage::AnswerRepeat(
                            answer,
                            file.path.clone(),
                        ))),
                        if position == 0 {
                            Tone::Primary
                        } else {
                            Tone::Secondary
                        },
                    )
                })
                .collect();
            SettingRow::listed_buttons(file_name(&file.path), Some(line), None, buttons, palette)
                .with_label_action(send(ReviewsMessage::OpenFile(file.path.clone())))
                .indented()
        })
        .collect();
    if found.len() > MAX_FILES_SHOWN {
        rows.push(
            SettingRow::adding(
                Some(fill_template(
                    lang.text(Msg::HermesPagesMoreFiles),
                    &[(
                        "{count}",
                        &format::grouped((found.len() - MAX_FILES_SHOWN) as u64),
                    )],
                )),
                None,
                Vec::new(),
            )
            .indented(),
        );
    }
    rows
}

fn already_filed_line(file: &AlreadyFiled, lang: Language) -> String {
    fill_template(
        lang.text(Msg::ReviewsPageAlreadyFiledLine),
        &[(
            "{path}",
            &format::ellipsize_middle(&file.keeper_path.display().to_string(), PATH_CHARS),
        )],
    )
}

pub(in crate::ui) fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

pub(in crate::ui) fn folder_of(path: &Path) -> String {
    format::ellipsize_middle(
        &path
            .parent()
            .map_or_else(String::new, |parent| parent.display().to_string()),
        FOLDER_CHARS,
    )
}

/// A file of the date or sender question: an input for the value that files
/// it, then the buttons.
fn typed_row<'a>(
    state: &ReviewsPage,
    question: Question,
    item: &ReviewItem,
    (line, buttons): (Option<String>, Vec<RowButton<Message>>),
    lang: Language,
    palette: &Palette,
) -> SettingRow<'a, Message> {
    let document = item.document_id;
    let typed = state.input_of(question, item);
    let (placeholder, invalid) = match question {
        Question::Date => (
            lang.text(Msg::ReviewsPageInputDate),
            !typed.trim().is_empty() && state.typed_for(question, document).is_none(),
        ),
        _ => (lang.text(Msg::ReviewsPageInputSender), false),
    };
    let field = || {
        kit::text_field(
            placeholder,
            &typed,
            input_id(document),
            Some(move |text| send(ReviewsMessage::InputChanged(document, text))),
            send(ReviewsMessage::AnswerFile(
                question,
                Answer::FileTyped,
                document,
            )),
            palette,
        )
    };
    let row =
        SettingRow::listed_field_buttons(file_name(&item.path), line, field, buttons, palette);
    SettingRow {
        problem: invalid.then(|| lang.text(Msg::ReviewsPageInvalidDate).to_owned()),
        ..row
    }
}
