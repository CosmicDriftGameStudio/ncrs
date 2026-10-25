//! The row across the window top: a tab with the path of each panel, the
//! Hermes status and the button for the command palette. Where the system
//! draws no window frame it also holds the window buttons, and the window
//! edges resize.
use std::path::{Path, MAIN_SEPARATOR};

use iced::widget::{button, column, container, mouse_area, row, text, Space};
use iced::{alignment, mouse, Color, Element, Font, Length, Padding};

use super::format::ellipsize_middle;
use super::layout::{TITLE_BAR_HEIGHT, TRAFFIC_LIGHTS_INSET};
use super::theme::{self, font_size, size, spacing};
use crate::i18n::{Language, Msg};
use crate::messages::{
    CommandPaletteMessage, FilingStatusMessage, Message, PanelSide, ResizeEdge, SettingsMessage,
    TitleBarMessage,
};
use crate::palette::Palette;

pub const LEFT_TAB_ID: &str = "title-tab-left";
pub const RIGHT_TAB_ID: &str = "title-tab-right";
pub const HERMES_CHIP_ID: &str = "title-hermes-chip";
pub const SETTINGS_TAB_ID: &str = "title-tab-settings";
const CLOSE_CROSS: &str = "×";

/// The macOS window has its own buttons (the traffic lights); the others get
/// ours.
const OWN_WINDOW_BUTTONS: bool = cfg!(not(target_os = "macos"));
const WINDOW_BUTTON_WIDTH: f32 = 44.0;
const TAB_PADDING_X: f32 = 12.0;
const TAB_GAP: f32 = 8.0;
/// Room the panel letter at the right end of a tab takes.
const TAB_LETTER_WIDTH: f32 = 8.0;
const RIGHT_GAP: f32 = 2.0;
const RIGHT_PADDING: f32 = 8.0;
const BUTTON_PADDING_X: f32 = 8.0;
const BUTTON_CONTENT_GAP: f32 = 6.0;
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

pub use crate::hermes_status::Tone as HermesTone;

pub struct TitleBar<'a> {
    pub tabs: [Tab<'a>; 2],
    pub home: &'a Path,
    /// The Hermes text while it is on; `None` hides it.
    pub hermes: Option<String>,
    /// How the dot of Hermes is coloured.
    pub hermes_tone: HermesTone,
    /// The key of the command palette, if it has one.
    pub lab_shortcut: Option<String>,
    /// The settings tab after the two path tabs; it is the active one then.
    pub settings_tab: bool,
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

/// How many characters of the path a tab shows.
fn tab_chars() -> usize {
    let room = size::TAB_WIDTH - 2.0 * TAB_PADDING_X - TAB_GAP - TAB_LETTER_WIDTH;
    characters_that_fit(room, font_size::FILE)
}

pub fn view<'a>(bar: &TitleBar<'a>) -> Element<'a, Message> {
    let palette = bar.palette;
    let lang = bar.lang;
    let chars = tab_chars();

    let tab = |tab: &Tab<'a>, id: &'static str, letter_of: Msg| -> Element<'a, Message> {
        let label = match &tab.title {
            TabTitle::Path { path, loading } => tab_label(path, bar.home, *loading, chars),
            TabTitle::Search(title) => ellipsize_middle(title, chars),
        };
        let letter: String = lang.text(letter_of).chars().take(1).collect();
        let content = button(
            row![
                text(label)
                    .size(font_size::FILE)
                    .font(Font::MONOSPACE)
                    .width(Length::Fill)
                    .wrapping(text::Wrapping::None),
                text(letter).size(font_size::SMALL).color(palette.dim_text),
            ]
            .spacing(TAB_GAP)
            .height(Length::Fill)
            .align_y(alignment::Vertical::Center),
        )
        .on_press(Message::ActivatePanel(tab.side))
        .padding([0.0, TAB_PADDING_X])
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .style(theme::title_tab(palette, tab.active));
        let top_line = if tab.active {
            palette.accent
        } else {
            Color::TRANSPARENT
        };
        let left_edge: Element<'a, Message> = if tab.active {
            theme::vertical_rule(palette.border_inactive)
        } else {
            Space::new().width(0.0).into()
        };
        container(row![
            left_edge,
            column![theme::horizontal_rule(top_line), content],
            theme::vertical_rule(palette.border_inactive),
        ])
        .id(id)
        .width(size::TAB_WIDTH)
        .height(Length::Fill)
        .into()
    };
    let [left, right] = &bar.tabs;
    let mut tabs = row![
        tab(left, LEFT_TAB_ID, Msg::StatusModeLeft),
        tab(right, RIGHT_TAB_ID, Msg::StatusModeRight),
    ]
    .height(Length::Fill);
    if bar.settings_tab {
        tabs = tabs.push(settings_tab(lang, palette));
    }

    let mut right_side = row![]
        .spacing(RIGHT_GAP)
        .align_y(alignment::Vertical::Center);
    if let Some(chip) = bar.hermes.clone() {
        let dot_color = match bar.hermes_tone {
            HermesTone::Failing => palette.error,
            // The palette has no warning colour; `kit::warning_box` uses the
            // accent for warnings, so the dot does too.
            HermesTone::Stuck => palette.accent,
            HermesTone::Paused => palette.dim_text,
            HermesTone::Fine => palette.success,
        };
        right_side = right_side.push(
            container(
                button(
                    row![
                        theme::status_dot(dot_color),
                        text(chip).size(font_size::BODY).color(palette.dim_text)
                    ]
                    .spacing(BUTTON_CONTENT_GAP)
                    .align_y(alignment::Vertical::Center),
                )
                .on_press(Message::FilingStatus(FilingStatusMessage::Open))
                .padding([0.0, BUTTON_PADDING_X])
                .height(size::BUTTON)
                .style(theme::icon_button(palette)),
            )
            .id(HERMES_CHIP_ID),
        );
    }
    right_side = right_side.push(
        button(
            row![
                text(lang.text(Msg::ShortcutCommands)).size(font_size::BODY),
                text(bar.lab_shortcut.clone().unwrap_or_default())
                    .size(font_size::BODY)
                    .color(palette.dim_text),
            ]
            .spacing(BUTTON_CONTENT_GAP)
            .align_y(alignment::Vertical::Center),
        )
        .on_press(Message::CommandPalette(CommandPaletteMessage::Toggle))
        .padding([0.0, BUTTON_PADDING_X])
        .height(size::BUTTON)
        .style(theme::title_command_button(palette)),
    );

    let mut bar_row = row![
        tabs,
        Space::new().width(Length::Fill),
        container(right_side).padding(Padding {
            right: if OWN_WINDOW_BUTTONS {
                0.0
            } else {
                RIGHT_PADDING
            },
            left: RIGHT_PADDING,
            ..Padding::ZERO
        }),
    ]
    .height(Length::Fill)
    .align_y(alignment::Vertical::Center);
    if OWN_WINDOW_BUTTONS {
        bar_row = bar_row.push(window_buttons(palette));
    }

    mouse_area(
        container(column![
            container(bar_row)
                .padding(Padding {
                    left: left_inset(),
                    ..Padding::ZERO
                })
                .width(Length::Fill)
                .height(Length::Fill),
            theme::horizontal_rule(palette.border_inactive),
        ])
        .width(Length::Fill)
        .height(TITLE_BAR_HEIGHT)
        .style(theme::statusbar(palette)),
    )
    .on_press(Message::TitleBar(TitleBarMessage::Drag))
    .on_double_click(Message::TitleBar(TitleBarMessage::ToggleMaximize))
    .into()
}

/// The tab of the settings: always the active one, with a cross that closes it.
fn settings_tab<'a>(lang: Language, palette: &Palette) -> Element<'a, Message> {
    let label = row![
        text(lang.text(Msg::SettingsTab).to_owned())
            .size(font_size::FILE)
            .width(Length::Fill)
            .wrapping(text::Wrapping::None),
        button(text(CLOSE_CROSS).size(font_size::FILE))
            .on_press(Message::Settings(SettingsMessage::Close))
            .padding([0.0, spacing::XS])
            .style(theme::close_button(palette)),
    ]
    .spacing(TAB_GAP)
    .height(Length::Fill)
    .align_y(alignment::Vertical::Center);
    container(row![
        theme::vertical_rule(palette.border_inactive),
        column![
            theme::horizontal_rule(palette.accent),
            container(label)
                .padding([0.0, TAB_PADDING_X])
                .width(Length::Fill)
                .height(Length::Fill)
                .style(theme::panel(palette)),
        ],
        theme::vertical_rule(palette.border_inactive),
    ])
    .id(SETTINGS_TAB_ID)
    .width(size::TAB_WIDTH)
    .height(Length::Fill)
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
        .height(Length::Fill)
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
}
