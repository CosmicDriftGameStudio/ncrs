//! The permissions dialog.

use iced::widget::{column, row};
use iced::{Element, Size};

use ncrs_fs::permissions::{ForFiles, ForFolders};
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::frame::{DialogSize, Frame, KeyHints};
use ncrs_ui_kit::kit::{self, Check, Tone};
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::permission_grid::{self, Grid};
use ncrs_ui_kit::theme::spacing;

use super::{PermissionsDialog, READ_ONLY, SETGID, SETUID, STICKY};
use crate::message::{OpsMessage, PermissionsMessage};

/// The id of the octal field.
pub const OCTAL_FIELD_ID: &str = "permissions-octal";

fn send(message: PermissionsMessage) -> OpsMessage {
    OpsMessage::Permissions(message)
}

pub fn view<'a>(
    dialog: &PermissionsDialog,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, OpsMessage> {
    let mut body = column![].spacing(spacing::LINE_GAP);
    if let Some(line) = dialog.owner_and_group(lang) {
        body = body.push(kit::dim(line, palette));
    }
    body = if dialog.is_windows() {
        body.push(permission_grid::labelled_box(
            lang.text(Msg::PermissionsWindowsReadOnly),
            dialog.state(READ_ONLY),
            send(PermissionsMessage::Toggle(READ_ONLY)),
            palette,
        ))
        .push(kit::dim(lang.text(Msg::PermissionsWindowsNote), palette))
    } else {
        rights(body, dialog, lang, palette)
    };
    if dialog.has_folder() {
        body = body.push(contents(dialog, lang, palette));
    }

    Frame::new(
        dialog.title(lang),
        body,
        DialogSize::Medium,
        window,
        lang,
        palette,
    )
    .buttons(vec![
        kit::button(
            lang.text(Msg::DialogCancel),
            Some(send(PermissionsMessage::Cancel)),
            Tone::Secondary,
            palette,
        ),
        kit::enter_button(
            lang.text(Msg::PermissionsApply),
            dialog
                .octal_is_valid()
                .then(|| send(PermissionsMessage::Confirm)),
            Tone::Primary,
            palette,
        ),
    ])
    .on_close(send(PermissionsMessage::Cancel))
    .hints(KeyHints {
        enter: true,
        tab: false,
    })
    .into_element()
}

/// The grid, the octal line, the note and the folded extra bits.
fn rights<'a>(
    mut body: iced::widget::Column<'a, OpsMessage>,
    dialog: &PermissionsDialog,
    lang: Language,
    palette: &Palette,
) -> iced::widget::Column<'a, OpsMessage> {
    let state = |row: usize, column: usize| dialog.state(row * 3 + column);
    let shown = Grid {
        headings: [
            lang.text(Msg::PermissionsRead),
            lang.text(Msg::PermissionsWrite),
            lang.text(Msg::PermissionsExecute),
        ],
        labels: [
            lang.text(Msg::PermissionsRowOwner),
            lang.text(Msg::PermissionsRowGroup),
            lang.text(Msg::PermissionsRowOthers),
        ],
        states: [0, 1, 2].map(|row| [0, 1, 2].map(|column| state(row, column))),
    };
    let grid = permission_grid::grid(
        &shown,
        |row, column| send(PermissionsMessage::Toggle(row * 3 + column)),
        palette,
    );
    let octal_text = dialog.octal_text(lang);
    let symbolic = dialog.symbolic().unwrap_or_default();
    let octal = permission_grid::octal_row(
        &permission_grid::OctalTexts {
            label: lang.text(Msg::PermissionsOctal),
            value: &octal_text,
            symbolic: &symbolic,
            note: lang.text(Msg::PermissionsOctalNote),
        },
        OCTAL_FIELD_ID,
        |text| send(PermissionsMessage::OctalInput(text)),
        send(PermissionsMessage::Confirm),
        palette,
    );
    body = body
        .push(grid)
        .push(octal)
        .push(kit::dim(lang.text(Msg::PermissionsCheckboxNote), palette));

    let chevron = if dialog.advanced_open() { "v" } else { ">" };
    body = body.push(kit::button(
        &format!("{chevron} {}", lang.text(Msg::PermissionsAdvanced)),
        Some(send(PermissionsMessage::ToggleAdvanced)),
        Tone::Secondary,
        palette,
    ));
    if dialog.advanced_open() {
        let special = |label: &str, index: usize| {
            permission_grid::labelled_box(
                label,
                dialog.state(index),
                send(PermissionsMessage::Toggle(index)),
                palette,
            )
        };
        body = body.push(
            row![
                special(lang.text(Msg::PermissionsSetuid), SETUID),
                special(lang.text(Msg::PermissionsSetgid), SETGID),
                special(lang.text(Msg::PermissionsSticky), STICKY),
            ]
            .spacing(spacing::XL),
        );
    }
    body
}

/// The switch for the contents of the folders and, when it is on, what
/// happens to the files and the folders in them.
fn contents<'a>(
    dialog: &PermissionsDialog,
    lang: Language,
    palette: &Palette,
) -> Element<'a, OpsMessage> {
    let check = if dialog.contents() {
        Check::On
    } else {
        Check::Off
    };
    let mut block = column![permission_grid::labelled_box(
        lang.text(Msg::PermissionsApplyToContents),
        check,
        send(PermissionsMessage::ToggleContents),
        palette,
    )]
    .spacing(spacing::LINE_GAP);
    if dialog.contents() {
        let files = [
            (Msg::PermissionsChoiceAsAbove, ForFiles::AsAbove),
            (
                Msg::PermissionsChoiceWithoutExecute,
                ForFiles::WithoutExecute,
            ),
            (Msg::PermissionsChoiceUnchanged, ForFiles::Unchanged),
        ];
        let file_options: Vec<(&str, OpsMessage)> = files
            .iter()
            .map(|(label, choice)| {
                (
                    lang.text(*label),
                    send(PermissionsMessage::SetForFiles(*choice)),
                )
            })
            .collect();
        let file_selected = files
            .iter()
            .position(|(_, choice)| *choice == dialog.for_files())
            .unwrap_or_default();
        let folders = [
            (Msg::PermissionsChoiceAsAbove, ForFolders::AsAbove),
            (Msg::PermissionsChoiceUnchanged, ForFolders::Unchanged),
        ];
        let folder_options: Vec<(&str, OpsMessage)> = folders
            .iter()
            .map(|(label, choice)| {
                (
                    lang.text(*label),
                    send(PermissionsMessage::SetForFolders(*choice)),
                )
            })
            .collect();
        let folder_selected = folders
            .iter()
            .position(|(_, choice)| *choice == dialog.for_folders())
            .unwrap_or_default();
        block = block
            .push(permission_grid::labelled_row(
                lang.text(Msg::PermissionsRowFiles),
                kit::segmented(&file_options, file_selected, true, palette),
                palette,
            ))
            .push(permission_grid::labelled_row(
                lang.text(Msg::PermissionsRowFolders),
                kit::segmented(&folder_options, folder_selected, true, palette),
                palette,
            ));
    }
    block.into()
}
