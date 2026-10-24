//! The row across the window top: a tab with the path of each panel, the
//! Hermes chip and the Lab button. Where the system draws no window frame it
//! also holds the window buttons, and the window edges resize.
use std::path::{Path, MAIN_SEPARATOR};

use iced::widget::{button, column, container, mouse_area, row, text, Space};
use iced::{alignment, mouse, Element, Length, Padding};

use super::format::ellipsize_middle;
use super::layout::{TITLE_BAR_HEIGHT, TRAFFIC_LIGHTS_INSET};
use super::theme::{self, font_size, spacing};
use crate::i18n::{Language, Msg};
use crate::messages::{
    CommandPaletteMessage, FilingStatusMessage, Message, PanelSide, ResizeEdge, TitleBarMessage,
};
use crate::palette::Palette;

pub const LEFT_TAB_ID: &str = "title-tab-left";
pub const RIGHT_TAB_ID: &str = "title-tab-right";
pub const HERMES_CHIP_ID: &str = "title-hermes-chip";

/// The macOS window has its own buttons (the traffic lights); the others get
/// ours.
const OWN_WINDOW_BUTTONS: bool = cfg!(not(target_os = "macos"));
const WINDOW_BUTTON_WIDTH: f32 = 44.0;
const TAB_HEIGHT: f32 = TITLE_BAR_HEIGHT - 10.0;
const ROW_SPACING: f32 = 12.0;
const TAB_SPACING: f32 = 6.0;
/// Width of a character of the monospace font, in font sizes.
const GLYPH_WIDTH_PER_SIZE: f32 = 0.6;
const LOADING_MARK: &str = "  …";
/// Width of the resize handles along the window edges.
const EDGE_WIDTH: f32 = 5.0;

/// A path tab: what it names and what a click on it does.
pub struct Tab<'a> {
    pub side: PanelSide,
    pub title: TabTitle<'a>,
    pub active: bool,
}

pub enum TabTitle<'a> {
    Path {
        path: &'a Path,
        loading: bool,
    },
    /// The search view stands in for the panel; its title is already worded.
    Search(String),
}

pub struct TitleBar<'a> {
    pub tabs: [Tab<'a>; 2],
    pub home: &'a Path,
    /// The chip text while Hermes is on; `None` hides the chip.
    pub hermes: Option<String>,
    /// The key of the Lab, if it has one.
    pub lab_shortcut: Option<String>,
    pub window_width: f32,
    pub lang: Language,
    pub palette: &'a Palette,
}

/// `path` as the tab shows it: `~` for the home folder, cut in the middle to
/// at most `max_chars` characters so the last folder stays visible.
pub fn tab_label(path: &Path, home: &Path, loading: bool, max_chars: usize) -> String {
    let reserved = if loading {
        LOADING_MARK.chars().count()
    } else {
        0
    };
    let mut label = ellipsize_middle(
        &home_relative(path, home),
        max_chars.saturating_sub(reserved),
    );
    if loading {
        label.push_str(LOADING_MARK);
    }
    label
}

fn home_relative(path: &Path, home: &Path) -> String {
    if home.as_os_str().is_empty() {
        return path.display().to_string();
    }
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~{MAIN_SEPARATOR}{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

fn characters_that_fit(width: f32, font_size: f32) -> usize {
    (width / (font_size * GLYPH_WIDTH_PER_SIZE))
        .floor()
        .max(1.0) as usize
}

fn left_inset() -> f32 {
    if cfg!(target_os = "macos") {
        TRAFFIC_LIGHTS_INSET
    } else {
        spacing::OUTER_PADDING
    }
}

/// How many characters one tab's label may have, once the right end of the row
/// (`right_chars` characters of chip and button) and the window buttons have
/// their room. The two tabs share what is left.
pub fn tab_chars(window_width: f32, right_chars: usize) -> usize {
    let buttons = if OWN_WINDOW_BUTTONS {
        3.0 * WINDOW_BUTTON_WIDTH
    } else {
        0.0
    };
    let right =
        right_chars as f32 * font_size::STATUS * GLYPH_WIDTH_PER_SIZE + 3.0 * ROW_SPACING + buttons;
    let tabs = window_width - left_inset() - spacing::OUTER_PADDING - right - TAB_SPACING;
    let per_tab = tabs / 2.0 - 2.0 * spacing::CELL_PADDING_X;
    characters_that_fit(per_tab, font_size::TITLE)
}

pub fn view<'a>(bar: &TitleBar<'a>) -> Element<'a, Message> {
    let palette = bar.palette;
    let lab = bar.lang.text(Msg::ShortcutCommands);
    let lab_theme = bar.lang.text(Msg::ActionCommandPaletteTheme);
    let lab_shortcut = bar.lab_shortcut.clone().unwrap_or_default();
    let right_chars = bar.hermes.as_ref().map_or(0, |chip| chip.chars().count())
        + lab.chars().count()
        + lab_theme.chars().count()
        + lab_shortcut.chars().count();
    let chars = tab_chars(bar.window_width, right_chars);

    let tab = |tab: &Tab<'a>, id: &'static str| -> Element<'a, Message> {
        let label = match &tab.title {
            TabTitle::Path { path, loading } => tab_label(path, bar.home, *loading, chars),
            TabTitle::Search(title) => ellipsize_middle(title, chars),
        };
        container(
            button(
                text(label)
                    .size(font_size::TITLE)
                    .height(Length::Fill)
                    .align_y(alignment::Vertical::Center)
                    .wrapping(text::Wrapping::None),
            )
            .on_press(Message::ActivatePanel(tab.side))
            .padding([0.0, spacing::CELL_PADDING_X])
            .width(Length::Fill)
            .height(TAB_HEIGHT)
            .clip(true)
            .style(theme::title_tab(palette, tab.active)),
        )
        .id(id)
        .width(Length::Fill)
        .into()
    };
    let [left, right] = &bar.tabs;
    let tabs = row![tab(left, LEFT_TAB_ID), tab(right, RIGHT_TAB_ID)]
        .spacing(TAB_SPACING)
        .width(Length::Fill);

    let mut bar_row = row![tabs].spacing(ROW_SPACING);
    if let Some(chip) = bar.hermes.clone() {
        bar_row = bar_row.push(
            container(
                button(text(chip).size(font_size::STATUS))
                    .on_press(Message::FilingStatus(FilingStatusMessage::Open))
                    .padding([2.0, 6.0])
                    .style(theme::title_button(palette)),
            )
            .id(HERMES_CHIP_ID),
        );
    }
    bar_row = bar_row.push(
        button(
            row![
                text(lab).size(font_size::STATUS),
                text(lab_theme).size(font_size::COLUMN_HEADER),
                text(lab_shortcut).size(font_size::COLUMN_HEADER),
            ]
            .spacing(TAB_SPACING)
            .align_y(alignment::Vertical::Center),
        )
        .on_press(Message::CommandPalette(CommandPaletteMessage::Toggle))
        .padding([2.0, 6.0])
        .style(theme::title_button(palette)),
    );
    if OWN_WINDOW_BUTTONS {
        bar_row = bar_row.push(window_buttons(palette));
    }

    mouse_area(
        container(bar_row.align_y(alignment::Vertical::Center))
            .padding(Padding {
                left: left_inset(),
                right: if OWN_WINDOW_BUTTONS {
                    0.0
                } else {
                    spacing::OUTER_PADDING
                },
                ..Padding::ZERO
            })
            .width(Length::Fill)
            .height(TITLE_BAR_HEIGHT)
            .align_y(alignment::Vertical::Center),
    )
    .on_press(Message::TitleBar(TitleBarMessage::Drag))
    .on_double_click(Message::TitleBar(TitleBarMessage::ToggleMaximize))
    .into()
}

fn window_buttons<'a>(palette: &Palette) -> Element<'a, Message> {
    let window_button = |glyph: &'static str, message: Message| {
        button(
            text(glyph)
                .size(font_size::TITLE)
                .align_x(alignment::Horizontal::Center)
                .width(Length::Fill),
        )
        .on_press(message)
        .width(WINDOW_BUTTON_WIDTH)
        .height(TITLE_BAR_HEIGHT)
        .style(theme::title_button(palette))
    };
    row![
        window_button("–", Message::TitleBar(TitleBarMessage::Minimize)),
        window_button("□", Message::TitleBar(TitleBarMessage::ToggleMaximize)),
        window_button("×", Message::Quit),
    ]
    .into()
}

fn resize_handle<'a>(edge: ResizeEdge, width: Length, height: Length) -> Element<'a, Message> {
    let interaction = match edge {
        ResizeEdge::North | ResizeEdge::South => mouse::Interaction::ResizingVertically,
        ResizeEdge::East | ResizeEdge::West => mouse::Interaction::ResizingHorizontally,
        ResizeEdge::NorthEast | ResizeEdge::SouthWest => mouse::Interaction::ResizingDiagonallyUp,
        ResizeEdge::NorthWest | ResizeEdge::SouthEast => mouse::Interaction::ResizingDiagonallyDown,
    };
    mouse_area(Space::new().width(width).height(height))
        .on_press(Message::TitleBar(TitleBarMessage::Resize(edge)))
        .interaction(interaction)
        .into()
}

/// Thin handles along the window edges and corners, over `content`. Only for
/// windows the system draws no frame for: there nothing else resizes them.
pub fn with_resize_edges<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    let thin = Length::Fixed(EDGE_WIDTH);
    let handles = column![
        row![
            resize_handle(ResizeEdge::NorthWest, thin, thin),
            resize_handle(ResizeEdge::North, Length::Fill, thin),
            resize_handle(ResizeEdge::NorthEast, thin, thin),
        ],
        row![
            resize_handle(ResizeEdge::West, thin, Length::Fill),
            Space::new().width(Length::Fill),
            resize_handle(ResizeEdge::East, thin, Length::Fill),
        ]
        .height(Length::Fill),
        row![
            resize_handle(ResizeEdge::SouthWest, thin, thin),
            resize_handle(ResizeEdge::South, Length::Fill, thin),
            resize_handle(ResizeEdge::SouthEast, thin, thin),
        ],
    ]
    .width(Length::Fill)
    .height(Length::Fill);
    iced::widget::Stack::with_children([content, handles.into()]).into()
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

    #[test]
    fn a_path_below_home_starts_with_a_tilde() {
        let home = Path::new("/home/test");
        assert_eq!(tab_label(home, home, false, 40), "~");
        let below = home.join("Documents");
        assert_eq!(
            tab_label(&below, home, false, 40),
            format!("~{MAIN_SEPARATOR}Documents")
        );
        assert_eq!(
            tab_label(Path::new("/home/tester"), home, false, 40),
            "/home/tester"
        );
    }

    #[test]
    fn a_long_path_is_cut_in_the_middle_and_keeps_its_last_folder() {
        let home = Path::new("/home/test");
        let deep = home.join("Company A/Contracts/2026/Quarter 3/Signed copies");
        let label = tab_label(&deep, home, false, 24);
        assert_eq!(label.chars().count(), 24);
        assert!(label.starts_with('~'), "{label}");
        assert!(label.contains('…'), "{label}");
        assert!(label.ends_with("Signed copies"), "{label}");
    }

    #[test]
    fn a_loading_panel_keeps_the_mark_inside_the_budget() {
        let home = Path::new("/home/test");
        let deep = home.join("Company A/Contracts/2026/Quarter 3");
        let label = tab_label(&deep, home, true, 24);
        assert_eq!(label.chars().count(), 24);
        assert!(label.ends_with(LOADING_MARK), "{label}");
    }

    #[test]
    fn a_wider_window_gives_the_tabs_more_characters() {
        assert!(tab_chars(1600.0, 30) > tab_chars(900.0, 30));
        assert!(tab_chars(900.0, 10) > tab_chars(900.0, 40));
        assert_eq!(tab_chars(10.0, 80), 1);
    }
}
