//! The dialog of a target and the question before a target is removed.

use std::path::Path;

use iced::widget::Row;
use iced::{alignment, Element, Size};
use ncrs_config::config::Filing;
use ncrs_config::palette::Palette;
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_ui_kit::frame::{DialogSize, Frame, KeyHints};
use ncrs_ui_kit::hermes_kit::{self as page_kit, GAP_S};
use ncrs_ui_kit::kit::{self, Tone};

use crate::message::Message;
use crate::target_dialog::{TargetDialog, TargetProblem, TargetRemoval, TargetsMessage};

pub const NAME_FIELD_ID: &str = "target-dialog-name";
pub const FOLDER_FIELD_ID: &str = "target-dialog-folder";

fn send(message: TargetsMessage) -> Message {
    Message::Targets(message)
}

pub fn view<'a>(
    dialog: &TargetDialog,
    filing: &Filing,
    home: &Path,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let problems = dialog.problems(filing, home);
    let submit = send(TargetsMessage::Save);
    let name = page_kit::form_row(
        lang.text(Msg::SortingFieldTargetName),
        page_kit::text_field(
            "",
            &dialog.name,
            NAME_FIELD_ID,
            Some(|typed| send(TargetsMessage::Name(typed))),
            submit.clone(),
            palette,
        ),
        None,
        palette,
    );
    let folder_field = Row::new()
        .spacing(GAP_S)
        .align_y(alignment::Vertical::Center)
        .push(page_kit::text_field(
            "",
            &dialog.folder,
            FOLDER_FIELD_ID,
            Some(|typed| send(TargetsMessage::Folder(typed))),
            submit,
            palette,
        ))
        .push(kit::button(
            lang.text(Msg::SettingsChoose),
            Some(send(TargetsMessage::PickFolder)),
            Tone::Secondary,
            palette,
        ));
    let folder = page_kit::form_row(
        lang.text(Msg::SortingFieldFolder),
        folder_field.into(),
        problems
            .first()
            .map(|problem| page_kit::error_line(problem_text(problem, lang), palette)),
        palette,
    );
    let title = if dialog.replacing.is_some() {
        Msg::SortingTargetDialogEdit
    } else {
        Msg::SortingTargetDialogNew
    };
    Frame::new(
        lang.text(title),
        iced::widget::column![name, folder].spacing(GAP_S),
        DialogSize::Wide,
        window,
        lang,
        palette,
    )
    .buttons(vec![
        kit::button(
            lang.text(Msg::DialogCancel),
            Some(send(TargetsMessage::Cancel)),
            Tone::Secondary,
            palette,
        ),
        kit::button(
            lang.text(Msg::SavedQueriesSave),
            dialog
                .can_save(filing, home)
                .then(|| send(TargetsMessage::Save)),
            Tone::Primary,
            palette,
        ),
    ])
    .on_close(send(TargetsMessage::Cancel))
    .hints(KeyHints {
        enter: true,
        tab: true,
    })
    .into_element()
}

fn problem_text(problem: &TargetProblem, lang: Language) -> String {
    match problem {
        TargetProblem::FolderMissing => lang.text(Msg::SortingProblemFolderMissing).to_owned(),
        TargetProblem::Overlaps(other) => fill_template(
            lang.text(Msg::SortingProblemTargetOverlap),
            &[("{name}", other)],
        ),
    }
}

/// The question before a target and its rules are removed.
pub fn removal<'a>(
    asked: &TargetRemoval,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let question = if asked.rules == 0 {
        fill_template(
            lang.text(Msg::SortingRemoveTargetQuestionPlain),
            &[("{name}", &asked.name)],
        )
    } else {
        fill_template(
            lang.text(Msg::SortingRemoveTargetQuestion),
            &[
                ("{name}", &asked.name),
                ("{count}", &asked.rules.to_string()),
            ],
        )
    };
    Frame::new(
        lang.text(Msg::SortingRemoveTargetTitle),
        kit::paragraph(question, palette),
        DialogSize::Small,
        window,
        lang,
        palette,
    )
    .buttons(vec![
        kit::button(
            lang.text(Msg::DialogCancel),
            Some(send(TargetsMessage::CancelRemove)),
            Tone::Secondary,
            palette,
        ),
        kit::enter_button(
            lang.text(Msg::SortingRemove),
            (!asked.busy).then(|| send(TargetsMessage::ConfirmRemove)),
            Tone::Danger,
            palette,
        ),
    ])
    .on_close(send(TargetsMessage::CancelRemove))
    .hints(KeyHints {
        enter: true,
        tab: false,
    })
    .into_element()
}
