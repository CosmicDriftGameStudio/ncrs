//! The drive menu for Alt+F1 and Alt+F2: one row per place, the highlighted one
//! is where Enter goes. Below the drives come the favourite folders.

use std::path::PathBuf;

use iced::widget::container;
use iced::widget::{button, column, text, Column, Row};
use iced::{Element, Length, Size};

use super::format;
use super::frame::{self, DialogSize, Frame, KeyHints, PageHint};
use super::kit;
use super::theme::{self, font_size, spacing};
use crate::fs::remote::ServerId;
use crate::fs::Volume;
use crate::i18n::{FuturamaName, Language, Msg};
use crate::messages::{Message, PanelSide};
use crate::palette::Palette;

/// Medium rather than large: the list is short, and a large frame would be
/// mostly empty.
const SIZE: DialogSize = DialogSize::Medium;

/// Rows shown at once. A list longer than this scrolls with the highlight, so
/// the dialog stays inside the window with many mounts.
const VISIBLE_ROWS: usize = 12;

/// The name column takes two parts of five, the path three.
const NAME_SHARE: f32 = 0.4;
const PATH_SHARE: f32 = 0.6;

/// A favourite folder as the menu lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FavouriteRow {
    pub path: PathBuf,
    /// Whether the folder existed when the menu opened. A missing one stays in
    /// the list, dimmed.
    pub present: bool,
}

impl FavouriteRow {
    /// The last component; a root has none and shows the whole path.
    fn name(&self) -> String {
        self.path.file_name().map_or_else(
            || self.path.to_string_lossy().into_owned(),
            |name| name.to_string_lossy().into_owned(),
        )
    }
}

/// A server as the menu lists it: a saved one, or one connected now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerRow {
    pub label: String,
    /// The address, which is also the path the panel is sent to.
    pub url: PathBuf,
    pub server: ServerId,
    /// The connection is open at the moment.
    pub connected: bool,
}

const SERVER_HINTS: &[PageHint] = &[PageHint {
    key: if cfg!(target_os = "macos") {
        "⌫"
    } else {
        "Del"
    },
    verb: Msg::VolumesHintDisconnect,
}];

/// The footer names the key that removes a favourite only while one is
/// highlighted.
const FAVOURITE_HINTS: &[PageHint] = &[PageHint {
    // The Mac key that sends Backspace is labelled ⌫ and has no Del.
    key: if cfg!(target_os = "macos") {
        "⌫"
    } else {
        "Del"
    },
    verb: Msg::VolumesHintRemoveFavourite,
}];
const NO_HINTS: &[PageHint] = &[];

/// Index of the first line shown when the line at `position` is highlighted:
/// the window only moves once the highlight would leave it. Lines are the
/// drives, then the heading of the favourites and the favourites, then the
/// heading of the servers and the servers, then a rule and the trash.
pub fn first_visible(position: usize) -> usize {
    (position + 1).saturating_sub(VISIBLE_ROWS)
}

enum Line<'a> {
    Drive(usize, &'a Volume),
    Heading(Msg),
    Favourite(usize, &'a FavouriteRow),
    Server(usize, &'a ServerRow),
    /// The rule above the trash row.
    Divider,
    Trash(usize, Msg),
}

impl Line<'_> {
    /// The row of `selected` this line is, if it is one.
    fn row(&self) -> Option<usize> {
        match self {
            Self::Drive(index, _)
            | Self::Favourite(index, _)
            | Self::Server(index, _)
            | Self::Trash(index, _) => Some(*index),
            Self::Heading(_) | Self::Divider => None,
        }
    }
}

/// What the menu lists, in this order.
#[derive(Debug, Clone, Copy)]
pub struct MenuRows<'a> {
    pub volumes: &'a [Volume],
    pub favourites: &'a [FavouriteRow],
    pub servers: &'a [ServerRow],
    /// The last row: the trash, named for what this build can show of it.
    pub trash: Option<Msg>,
}

pub fn view<'a>(
    side: PanelSide,
    listed: MenuRows<'a>,
    selected: usize,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let MenuRows {
        volumes,
        favourites,
        servers,
        trash,
    } = listed;
    let (subtitle, theme_name) = match side {
        PanelSide::Left => (Msg::VolumesSubtitleLeft, FuturamaName::ActionDriveMenuLeft),
        PanelSide::Right => (
            Msg::VolumesSubtitleRight,
            FuturamaName::ActionDriveMenuRight,
        ),
    };
    let mut lines: Vec<Line<'a>> = volumes
        .iter()
        .enumerate()
        .map(|(index, volume)| Line::Drive(index, volume))
        .collect();
    if !favourites.is_empty() {
        lines.push(Line::Heading(Msg::VolumesFavouritesHeading));
        lines.extend(
            favourites
                .iter()
                .enumerate()
                .map(|(index, row)| Line::Favourite(volumes.len() + index, row)),
        );
    }
    if !servers.is_empty() {
        lines.push(Line::Heading(Msg::VolumesServersHeading));
        let before = volumes.len() + favourites.len();
        lines.extend(
            servers
                .iter()
                .enumerate()
                .map(|(index, row)| Line::Server(before + index, row)),
        );
    }
    if let Some(label) = trash {
        lines.push(Line::Divider);
        lines.push(Line::Trash(
            volumes.len() + favourites.len() + servers.len(),
            label,
        ));
    }
    let position = lines
        .iter()
        .position(|line| line.row() == Some(selected))
        .unwrap_or(0);
    let first = first_visible(position);
    let row_width = frame::content_width(SIZE, window) - 2.0 * spacing::CELL_PADDING_X;
    let name_chars = frame::chars_that_fit(row_width * NAME_SHARE);
    let path_chars = frame::chars_that_fit(row_width * PATH_SHARE);
    let missing = lang.text(Msg::VolumesMissing);
    let rows = lines
        .into_iter()
        .skip(first)
        .take(VISIBLE_ROWS)
        .map(|line| {
            let (index, name, path, present) = match line {
                Line::Heading(title) => {
                    return column![
                        kit::divider(palette),
                        text(lang.text(title))
                            .size(font_size::BODY)
                            .color(palette.dim_text),
                    ]
                    .padding([0.0, spacing::CELL_PADDING_X])
                    .spacing(spacing::LINE_GAP)
                    .into();
                }
                Line::Divider => return kit::divider(palette),
                Line::Trash(index, label) => {
                    (index, lang.text(label).to_owned(), String::new(), true)
                }
                Line::Drive(index, volume) => (
                    index,
                    volume.name.clone(),
                    volume.path.to_string_lossy().into_owned(),
                    true,
                ),
                Line::Favourite(index, row) => (
                    index,
                    row.name(),
                    row.path.to_string_lossy().into_owned(),
                    row.present,
                ),
                Line::Server(index, row) => {
                    let name = if row.connected {
                        format!("{} ({})", row.label, lang.text(Msg::VolumesServerConnected))
                    } else {
                        row.label.clone()
                    };
                    (index, name, row.url.to_string_lossy().into_owned(), true)
                }
            };
            let is_selected = index == selected;
            let name_color = match (is_selected, present) {
                (true, _) => palette.cursor_text,
                (false, true) => palette.directory,
                (false, false) => palette.dim_text,
            };
            let path_color = if is_selected {
                palette.cursor_text
            } else {
                palette.dim_text
            };
            let shown_name = if present {
                format::ellipsize_end(&name, name_chars)
            } else {
                format!("{} ({missing})", format::ellipsize_end(&name, name_chars))
            };
            let content = Row::with_children([
                container(
                    text(shown_name)
                        .size(font_size::BODY)
                        .color(name_color)
                        .wrapping(text::Wrapping::None),
                )
                .width(Length::FillPortion(2))
                .clip(true)
                .into(),
                container(
                    text(format::ellipsize_middle(&path, path_chars))
                        .size(font_size::BODY)
                        .color(path_color)
                        .wrapping(text::Wrapping::None),
                )
                .width(Length::FillPortion(3))
                .clip(true)
                .into(),
            ])
            .spacing(spacing::CELL_PADDING_X);
            button(content)
                .on_press(Message::VolumeMenuClick(index))
                .width(Length::Fill)
                .style(theme::row(palette, false, is_selected, true))
                .into()
        });

    let on_favourite = !favourites.is_empty()
        && (volumes.len()..volumes.len() + favourites.len()).contains(&selected);
    let on_connected_server = selected
        .checked_sub(volumes.len() + favourites.len())
        .and_then(|index| servers.get(index))
        .is_some_and(|row| row.connected);
    Frame::new(
        lang.text(subtitle),
        column![Column::with_children(rows)],
        SIZE,
        window,
        lang,
        palette,
    )
    .title_theme(lang.futurama_name(theme_name))
    .on_close(Message::VolumeMenuClose)
    // Enter goes to the highlighted row, which the content defines.
    .hints(KeyHints::default())
    .page_hints(if on_favourite {
        FAVOURITE_HINTS
    } else if on_connected_server {
        SERVER_HINTS
    } else {
        NO_HINTS
    })
    .into_element()
}
