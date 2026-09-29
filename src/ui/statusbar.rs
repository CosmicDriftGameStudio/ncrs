//! Bottom bar: active path, selected entry info, or the last error.

use iced::widget::{container, row, text};
use iced::{alignment, Element, Length};

use super::format;
use super::layout::{DATE_COLUMN_WIDTH, SIZE_COLUMN_WIDTH, STATUSBAR_HEIGHT};
use super::panel::PanelState;
use super::theme::{self, colors, font_size, spacing};
use crate::fs::ReadError;
use crate::i18n::{Language, Msg};

pub fn view<'a, M: 'a>(panel: &'a PanelState, lang: Language) -> Element<'a, M> {
    let path = text(panel.path.display().to_string())
        .size(font_size::STATUS)
        .color(colors::ACCENT)
        .wrapping(text::Wrapping::None);

    let details: Element<'a, M> = if let Some(error) = &panel.error {
        text(format_error(error, &panel.path, lang))
            .size(font_size::STATUS)
            .color(colors::ERROR)
            .wrapping(text::Wrapping::None)
            .into()
    } else if let Some(entry) = panel.selected_entry() {
        row![
            container(
                text(entry.name.as_str())
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
            lang.text(Msg::Loading)
        } else {
            ""
        })
        .size(font_size::STATUS)
        .color(colors::DIM_TEXT)
        .into()
    };

    let separator = text("│").size(font_size::STATUS).color(colors::DIM_TEXT);

    container(
        row![
            container(path).width(Length::FillPortion(2)).clip(true),
            separator,
            container(details).width(Length::FillPortion(3)).clip(true),
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
