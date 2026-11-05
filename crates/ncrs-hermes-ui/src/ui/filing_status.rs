//! The read errors page of the settings: the files Hermes could not read,
//! newest first, each with its reason and how long ago it happened. Pure view
//! over [`FilingStatus`].

use std::path::Path;
use std::time::SystemTime;

use iced::Element;
use ncrs_config::palette::Palette;
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::format;
use ncrs_ui_kit::kit::{self, Tone};
use ncrs_ui_kit::setting_rows::{Group, SettingRow};

use crate::filing_status::FilingStatus;
use crate::message::{FilingStatusMessage, Message};

/// From this many rows on, a button reads all of them again at once.
const MIN_ROWS_FOR_ALL_AT_ONCE: usize = 2;
const SECONDS_PER_HOUR: u64 = 3600;

fn send(message: FilingStatusMessage) -> Message {
    Message::FilingStatus(message)
}

/// The sections of the page: one row per file that could not be read, or one
/// row that says there are none.
pub fn groups<'a>(
    status: &FilingStatus,
    lang: Language,
    palette: &Palette,
) -> Vec<Group<'a, Message>> {
    let mut rows: Vec<SettingRow<'a, Message>> = status
        .errors_newest_first()
        .map(|error| {
            SettingRow::listed(
                file_label(&error.file),
                Some(error.message.clone()),
                Some(age_label(status, error.at)),
                vec![
                    kit::button(
                        lang.text(Msg::FilingStatusShowFile),
                        Some(send(FilingStatusMessage::ErrorActivated(
                            error.file.clone(),
                        ))),
                        Tone::Secondary,
                        palette,
                    ),
                    kit::button(
                        lang.text(Msg::FilingStatusProcessAgain),
                        Some(send(FilingStatusMessage::ProcessOneAgain(
                            error.file.clone(),
                        ))),
                        Tone::Secondary,
                        palette,
                    ),
                ],
                palette,
            )
        })
        .collect();
    if rows.is_empty() {
        rows.push(SettingRow::adding(
            Some(lang.text(Msg::FilingStatusErrorsNone).to_owned()),
            None,
            Vec::new(),
        ));
    } else if rows.len() >= MIN_ROWS_FOR_ALL_AT_ONCE {
        rows.push(SettingRow::adding(
            None,
            None,
            vec![kit::button(
                lang.text(Msg::FilingStatusProcessAllAgain),
                Some(send(FilingStatusMessage::ProcessAllAgain)),
                Tone::Secondary,
                palette,
            )],
        ));
    }
    vec![Group {
        intro: Some(lang.text(Msg::FilingStatusExplain).to_owned()),
        title: String::new(),
        rows,
    }]
}

/// The notice that the PDF library is missing, with the place it was
/// searched in.
pub fn pdf_missing_notice<'a>(
    searched: &Path,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    kit::warning_box(
        format!(
            "{}\n{}",
            lang.text(Msg::FilingStatusPdfMissing),
            ncrs_i18n::fill_template(
                lang.text(Msg::FilingStatusPdfSearched),
                &[(
                    "{path}",
                    &format::ellipsize_middle(&searched.display().to_string(), 60),
                )],
            ),
        ),
        palette,
    )
}

fn file_label(file: &Path) -> String {
    file.file_name().map_or_else(
        || file.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// How long ago an error happened: `m:ss`, and `h:mm:ss` from one hour on.
fn age_label(status: &FilingStatus, at: SystemTime) -> String {
    let total = status
        .taken_at()
        .duration_since(at)
        .unwrap_or_default()
        .as_secs();
    let hours = total.div_euclid(SECONDS_PER_HOUR);
    let minutes = total.rem_euclid(SECONDS_PER_HOUR).div_euclid(60);
    let seconds = total.rem_euclid(60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}
