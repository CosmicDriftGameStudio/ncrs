//! Bottom bar: active path, selected entry info, or the last error.
use iced::widget::{button, column, container, row, text, text_input, tooltip, Stack};
use iced::{alignment, Element, Font, Length, Padding};

use super::format;
use super::layout::STATUSBAR_HEIGHT;
use super::panel::PanelState;
use super::theme::{self, font_size, spacing};
use crate::fs::ReadError;
use crate::i18n::{Language, Msg};
use crate::messages::PanelSide;
use crate::palette::Palette;

/// The path at the left of the bar: a text field, with a button that copies
/// the panel's path. The messages are the caller's, so
/// this stays a view.
pub struct PathBar<'a, M> {
    /// The text being edited; `None` shows the panel's own path.
    pub editing: Option<&'a str>,
    pub on_input: fn(String) -> M,
    pub on_submit: M,
    pub on_copy: M,
}

/// Room left and right of each field of the bar.
const FIELD_PADDING_X: f32 = 10.0;
/// Width of a character of the monospace font, in font sizes.
const GLYPH_WIDTH_PER_SIZE: f32 = 0.6;
/// The path field is as wide as its text, so the entry info stands right after
/// it; room for the copy button and for typing a longer path.
const PATH_EXTRA_WIDTH: f32 = 36.0;
const PATH_MIN_WIDTH: f32 = 160.0;
const PATH_MAX_WIDTH: f32 = 520.0;
const ENTRY_SEPARATOR: &str = " · ";
const COPY_ICON_SIZE: f32 = 14.0;
const COPY_ICON_SQUARE: f32 = 9.0;

/// Two overlapping squares, drawn from widgets so no font or image is needed.
fn copy_icon<'a, M: 'a>(palette: &Palette) -> Element<'a, M> {
    let square = |filled: bool, offset: f32| {
        container(
            container(column![])
                .width(COPY_ICON_SQUARE)
                .height(COPY_ICON_SQUARE)
                .style(theme::icon_square(palette, filled)),
        )
        .padding(Padding {
            top: offset,
            left: offset,
            ..Padding::ZERO
        })
    };
    Stack::with_children([
        square(false, COPY_ICON_SIZE - COPY_ICON_SQUARE).into(),
        square(true, 0.0).into(),
    ])
    .width(COPY_ICON_SIZE)
    .height(COPY_ICON_SIZE)
    .into()
}

fn path_bar<'a, M: Clone + 'a>(
    panel: &'a PanelState,
    lang: Language,
    palette: &Palette,
    bar: PathBar<'a, M>,
) -> Element<'a, M> {
    // Always a text field: a click puts the caret where it landed, and what it
    // shows is the panel's path until the user types something else.
    let shown = bar
        .editing
        .map_or_else(|| panel.path.display().to_string(), str::to_owned);
    let path: Element<'a, M> = text_input("", &shown)
        .on_input(bar.on_input)
        .on_submit(bar.on_submit)
        .size(font_size::FILE)
        .font(Font::MONOSPACE)
        .padding(0.0)
        .style(theme::path_input(palette))
        .into();
    let copy = tooltip(
        button(copy_icon(palette))
            .on_press(bar.on_copy)
            .padding(3.0)
            .style(theme::icon_button(palette)),
        container(
            text(lang.text(Msg::StatusCopyPathTooltip))
                .size(font_size::SMALL)
                .wrapping(text::Wrapping::None),
        )
        .padding([2.0, 6.0])
        .style(theme::dialog(palette)),
        tooltip::Position::Top,
    );
    row![container(path).width(Length::Fill).clip(true), copy]
        .spacing(spacing::CELL_PADDING_X / 2.0)
        .align_y(alignment::Vertical::Center)
        .into()
}

/// What a running job shows in the bar. Built by `App`, which owns the queue.
pub struct JobStatus {
    /// The name of what is running, already translated.
    pub label: &'static str,
    pub done: usize,
    pub total: usize,
    /// How many jobs are waiting behind this one.
    pub waiting: usize,
    /// Whether the running job can be stopped. False while a prompt is open,
    /// where Escape means something else.
    pub abortable: bool,
}

/// The filing progress at the right end of the bar; pressing it opens the
/// filing status page. The text is the caller's, already translated.
pub struct FilingBadge<M> {
    pub text: String,
    /// The text is in the error colour.
    pub failing: bool,
    pub on_press: M,
}

/// What goes on in the background, at the right of the bar.
pub struct Activity<'a, M> {
    /// The panel the bar speaks for: named in the mode field at its left end.
    pub side: PanelSide,
    /// The name of the colour theme, shown at the right end. `None` for a
    /// palette that is no preset.
    pub theme_name: Option<String>,
    pub job: Option<JobStatus>,
    /// What a press on the job text sends: the progress mask back. `None`
    /// where the job has no mask.
    pub job_press: Option<M>,
    pub filing: Option<FilingBadge<M>>,
    /// Makes the "access blocked" error a button. `None` where there is no
    /// settings page to open.
    pub on_blocked_press: Option<M>,
    /// A confirmation that goes away by itself. It takes the right end of the
    /// bar from the filing progress and stands beside an error.
    pub notice: Option<&'a str>,
    /// What Hermes says about the file under the cursor: its state, why, and
    /// where it would go. Shown in place of the entry info.
    pub hermes_line: Option<String>,
}

/// The bottom bar: path, then the running job, then the failure of the last
/// job, the panel's read error, or the selected entry; at the right the
/// notice or the filing progress.
///
/// A running job takes the middle: the file under the cursor is not what the
/// user is looking at while fifty thousand files are on their way.
pub fn view<'a, M: Clone + 'a>(
    panel: &'a PanelState,
    lang: Language,
    palette: &Palette,
    activity: Activity<'a, M>,
    failure: Option<&'a str>,
    search_file: Option<String>,
    path_bar_input: PathBar<'a, M>,
) -> Element<'a, M> {
    let Activity {
        side,
        theme_name,
        job,
        job_press,
        filing,
        on_blocked_press,
        notice,
        hermes_line,
    } = activity;
    let path_chars = path_bar_input.editing.map_or_else(
        || panel.path.display().to_string().chars().count(),
        |typed| typed.chars().count(),
    );
    let path_width = (path_chars as f32 * font_size::FILE * GLYPH_WIDTH_PER_SIZE
        + PATH_EXTRA_WIDTH)
        .clamp(PATH_MIN_WIDTH, PATH_MAX_WIDTH);
    let path = path_bar(panel, lang, palette, path_bar_input);

    let details: Element<'a, M> = if let Some(last_failure) = failure {
        text(last_failure)
            .size(font_size::SMALL)
            .color(palette.error)
            .wrapping(text::Wrapping::None)
            .into()
    } else if let Some(error) = &panel.error {
        let line = text(format_error(error, &panel.path, lang))
            .size(font_size::SMALL)
            .color(palette.error)
            .wrapping(text::Wrapping::None);
        match (error, on_blocked_press) {
            (ReadError::AccessBlocked { .. }, Some(open_settings)) => button(line)
                .padding(0)
                .style(theme::icon_button(palette))
                .on_press(open_settings)
                .into(),
            _ => line.into(),
        }
    } else if let Some(file) = search_file {
        text(file)
            .size(font_size::SMALL)
            .wrapping(text::Wrapping::None)
            .into()
    } else if let (Some(line), Some(entry)) = (hermes_line, panel.selected_entry()) {
        hermes_details(&entry.name.to_string_lossy(), line, palette)
    } else if let Some(entry) = panel.selected_entry() {
        let name = entry.name.to_string_lossy().into_owned();
        let dim = |content: String| {
            text(content)
                .size(font_size::SMALL)
                .color(palette.dim_text)
                .wrapping(text::Wrapping::None)
        };
        let mut info = row![
            dim(name).font(Font::MONOSPACE),
            dim(ENTRY_SEPARATOR.to_owned()),
            dim(format::entry_size(entry)),
        ];
        // The parent entry has no date; no dangling separator then.
        let modified = format::time(entry.modified);
        if !modified.is_empty() {
            info = info
                .push(dim(ENTRY_SEPARATOR.to_owned()))
                .push(dim(modified));
        }
        info.into()
    } else {
        text(if panel.loading {
            lang.text(Msg::StatusLoading)
        } else {
            ""
        })
        .size(font_size::SMALL)
        .color(palette.dim_text)
        .into()
    };

    /// The job line: what it is, how far along, and the abort hint. Returns
    /// None when nothing is running, so the bar falls back to the file info.
    fn job_text<'a, M: 'a>(job: &JobStatus, lang: Language, palette: &Palette) -> Element<'a, M> {
        let counter = lang
            .text(Msg::JobProgress)
            .replace("{done}", &job.done.to_string())
            .replace("{total}", &job.total.to_string());

        let mut line = format!("{}  {counter}", job.label);
        if job.waiting > 0 {
            let waiting = lang
                .text(Msg::JobQueued)
                .replace("{count}", &job.waiting.to_string());
            line.push_str("  •  ");
            line.push_str(&waiting);
        }
        if job.abortable {
            line.push_str("  •  ");
            line.push_str(lang.text(Msg::JobAbort));
        }
        text(line)
            .size(font_size::SMALL)
            .color(palette.accent)
            .wrapping(text::Wrapping::None)
            .into()
    }

    // A running job replaces the file info, not the path: the path is where the
    // operation is going, which is still what the user is watching.
    let middle: Element<'a, M> = match &job {
        Some(running) => match job_press {
            Some(press) => button(job_text(running, lang, palette))
                .padding(0)
                .style(theme::icon_button(palette))
                .on_press(press)
                .into(),
            None => job_text(running, lang, palette),
        },
        None => details,
    };
    let mode = lang.text(match side {
        PanelSide::Left => Msg::StatusModeLeft,
        PanelSide::Right => Msg::StatusModeRight,
    });
    let mode_field = container(
        text(mode)
            .size(font_size::SMALL)
            .wrapping(text::Wrapping::None),
    )
    .padding([0.0, FIELD_PADDING_X])
    .height(Length::Fill)
    .align_y(alignment::Vertical::Center)
    .style(theme::mode_field(palette));

    let mut bar = row![
        mode_field,
        container(path)
            .width(path_width)
            .padding([0.0, FIELD_PADDING_X])
            .clip(true),
        container(middle)
            .width(Length::Fill)
            .padding([0.0, FIELD_PADDING_X])
            .clip(true),
    ]
    .align_y(alignment::Vertical::Center);
    if let Some(shown) = notice {
        bar = bar.push(
            text(format!("\u{2713} {shown}"))
                .size(font_size::SMALL)
                .color(palette.success)
                .wrapping(text::Wrapping::None),
        );
    } else if let Some(badge) = filing {
        bar = bar.push(
            button(
                text(badge.text)
                    .size(font_size::SMALL)
                    .color(if badge.failing {
                        palette.error
                    } else {
                        palette.accent
                    })
                    .wrapping(text::Wrapping::None),
            )
            .on_press(badge.on_press)
            .padding([0.0, 3.0])
            .style(theme::icon_button(palette)),
        );
    }
    for dim in [Some(lang.code().to_uppercase()), theme_name]
        .into_iter()
        .flatten()
    {
        bar = bar.push(
            container(
                text(dim)
                    .size(font_size::SMALL)
                    .color(palette.dim_text)
                    .wrapping(text::Wrapping::None),
            )
            .padding([0.0, FIELD_PADDING_X]),
        );
    }

    container(column![
        theme::horizontal_rule(palette.border_inactive),
        container(bar).width(Length::Fill).height(Length::Fill),
    ])
    .height(STATUSBAR_HEIGHT)
    .width(Length::Fill)
    .style(theme::statusbar(palette))
    .into()
}

/// The cursor file's name, then what Hermes says about it.
fn hermes_details<'a, M: 'a>(name: &str, line: String, palette: &Palette) -> Element<'a, M> {
    let small = |content: String| {
        text(content)
            .size(font_size::SMALL)
            .wrapping(text::Wrapping::None)
    };
    row![
        small(name.to_owned())
            .font(Font::MONOSPACE)
            .color(palette.dim_text),
        small(ENTRY_SEPARATOR.to_owned()).color(palette.dim_text),
        small(line),
    ]
    .into()
}

/// Renders a read error in the current language. The path comes from the panel
/// that failed, the reason from the OS; both templates name their placeholders
/// identically in every language, so word order is free to differ.
fn format_error(error: &ReadError, path: &std::path::Path, lang: Language) -> String {
    let msg = match error {
        ReadError::CannotRead { .. } => Msg::ErrorCannotRead,
        ReadError::AccessBlocked { .. } => Msg::ErrorAccessBlocked,
        ReadError::TaskFailed { .. } => Msg::ErrorTaskFailed,
        ReadError::EntriesLeftOut { .. } => Msg::ErrorArchiveEntriesLeftOut,
        ReadError::ArchiveEncrypted => Msg::ErrorArchiveEncrypted,
        ReadError::ArchiveDictionaryTooLarge => Msg::ErrorArchiveDictionaryTooLarge,
        ReadError::RemoteDisconnected { .. } => Msg::ErrorRemoteDisconnected,
        ReadError::RemoteTimeout { .. } => Msg::ErrorRemoteTimeout,
        ReadError::Remote { .. } => Msg::ErrorRemoteFailed,
    };
    let reason = match error {
        ReadError::CannotRead { reason }
        | ReadError::AccessBlocked { reason }
        | ReadError::TaskFailed { reason }
        | ReadError::Remote { reason } => reason.clone(),
        ReadError::EntriesLeftOut { count } => count.to_string(),
        ReadError::RemoteDisconnected { server } => server.to_string(),
        ReadError::RemoteTimeout { host } => host.clone(),
        ReadError::ArchiveEncrypted | ReadError::ArchiveDictionaryTooLarge => String::new(),
    };
    lang.text(msg)
        .replace("{path}", &path.display().to_string())
        .replace("{reason}", &reason)
        .replace("{error}", &reason)
        .replace("{count}", &reason)
        .replace("{server}", &reason)
        .replace("{host}", &reason)
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn cannot_read() -> ReadError {
        ReadError::CannotRead {
            reason: "Permission denied".to_string(),
        }
    }

    /// German puts the path first, English leads with the verb. Both must keep
    /// the path and the OS reason intact.
    #[test]
    fn error_is_rendered_per_language() {
        let path = Path::new("/etc/shadow");

        assert_eq!(
            format_error(&cannot_read(), path, Language::English),
            "Cannot read /etc/shadow: Permission denied"
        );
        assert_eq!(
            format_error(&cannot_read(), path, Language::German),
            "/etc/shadow nicht lesbar: Permission denied"
        );
    }

    #[test]
    fn blocked_access_names_the_settings_without_a_path() {
        let err = ReadError::AccessBlocked {
            reason: "Operation not permitted".to_string(),
        };
        let path = Path::new("/Volumes/Vault");
        assert_eq!(
            format_error(&err, path, Language::English),
            "macOS blocks access. Give ncrs, or the terminal that started it, Full Disk Access (click to open)"
        );
        assert_eq!(
            format_error(&err, path, Language::German),
            "macOS sperrt den Zugriff. ncrs oder dem Terminal, aus dem es gestartet wurde, Festplattenvollzugriff geben (Klick öffnet)"
        );
    }

    #[test]
    fn task_failure_keeps_the_internal_reason() {
        let err = ReadError::TaskFailed {
            reason: "cancelled".to_string(),
        };
        assert_eq!(
            format_error(&err, Path::new("/tmp"), Language::English),
            "Directory read task failed: cancelled"
        );
    }

    #[test]
    fn an_encrypted_archive_says_so_in_both_languages() {
        let err = ReadError::ArchiveEncrypted;
        assert_eq!(
            format_error(&err, Path::new("/tmp/a.7z"), Language::English),
            "This archive is encrypted; ncrs cannot open it yet"
        );
        assert_eq!(
            format_error(&err, Path::new("/tmp/a.7z"), Language::German),
            "Dieses Archiv ist verschlüsselt; ncrs kann es noch nicht öffnen"
        );
    }

    #[test]
    fn an_archive_with_a_huge_dictionary_says_so_in_both_languages() {
        let err = ReadError::ArchiveDictionaryTooLarge;
        assert_eq!(
            format_error(&err, Path::new("/tmp/a.7z"), Language::English),
            "This 7z archive needs more than 256 MB of memory to unpack; ncrs does not open it"
        );
        assert_eq!(
            format_error(&err, Path::new("/tmp/a.7z"), Language::German),
            "Dieses 7z-Archiv braucht zum Entpacken mehr als 256 MB Speicher; ncrs öffnet es nicht"
        );
    }

    #[test]
    fn left_out_archive_entries_are_counted_in_the_message() {
        let err = ReadError::EntriesLeftOut { count: 3 };
        assert_eq!(
            format_error(&err, Path::new("/tmp/a.zip"), Language::English),
            "3 entries left out: unsafe paths, links or special files"
        );
        assert_eq!(
            format_error(&err, Path::new("/tmp/a.zip"), Language::German),
            "3 Einträge ausgelassen: unsichere Pfade, Links oder Sonderdateien"
        );
    }
}
