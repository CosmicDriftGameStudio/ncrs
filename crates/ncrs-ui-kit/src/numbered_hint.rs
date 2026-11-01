//! The lines under a dialog that renames files that share a name.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use iced::widget::text::Wrapping;
use iced::widget::{row, text};
use iced::{alignment, Element, Font};
use ncrs_i18n::{fill_template, Language, Msg};

use crate::format;
use crate::kit;
use crate::palette::Palette;
use crate::theme::{font_size, spacing};

/// The lines under a dialog that renames files with equal names: the sentence,
/// then every renamed file with the folder it comes from, at most
/// `HINT_LINES` of them. Nothing when no name is shared.
pub fn numbered_hint<'a, M: Clone + 'a>(
    sentence: &str,
    members: &[(PathBuf, OsString)],
    scope: Option<&Path>,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, M>> {
    if members.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<Element<'a, M>> = vec![kit::dim(
        fill_template(sentence, &[("{count}", &members.len().to_string())]),
        palette,
    )];
    for (source, name) in members.iter().take(HINT_LINES) {
        let folder = match scope {
            Some(root) => format::folder_below(source, root),
            None => source
                .parent()
                .map_or_else(String::new, |parent| parent.to_string_lossy().into_owned()),
        };
        lines.push(
            row![
                text(name.to_string_lossy().into_owned())
                    .size(font_size::FILE)
                    .font(Font::MONOSPACE)
                    .wrapping(Wrapping::None),
                text(fill_template(
                    lang.text(Msg::HintNumberedFrom),
                    &[("{folder}", &folder)],
                ))
                .size(font_size::SMALL)
                .color(palette.dim_text)
                .wrapping(Wrapping::None),
            ]
            .spacing(spacing::LINE_GAP)
            .align_y(alignment::Vertical::Center)
            .into(),
        );
    }
    if members.len() > HINT_LINES {
        lines.push(kit::dim(
            fill_template(
                lang.text(Msg::TransferListMore),
                &[("{count}", &(members.len() - HINT_LINES).to_string())],
            ),
            palette,
        ));
    }
    lines
}

/// Renamed files a dialog lists before it says how many more there are.
const HINT_LINES: usize = 6;
