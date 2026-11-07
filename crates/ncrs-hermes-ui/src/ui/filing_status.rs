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
use ncrs_ui_kit::setting_rows::{Group, RowButton, SettingRow};

use crate::filing_status::FilingStatus;
use crate::message::{FilingStatusMessage, Message};

/// From this many rows on, a button reads all of them again at once.
const MIN_ROWS_FOR_ALL_AT_ONCE: usize = 2;
const SECONDS_PER_MINUTE: u64 = 60;
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
            SettingRow::listed_buttons(
                file_label(&error.file),
                Some(reason_of(&error.file, &error.message)),
                Some(age_label(status, error.at, lang)),
                vec![
                    RowButton::new(
                        lang.text(Msg::FilingStatusShowFile),
                        Some(send(FilingStatusMessage::ErrorActivated(
                            error.file.clone(),
                        ))),
                        Tone::Secondary,
                    ),
                    RowButton::new(
                        lang.text(Msg::FilingStatusProcessAgain),
                        Some(send(FilingStatusMessage::ProcessOneAgain(
                            error.file.clone(),
                        ))),
                        Tone::Secondary,
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
        rows.push(SettingRow::adding_buttons(
            None,
            None,
            vec![RowButton::new(
                lang.text(Msg::FilingStatusProcessAllAgain),
                Some(send(FilingStatusMessage::ProcessAllAgain)),
                Tone::Secondary,
            )],
            palette,
        ));
    }
    vec![Group {
        intro: None,
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

/// The reason of an error without the path in front of it: the file name is
/// the title of the row already.
fn reason_of(file: &Path, message: &str) -> String {
    message
        .strip_prefix(&format!("{}: ", file.display()))
        .unwrap_or(message)
        .to_owned()
}

/// How long ago an error happened, in whole minutes or hours.
fn age_label(status: &FilingStatus, at: SystemTime, lang: Language) -> String {
    let total = status
        .taken_at()
        .duration_since(at)
        .unwrap_or_default()
        .as_secs();
    let (template, count) = if total >= SECONDS_PER_HOUR {
        (
            Msg::FilingStatusAgeHours,
            total.div_euclid(SECONDS_PER_HOUR),
        )
    } else if total >= SECONDS_PER_MINUTE {
        (
            Msg::FilingStatusAgeMinutes,
            total.div_euclid(SECONDS_PER_MINUTE),
        )
    } else {
        return lang.text(Msg::FilingStatusAgeNow).to_owned();
    };
    ncrs_i18n::fill_template(lang.text(template), &[("{count}", &count.to_string())])
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::reason_of;

    #[test]
    fn the_reason_leaves_out_the_path_in_front_of_it() {
        let file = Path::new("/archive/scan.pdf");
        assert_eq!(reason_of(file, "/archive/scan.pdf: not a PDF"), "not a PDF");
        assert_eq!(
            reason_of(file, "no text could be read"),
            "no text could be read"
        );
    }
}
