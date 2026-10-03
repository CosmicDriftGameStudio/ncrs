//! Bottom bar: active path, selected entry info, or the last error.
use iced::widget::{button, column, container, mouse_area, row, text, text_input, tooltip, Stack};
use iced::{alignment, mouse, Element, Length, Padding};

use super::format;
use super::layout::{DATE_COLUMN_WIDTH, SIZE_COLUMN_WIDTH, STATUSBAR_HEIGHT};
use super::panel::PanelState;
use super::theme::{self, colors, font_size, spacing};
use crate::fs::ReadError;
use crate::i18n::{Language, Msg};

/// The text field the path turns into when it is clicked.
pub const PATH_FIELD_ID: &str = "path-field";

/// The path at the left of the bar: plain text that becomes a text field when
/// clicked, with a button that copies it. The messages are the caller's, so
/// this stays a view.
pub struct PathBar<'a, M> {
    /// The text being edited; `None` shows the panel's own path.
    pub editing: Option<&'a str>,
    pub on_open: M,
    pub on_input: fn(String) -> M,
    pub on_submit: M,
    pub on_copy: M,
}

const COPY_ICON_SIZE: f32 = 14.0;
const COPY_ICON_SQUARE: f32 = 9.0;

/// Two overlapping squares, drawn from widgets so no font or image is needed.
fn copy_icon<'a, M: 'a>() -> Element<'a, M> {
    let square = |filled: bool, offset: f32| {
        container(
            container(column![])
                .width(COPY_ICON_SQUARE)
                .height(COPY_ICON_SQUARE)
                .style(theme::icon_square(filled)),
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
    bar: PathBar<'a, M>,
) -> Element<'a, M> {
    let path: Element<'a, M> = match bar.editing {
        Some(typed) => text_input("", typed)
            .id(PATH_FIELD_ID)
            .on_input(bar.on_input)
            .on_submit(bar.on_submit)
            .size(font_size::STATUS)
            .padding([0.0, 4.0])
            .style(theme::path_input)
            .into(),
        // The whole line is the click target, not just the glyphs: a short
        // path would leave most of the cell dead.
        None => mouse_area(
            container(
                text(panel.path.display().to_string())
                    .size(font_size::STATUS)
                    .color(colors::ACCENT)
                    .wrapping(text::Wrapping::None),
            )
            .width(Length::Fill),
        )
        .on_press(bar.on_open)
        .interaction(mouse::Interaction::Text)
        .into(),
    };
    let copy = tooltip(
        button(copy_icon())
            .on_press(bar.on_copy)
            .padding(3.0)
            .style(theme::icon_button),
        container(
            text(lang.text(Msg::StatusCopyPathTooltip))
                .size(font_size::STATUS)
                .wrapping(text::Wrapping::None),
        )
        .padding([2.0, 6.0])
        .style(theme::dialog),
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

/// The bottom bar: path, then the running job, then the failure of the last
/// job, the panel's read error, or the selected entry.
///
/// A running job takes the middle: the file under the cursor is not what the
/// user is looking at while fifty thousand files are on their way.
pub fn view<'a, M: Clone + 'a>(
    panel: &'a PanelState,
    lang: Language,
    job: Option<JobStatus>,
    failure: Option<&'a str>,
    path_bar_input: PathBar<'a, M>,
) -> Element<'a, M> {
    let path = path_bar(panel, lang, path_bar_input);

    let details: Element<'a, M> = if let Some(last_failure) = failure {
        text(last_failure)
            .size(font_size::STATUS)
            .color(colors::ERROR)
            .wrapping(text::Wrapping::None)
            .into()
    } else if let Some(error) = &panel.error {
        text(format_error(error, &panel.path, lang))
            .size(font_size::STATUS)
            .color(colors::ERROR)
            .wrapping(text::Wrapping::None)
            .into()
    } else if let Some(entry) = panel.selected_entry() {
        let name = entry.name.to_string_lossy().into_owned();
        row![
            container(
                text(name)
                    .size(font_size::STATUS)
                    .wrapping(text::Wrapping::None)
            )
            .width(Length::Fill)
            .clip(true),
            text(format::entry_size(entry))
                .size(font_size::STATUS)
                .width(SIZE_COLUMN_WIDTH)
                .align_x(alignment::Horizontal::Right),
            text(format::time(entry.modified))
                .size(font_size::STATUS)
                .width(DATE_COLUMN_WIDTH)
                .align_x(alignment::Horizontal::Right),
        ]
        .into()
    } else {
        text(if panel.loading {
            lang.text(Msg::StatusLoading)
        } else {
            ""
        })
        .size(font_size::STATUS)
        .color(colors::DIM_TEXT)
        .into()
    };

    let separator = || text("│").size(font_size::STATUS).color(colors::DIM_TEXT);
    // An invisible separator keeps the columns aligned whether or not a job is
    // running, so the file info does not jump sideways when one starts.
    let separator_hidden = || text(" ").size(font_size::STATUS);

    /// The job line: what it is, how far along, and the abort hint. Returns
    /// None when nothing is running, so the bar falls back to the file info.
    fn job_text<'a, M: 'a>(job: &JobStatus, lang: Language) -> Element<'a, M> {
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
            .size(font_size::STATUS)
            .color(colors::ACCENT)
            .wrapping(text::Wrapping::None)
            .into()
    }

    // A running job replaces the file info, not the path: the path is where the
    // operation is going, which is still what the user is watching.
    let middle: Element<'a, M> = match &job {
        Some(running) => job_text(running, lang),
        None => details,
    };
    // The path keeps two parts, the middle three, whatever is in the middle.
    const PATH_PARTS: u16 = 2;
    const MIDDLE_PARTS: u16 = 3;

    container(
        row![
            container(path)
                .width(Length::FillPortion(PATH_PARTS))
                .clip(true),
            separator(),
            match &job {
                Some(_) => separator(),
                None => separator_hidden(),
            },
            container(middle)
                .width(Length::FillPortion(MIDDLE_PARTS))
                .clip(true),
        ]
        .spacing(spacing::CELL_PADDING_X)
        .align_y(alignment::Vertical::Center),
    )
    .padding([0.0, spacing::CELL_PADDING_X * 1.5])
    .height(STATUSBAR_HEIGHT)
    .width(Length::Fill)
    .align_y(alignment::Vertical::Center)
    .style(theme::statusbar)
    .into()
}

/// Renders a read error in the current language. The path comes from the panel
/// that failed, the reason from the OS; both templates name their placeholders
/// identically in every language, so word order is free to differ.
fn format_error(error: &ReadError, path: &std::path::Path, lang: Language) -> String {
    let msg = match error {
        ReadError::CannotRead { .. } => Msg::ErrorCannotRead,
        ReadError::TaskFailed { .. } => Msg::ErrorTaskFailed,
    };
    let reason = match error {
        ReadError::CannotRead { reason } | ReadError::TaskFailed { reason } => reason.as_str(),
    };
    lang.text(msg)
        .replace("{path}", &path.display().to_string())
        .replace("{reason}", reason)
        .replace("{error}", reason)
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
    fn task_failure_keeps_the_internal_reason() {
        let err = ReadError::TaskFailed {
            reason: "cancelled".to_string(),
        };
        assert_eq!(
            format_error(&err, Path::new("/tmp"), Language::English),
            "Directory read task failed: cancelled"
        );
    }
}
