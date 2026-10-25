//! The filing setup card. It sits on the scrim like the filing cards, but the
//! user still walks the panels to the next folder by key while it is up.

use std::path::Path;

use iced::widget::text::Wrapping;
use iced::widget::{checkbox, container, radio, text, Column, Row, Space};
use iced::{Alignment, Element, Length, Size};
use ncrs_intake::LocationMode;

use super::filing::radio_look;
use super::format;
use super::frame::{self, DialogSize, Frame, KeyHints};
use super::kit::{self, dim, error_line, paragraph, Tone};
use super::theme::{self, font_size, spacing};
use crate::app::fill_template;
use crate::config::{ConfigError, FilingLocation, FilingRuleViolation, FilingWriteError};
use crate::filing_setup::{shown_path, FilingSetup, SetupNote, SetupProblem};
use crate::i18n::{Language, Msg};
use crate::messages::{FilingSetupMessage, Message};
use crate::palette::Palette;

const SIZE: DialogSize = DialogSize::Medium;
const KIND_WIDTH: f32 = 130.0;

/// What a row keeps for its other controls: the checkbox, its label and the
/// remove button, and for a document folder the kind column besides.
const INBOX_ROW_RESERVED: f32 = 220.0;
const ARCHIVE_ROW_RESERVED: f32 = INBOX_ROW_RESERVED + KIND_WIDTH + 16.0;
/// What the line under the add buttons keeps for the two buttons.
const ADD_ROW_RESERVED: f32 = 320.0;

/// Wrap the two add buttons of each section, so a test can tell the sections
/// apart: both carry the same labels.
pub const ADD_TUBES_FOLDER_ID: &str = "filing-setup-add-tubes-folder";
pub const ADD_DOCUMENT_FOLDER_ID: &str = "filing-setup-add-document-folder";
pub const USE_PANEL_FOR_TUBES_ID: &str = "filing-setup-use-panel-for-tubes";
pub const USE_PANEL_FOR_DOCUMENTS_ID: &str = "filing-setup-use-panel-for-documents";

/// The id of the radio button that makes the document folder at `index` of the
/// chosen list the Central Filing. A radio button's label is not a text a test
/// can find, so it is addressed by this.
pub fn target_choice_id(index: usize) -> String {
    format!("filing-setup-target-{index}")
}

/// What the card shows besides the setup's own state.
pub struct SetupContext<'a> {
    /// The folder the "use the folder in the panel" buttons add: the active
    /// panel's.
    pub current_folder: &'a Path,
    /// Paths below it are shown with `~`.
    pub home: &'a Path,
    /// The open index is unencrypted, so the vault warning applies.
    pub index_is_plain: bool,
    pub lang: Language,
    pub window: Size,
}

pub fn view<'a>(
    setup: &'a FilingSetup,
    context: &SetupContext<'_>,
    palette: &Palette,
) -> Element<'a, Message> {
    let lang = context.lang;
    let busy = setup.is_busy();
    let tubes = kit::section(
        lang.text(Msg::FilingSetupTubesHeading),
        kit::stack([
            paragraph(lang.text(Msg::FilingSetupTubesExplain), palette),
            dim(lang.text(Msg::FilingSetupTubesExample), palette),
            section_rows(setup, LocationMode::Inbox, busy, context, palette),
            add_buttons(LocationMode::Inbox, busy, context, palette),
        ]),
        palette,
    );
    let documents = kit::section(
        lang.text(Msg::FilingSetupDocumentsHeading),
        kit::stack([
            paragraph(lang.text(Msg::FilingSetupDocumentsExplain), palette),
            dim(lang.text(Msg::FilingSetupDocumentsKinds), palette),
            section_rows(setup, LocationMode::Archive, busy, context, palette),
            add_buttons(LocationMode::Archive, busy, context, palette),
        ]),
        palette,
    );

    let mut body = kit::stack([
        paragraph(lang.text(Msg::FilingSetupExplain), palette),
        tubes,
        documents,
    ]);

    if let Some(problem) = setup.problem() {
        body = body.push(error_line(
            problem_text(problem, setup.chosen(), context),
            palette,
        ));
    }
    if context.index_is_plain {
        body = body.push(paragraph(
            lang.text(Msg::FilingEncryptionNoteVault),
            palette,
        ));
    }
    if let Some(note) = setup.note() {
        let (line, is_error) = note_text(note, setup.chosen(), context);
        body = body.push(if is_error {
            error_line(line, palette)
        } else {
            paragraph(line, palette)
        });
    }
    Frame::new(
        lang.text(Msg::FilingSetupTitle),
        body,
        SIZE,
        context.window,
        lang,
        palette,
    )
    .buttons(vec![
        action(
            lang.text(Msg::FilingSetupCancel),
            Some(FilingSetupMessage::Close),
            false,
            palette,
        ),
        action(
            lang.text(Msg::FilingSetupSave),
            setup.can_save().then_some(FilingSetupMessage::Save),
            false,
            palette,
        ),
    ])
    .on_close(setup_message(FilingSetupMessage::Close))
    // The panels keep Enter and Tab while the card is up.
    .hints(KeyHints::default())
    .into_element()
}

fn setup_message(message: FilingSetupMessage) -> Message {
    Message::FilingSetup(message)
}

fn action<'a>(
    label: &'a str,
    on_press: Option<FilingSetupMessage>,
    primary: bool,
    palette: &Palette,
) -> Element<'a, Message> {
    if primary {
        kit::enter_button(label, on_press.map(setup_message), Tone::Primary, palette)
    } else {
        kit::button(label, on_press.map(setup_message), Tone::Secondary, palette)
    }
}

fn add_buttons<'a>(
    mode: LocationMode,
    busy: bool,
    context: &SetupContext<'_>,
    palette: &Palette,
) -> Element<'a, Message> {
    let lang = context.lang;
    let (pick_id, panel_id) = match mode {
        LocationMode::Inbox => (ADD_TUBES_FOLDER_ID, USE_PANEL_FOR_TUBES_ID),
        LocationMode::Archive => (ADD_DOCUMENT_FOLDER_ID, USE_PANEL_FOR_DOCUMENTS_ID),
    };
    Row::with_children([
        container(action(
            lang.text(Msg::FilingSetupAddFolder),
            (!busy).then_some(FilingSetupMessage::PickFolder(mode)),
            false,
            palette,
        ))
        .id(pick_id)
        .into(),
        container(action(
            lang.text(Msg::FilingSetupUsePanelFolder),
            (!busy).then_some(FilingSetupMessage::AddPanelFolder(mode)),
            false,
            palette,
        ))
        .id(panel_id)
        .into(),
        text(format::ellipsize_middle(
            &shown_path(context.current_folder, context.home),
            frame::content_chars(SIZE, context.window, ADD_ROW_RESERVED),
        ))
        .size(font_size::BODY)
        .color(palette.dim_text)
        .wrapping(Wrapping::None)
        .width(Length::Fill)
        .into(),
    ])
    .spacing(spacing::CELL_PADDING_X)
    .align_y(Alignment::Center)
    .into()
}

/// The entries of one section. The indices they send point into the whole
/// chosen list, not into the section.
fn section_rows<'a>(
    setup: &'a FilingSetup,
    mode: LocationMode,
    busy: bool,
    context: &SetupContext<'_>,
    palette: &Palette,
) -> Element<'a, Message> {
    let lang = context.lang;
    let rows: Vec<Element<'a, Message>> = setup
        .chosen()
        .iter()
        .enumerate()
        .filter(|(_, location)| location.mode == mode)
        .map(|(index, location)| entry_row(setup, index, location, busy, context, palette))
        .collect();
    if rows.is_empty() {
        return dim(lang.text(Msg::FilingSetupEmptySection), palette);
    }
    Column::with_children(rows)
        .spacing(spacing::LINE_GAP)
        .into()
}

fn entry_row<'a>(
    setup: &FilingSetup,
    index: usize,
    location: &FilingLocation,
    busy: bool,
    context: &SetupContext<'_>,
    palette: &Palette,
) -> Element<'a, Message> {
    let lang = context.lang;
    let reserved = if location.mode == LocationMode::Archive {
        ARCHIVE_ROW_RESERVED
    } else {
        INBOX_ROW_RESERVED
    };
    let shown = format::ellipsize_middle(
        &shown_path(&location.path, context.home),
        frame::content_chars(SIZE, context.window, reserved),
    );
    let mut row = Row::new()
        .spacing(spacing::CELL_PADDING_X)
        .align_y(Alignment::Center);
    if location.mode == LocationMode::Archive {
        let is_central = setup.target_index() == Some(index);
        row = row.push(if setup.chooses_target() {
            container(
                radio(shown, index, setup.target_index(), |chosen| {
                    setup_message(FilingSetupMessage::SetTarget(chosen))
                })
                .size(font_size::BODY)
                .text_size(font_size::BODY)
                .style(radio_look(palette))
                .width(Length::Shrink),
            )
            .width(Length::Shrink)
            .id(target_choice_id(index))
            .into()
        } else {
            path_text(shown, palette)
        });
        row = row.push(dim(
            lang.text(if is_central {
                Msg::FilingSetupKindCentral
            } else {
                Msg::FilingSetupKindRecords
            }),
            palette,
        ));
    } else {
        row = row.push(path_text(shown, palette));
    }
    row.push(Space::new().width(Length::Fill))
        .push(
            checkbox(location.images)
                .on_toggle_maybe(
                    (!busy)
                        .then_some(move |_| setup_message(FilingSetupMessage::ToggleImages(index))),
                )
                .style(theme::dialog_checkbox(palette)),
        )
        .push(
            text(lang.text(Msg::FilingSetupImages))
                .size(font_size::BODY)
                .color(palette.text),
        )
        .push(action(
            lang.text(Msg::FilingSetupRemove),
            (!busy).then_some(FilingSetupMessage::Remove(index)),
            false,
            palette,
        ))
        .into()
}

fn path_text<'a>(shown: String, palette: &Palette) -> Element<'a, Message> {
    text(shown)
        .size(font_size::BODY)
        .color(palette.text)
        .wrapping(Wrapping::None)
        .into()
}

/// The sentence for a problem found outside the card, as it would read there.
pub fn problem_sentence(
    problem: SetupProblem,
    chosen: &[FilingLocation],
    lang: Language,
    home: &Path,
) -> String {
    let context = SetupContext {
        current_folder: home,
        home,
        index_is_plain: false,
        lang,
        window: Size::ZERO,
    };
    problem_text(problem, chosen, &context)
}

fn problem_text(
    problem: SetupProblem,
    chosen: &[FilingLocation],
    context: &SetupContext<'_>,
) -> String {
    match problem {
        SetupProblem::NoDocumentFolder => context
            .lang
            .text(Msg::FilingSetupProblemNoDocumentFolder)
            .to_owned(),
        SetupProblem::ChooseCentralFiling => context
            .lang
            .text(Msg::FilingSetupProblemChooseCentral)
            .to_owned(),
        SetupProblem::Rule(violation) => rule_text(violation, chosen, context),
    }
}

fn rule_text(
    violation: FilingRuleViolation,
    chosen: &[FilingLocation],
    context: &SetupContext<'_>,
) -> String {
    let lang = context.lang;
    let path_at = |index: usize| {
        chosen
            .get(index)
            .map(|location| shown_path(&location.path, context.home))
            .unwrap_or_default()
    };
    match violation {
        FilingRuleViolation::NoTarget => lang.text(Msg::FilingSetupRuleNoTarget).to_owned(),
        FilingRuleViolation::SecondTarget { .. } => {
            lang.text(Msg::FilingSetupRuleSecondTarget).to_owned()
        }
        FilingRuleViolation::InboxAsTarget { .. } => {
            lang.text(Msg::FilingSetupRuleInboxTarget).to_owned()
        }
        FilingRuleViolation::Overlap { earlier, later } => fill_template(
            lang.text(Msg::FilingSetupRuleOverlap),
            &[
                ("{first}", &path_at(earlier)),
                ("{second}", &path_at(later)),
            ],
        ),
    }
}

/// The line for `note`, and whether it reports a failure.
fn note_text(
    note: &SetupNote,
    chosen: &[FilingLocation],
    context: &SetupContext<'_>,
) -> (String, bool) {
    let lang = context.lang;
    let shown = |path: &Path| shown_path(path, context.home);
    match note {
        SetupNote::AlreadyAdded { folder } => (
            fill_template(
                lang.text(Msg::FilingSetupAlreadyAdded),
                &[("{path}", &shown(folder))],
            ),
            false,
        ),
        SetupNote::NoConfigPath => (lang.text(Msg::FilingSetupNoConfigPath).to_owned(), true),
        SetupNote::FolderUnusable { folder, reason } => (
            fill_template(
                lang.text(Msg::FilingSetupFolderUnusable),
                &[("{path}", &shown(folder)), ("{reason}", reason)],
            ),
            true,
        ),
        // Saving waits for a clean list, so this only happens when the
        // canonical paths reveal an overlap the shown ones hid.
        SetupNote::SaveFailed(FilingWriteError::Rule(violation)) => {
            (rule_text(*violation, chosen, context), true)
        }
        SetupNote::SaveFailed(FilingWriteError::PathNotText(path)) => (
            fill_template(
                lang.text(Msg::FilingSetupPathNotText),
                &[("{path}", &shown(path))],
            ),
            true,
        ),
        SetupNote::SaveFailed(FilingWriteError::Config(error)) => {
            (config_error_text(error, context), true)
        }
        SetupNote::SaveFailed(FilingWriteError::Io { path, reason }) => (
            fill_template(
                lang.text(Msg::FilingSetupWriteFailed),
                &[("{path}", &shown(path)), ("{reason}", reason)],
            ),
            true,
        ),
    }
}

fn config_error_text(error: &ConfigError, context: &SetupContext<'_>) -> String {
    let path = shown_path(error.path(), context.home);
    match error.line() {
        Some(line) => fill_template(
            context.lang.text(Msg::FilingSetupConfigBrokenAtLine),
            &[
                ("{path}", &path),
                ("{line}", &line.to_string()),
                ("{reason}", error.reason()),
            ],
        ),
        None => fill_template(
            context.lang.text(Msg::FilingSetupConfigBroken),
            &[("{path}", &path), ("{reason}", error.reason())],
        ),
    }
}
