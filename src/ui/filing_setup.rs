//! The filing setup card. Not modal, like the filing cards: the user walks the
//! panels to the next folder while it is up.

use std::path::Path;

use iced::widget::text::Wrapping;
use iced::widget::{button, checkbox, column, container, text, Column, Row};
use iced::{Alignment, Element, Length};

use super::filing::{buttons, error_line, paragraph, title};
use super::theme::{self, font_size, spacing};
use crate::app::fill_template;
use crate::config::{ConfigError, FilingLocation, FilingRuleViolation, FilingWriteError};
use crate::filing_setup::{shown_path, FilingSetup, LocationKind, SetupNote};
use crate::i18n::{Language, Msg};
use crate::messages::{FilingSetupMessage, Message};
use crate::palette::Palette;

/// Wider than the filing cards: a row holds a path next to its controls.
const CARD_WIDTH: f32 = 640.0;
const KIND_WIDTH: f32 = 90.0;

/// What the card shows besides the setup's own state.
pub struct SetupContext<'a> {
    /// The folder the add buttons add: the active panel's.
    pub current_folder: &'a Path,
    /// Paths below it are shown with `~`.
    pub home: &'a Path,
    /// The open index is unencrypted, so the vault warning applies.
    pub index_is_plain: bool,
    pub lang: Language,
}

pub fn view<'a>(
    setup: &'a FilingSetup,
    context: &SetupContext<'_>,
    palette: &Palette,
) -> Element<'a, Message> {
    let lang = context.lang;
    let busy = setup.is_busy();
    let add = |label: Msg, kind: LocationKind| {
        action(
            lang.text(label),
            (!busy).then_some(FilingSetupMessage::Add(kind)),
            false,
            palette,
        )
    };
    let mut body = column![
        title(lang.text(Msg::FilingSetupTitle), palette),
        paragraph(lang.text(Msg::FilingSetupExplain), palette),
        paragraph(
            fill_template(
                lang.text(Msg::FilingSetupCurrentFolder),
                &[("{path}", &shown_path(context.current_folder, context.home))],
            ),
            palette,
        ),
        buttons(vec![
            add(Msg::FilingSetupAddInbox, LocationKind::Inbox),
            add(Msg::FilingSetupAddArchive, LocationKind::Archive),
            add(Msg::FilingSetupAddTarget, LocationKind::Target),
        ]),
    ]
    .spacing(spacing::SECTION_GAP);

    body = body.push(if setup.chosen().is_empty() {
        paragraph(lang.text(Msg::FilingSetupEmpty), palette)
    } else {
        locations(setup.chosen(), busy, context, palette)
    });
    if let Some(violation) = setup.violation() {
        body = body.push(error_line(
            rule_text(violation, setup.chosen(), context),
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
    body = body.push(buttons(vec![
        action(
            lang.text(Msg::FilingSetupSave),
            setup.can_save().then_some(FilingSetupMessage::Save),
            true,
            palette,
        ),
        action(
            lang.text(Msg::FilingSetupClose),
            Some(FilingSetupMessage::Close),
            false,
            palette,
        ),
    ]));
    container(body)
        .padding(spacing::OUTER_PADDING * 2.0)
        .width(CARD_WIDTH)
        .style(theme::dialog(palette))
        .into()
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
    button(text(label))
        .on_press_maybe(on_press.map(setup_message))
        .style(theme::dialog_button(palette, primary))
        .into()
}

fn kind_label(kind: LocationKind) -> Msg {
    match kind {
        LocationKind::Inbox => Msg::FilingSetupKindInbox,
        LocationKind::Archive => Msg::FilingSetupKindArchive,
        LocationKind::Target => Msg::FilingSetupKindTarget,
    }
}

fn locations<'a>(
    chosen: &[FilingLocation],
    busy: bool,
    context: &SetupContext<'_>,
    palette: &Palette,
) -> Element<'a, Message> {
    let lang = context.lang;
    let rows = chosen.iter().enumerate().map(|(index, location)| {
        Row::with_children([
            text(lang.text(kind_label(LocationKind::of(location))))
                .size(font_size::STATUS)
                .color(palette.accent)
                .width(KIND_WIDTH)
                .into(),
            text(shown_path(&location.path, context.home))
                .size(font_size::STATUS)
                .color(palette.text)
                .wrapping(Wrapping::Glyph)
                .width(Length::Fill)
                .into(),
            checkbox(location.images)
                .on_toggle_maybe(
                    (!busy)
                        .then_some(move |_| setup_message(FilingSetupMessage::ToggleImages(index))),
                )
                .style(theme::dialog_checkbox(palette))
                .into(),
            text(lang.text(Msg::FilingSetupImages))
                .size(font_size::STATUS)
                .color(palette.text)
                .into(),
            action(
                lang.text(Msg::FilingSetupRemove),
                (!busy).then_some(FilingSetupMessage::Remove(index)),
                false,
                palette,
            ),
        ])
        .spacing(spacing::CELL_PADDING_X)
        .align_y(Alignment::Center)
        .into()
    });
    Column::with_children(rows)
        .spacing(spacing::CELL_PADDING_X)
        .into()
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
        SetupNote::Applied => (lang.text(Msg::FilingSetupApplied).to_owned(), false),
        SetupNote::AppliesAtNextStart => (lang.text(Msg::FilingSetupNextStart).to_owned(), false),
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
