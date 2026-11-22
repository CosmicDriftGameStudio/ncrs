//! The reviews page of the settings: the documents that wait for a decision,
//! one row per question Hermes asks, with the answers as buttons. A group
//! unfolds to its files. Pure view over [`ReviewsPage`].

use std::path::Path;

use ncrs_config::palette::Palette;
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_ui_kit::format;
use ncrs_ui_kit::kit::Tone;
use ncrs_ui_kit::setting_rows::{Group, RowButton, SettingRow};

use super::review_reason::field_name;
use crate::message::{Message, ReviewsMessage};
use crate::reviews_page::{Answer, Question, ReviewsPage};

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
    let (title, rows) = if let Some(problem) = state.problem() {
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
    let can_file = !state.fileable(question).is_empty();
    let answers = question
        .answers(can_file)
        .into_iter()
        .enumerate()
        .map(|(position, answer)| {
            RowButton::new(
                lang.text(answer_text(question, answer)).to_owned(),
                Some(send(ReviewsMessage::Answer(question, answer))),
                if position == 0 {
                    Tone::Primary
                } else {
                    Tone::Secondary
                },
            )
        });
    SettingRow::listed_buttons(
        question_text(question, lang),
        Some(detail_text(state, question, lang)),
        Some(format::files_counted(state.members(question).len(), lang)),
        std::iter::once(fold).chain(answers).collect(),
        palette,
    )
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
        Question::Missing(field) => field_text(field, Msg::ReviewsPageQuestionMissing, lang),
        Question::Unsure(field) => field_text(field, Msg::ReviewsPageQuestionUnsure, lang),
    }
}

/// The line under the question: what Hermes read and where it would file for
/// the questions about the sender and the rule, else why Hermes asks.
fn detail_text(state: &ReviewsPage, question: Question, lang: Language) -> String {
    let template = match question {
        Question::Sender => Some(Msg::ReviewsPageDetailSender),
        Question::RuleUnconfirmed => Some(Msg::ReviewsPageDetailRule),
        _ => None,
    };
    let known = template
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
        Question::Missing(field) => field_text(field, Msg::ReviewsPageHintMissing, lang),
        Question::Unsure(field) => field_text(field, Msg::ReviewsPageHintUnsure, lang),
    }
}

fn answer_text(question: Question, answer: Answer) -> Msg {
    match (answer, question) {
        (Answer::Accept, Question::RuleUnconfirmed) => Msg::ReviewsPageAnswerOnce,
        (Answer::Accept, _) => Msg::ReviewsPageAnswerAccept,
        (Answer::SetFields, Question::Sender) => Msg::ReviewsPageAnswerOtherSender,
        (Answer::SetFields, Question::Date) => Msg::ReviewsPageAnswerEnterDate,
        (Answer::SetFields, _) => Msg::ReviewsPageAskChat,
        (Answer::CreateRule, _) => Msg::ReviewsPageAnswerCreateRule,
        (Answer::Exclude, _) => Msg::QuickFixExclude,
        (Answer::Later, _) => Msg::QuickFixLater,
    }
}

fn file_rows<'a>(
    state: &ReviewsPage,
    question: Question,
    lang: Language,
    palette: &Palette,
) -> Vec<SettingRow<'a, Message>> {
    let members = state.members_with_target(question);
    let mut rows: Vec<SettingRow<'a, Message>> = members
        .iter()
        .take(MAX_FILES_SHOWN)
        .map(|(item, target)| {
            let line = match target {
                Some(path) => {
                    let extension = item
                        .path
                        .extension()
                        .map_or_else(String::new, |found| format!(".{}", found.to_string_lossy()));
                    format::ellipsize_middle(&format!("{}{extension}", path.display()), PATH_CHARS)
                }
                None => folder_of(&item.path),
            };
            SettingRow::listed_buttons(
                file_name(&item.path),
                Some(line),
                None,
                vec![RowButton::new(
                    lang.text(Msg::HermesPagesShowFile),
                    Some(send(ReviewsMessage::ShowFile(item.path.clone()))),
                    Tone::Secondary,
                )],
                palette,
            )
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

pub(super) fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

pub(super) fn folder_of(path: &Path) -> String {
    format::ellipsize_middle(
        &path
            .parent()
            .map_or_else(String::new, |parent| parent.display().to_string()),
        FOLDER_CHARS,
    )
}
