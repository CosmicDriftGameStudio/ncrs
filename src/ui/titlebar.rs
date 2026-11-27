//! The row across the window top: a tab with the path of each panel, the
//! Where the system draws no window frame it also holds the window buttons,
//! and the window edges resize.
use std::path::Path;

use iced::widget::{button, column, container, mouse_area, row, text, Space};
use iced::{alignment, mouse, Element, Length, Padding};

use super::format::{ellipsize_end, ellipsize_middle, estimated_text_width, home_relative};
use super::kit::{self, TAB_LABEL_GAP as TAB_GAP, TAB_PADDING_X};
use super::layout::{TITLE_BAR_HEIGHT, TRAFFIC_LIGHTS_INSET};
use super::theme::{self, font_size, size, spacing};
use crate::i18n::{Language, Msg};
use crate::messages::PanelMessage;
use crate::messages::{GitTabId, ShownTab};
use crate::messages::{
    HermesChatMessage, HitListMessage, Message, PanelSide, ResizeEdge, SettingsMessage,
    TitleBarMessage,
};
use crate::palette::Palette;

pub const LEFT_TAB_ID: &str = "title-tab-left";
pub const RIGHT_TAB_ID: &str = "title-tab-right";
pub const SETTINGS_TAB_ID: &str = "title-tab-settings";
pub const MONITOR_TAB_ID: &str = "title-tab-monitor";
pub const VIEWER_TAB_ID: &str = "title-tab-viewer";
pub const NOTES_TAB_ID: &str = "title-tab-notes";
pub const FILING_TAB_ID: &str = "title-tab-filing";
pub const REPOS_TAB_ID: &str = "title-tab-repos";
pub const SPACE_TAB_ID: &str = "title-tab-space";
pub const GIT_TAB_ID: &str = "title-tab-git";
pub const HERMES_TAB_ID: &str = "title-tab-hermes";
/// Characters of room the cross of a hit list tab takes from its title.
const CROSS_CHARS: usize = 3;

/// The macOS window has its own buttons (the traffic lights); the others get
/// ours.
const OWN_WINDOW_BUTTONS: bool = cfg!(not(target_os = "macos"));
const WINDOW_BUTTON_WIDTH: f32 = 44.0;
/// Room the panel letter at the right end of a tab takes.
const TAB_LETTER_WIDTH: f32 = 8.0;
/// Room the cross of a page tab takes.
const PAGE_CROSS_WIDTH: f32 = 22.0;
const RIGHT_PADDING: f32 = spacing::M;
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
    /// The trash view stands in for the panel; its title is already worded.
    Trash(String),
    /// A hit list stands in for the panel; the tab has a cross that closes it.
    HitList(String),
}

/// The page that stands in for the panels, as a tab with a cross.
pub enum PageTab {
    Settings,
    System,
    /// The file viewer; the title is the file name.
    Viewer(String),
    Notes,
    /// The Repos tab, every repository of the machine.
    Repos,
    /// The Platz tab: what takes the space on the drive.
    Space,
    /// The filing tab; the title is already worded.
    Filing(String),
    /// A Git tab; the title is already worded, the name is the repository alone.
    Git(GitTabId, String, String),
    Hermes,
}

pub struct PageTabState {
    pub page: PageTab,
    pub active: bool,
}

pub struct TitleBar<'a> {
    pub tabs: [Tab<'a>; 2],
    pub home: &'a Path,
    /// The tabs of the open pages after the two path tabs.
    pub page_tabs: Vec<PageTabState>,
    /// The window width, which the tabs share with the right side.
    pub window_width: f32,
    /// The first tab that shows while not all of them fit.
    pub first_tab: usize,
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

fn characters_that_fit(width: f32, font_size: f32) -> usize {
    (width / estimated_text_width(1, font_size))
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

/// How many characters of the path a tab of `width` shows.
fn tab_chars(width: f32) -> usize {
    let room = width - 2.0 * TAB_PADDING_X - TAB_GAP - TAB_LETTER_WIDTH;
    characters_that_fit(room, font_size::FILE)
}

/// How many characters of its title a page tab of `width` shows.
fn page_tab_chars(width: f32) -> usize {
    let room = width - 2.0 * TAB_PADDING_X - TAB_GAP - PAGE_CROSS_WIDTH;
    characters_that_fit(room, font_size::FILE)
}

/// The room beside the tabs: the window margin and, where we draw them, the
/// window buttons.
fn outside_tabs_width() -> f32 {
    let window_buttons = if OWN_WINDOW_BUTTONS {
        3.0 * WINDOW_BUTTON_WIDTH
    } else {
        RIGHT_PADDING
    };
    left_inset() + window_buttons
}

pub fn view<'a>(bar: &TitleBar<'a>) -> Element<'a, Message> {
    let palette = bar.palette;
    let lang = bar.lang;
    let layout = kit::tab_strip_layout(
        bar.tabs.len(),
        bar.page_tabs.len(),
        (bar.window_width - outside_tabs_width()).max(size::TAB_MIN_WIDTH),
        bar.page_tabs.iter().position(|tab| tab.active),
        bar.first_tab,
    );
    let tab_width = layout.tab_width;
    let chars = tab_chars(layout.pinned_width);
    let page_chars = (tab_width < size::TAB_WIDTH).then(|| page_tab_chars(tab_width));

    let tab = |tab: &Tab<'a>, id: &'static str, letter_of: Msg| -> Element<'a, Message> {
        let closable = matches!(tab.title, TabTitle::HitList(_));
        let label_chars = if closable {
            chars.saturating_sub(CROSS_CHARS)
        } else {
            chars
        };
        let label = match &tab.title {
            TabTitle::Path { path, loading } => tab_label(path, bar.home, *loading, chars),
            TabTitle::Search(title) | TabTitle::Trash(title) | TabTitle::HitList(title) => {
                ellipsize_middle(title, label_chars)
            }
        };
        let letter: String = lang.text(letter_of).chars().take(1).collect();
        kit::title_tab(
            id,
            label,
            tab.active,
            Message::Panel(PanelMessage::ActivatePanel(tab.side)),
            closable.then_some(Message::HitList(HitListMessage::Close(tab.side))),
        )
        .mark(letter)
        .left_edge(tab.active)
        .width(layout.pinned_width)
        .view(palette)
    };
    let [left, right] = &bar.tabs;
    let fit = TabFit {
        palette,
        width: tab_width,
        chars: page_chars,
    };
    let pinned: Vec<Element<'a, Message>> = vec![
        tab(left, LEFT_TAB_ID, Msg::StatusModeLeft),
        tab(right, RIGHT_TAB_ID, Msg::StatusModeRight),
    ];
    let mut others: Vec<Element<'a, Message>> = Vec::new();
    for open in &bar.page_tabs {
        others.push(match &open.page {
            PageTab::Settings => page_tab(
                lang.text(Msg::SettingsTab),
                SETTINGS_TAB_ID,
                open.active,
                (
                    ShownTab::Settings,
                    Message::Settings(SettingsMessage::Close),
                ),
                fit,
            ),
            PageTab::System => page_tab(
                lang.text(Msg::MonitorTitle),
                MONITOR_TAB_ID,
                open.active,
                (
                    ShownTab::Monitor,
                    Message::Monitor(ncrs_monitor::MonitorMessage::Close),
                ),
                fit,
            ),
            PageTab::Viewer(title) => page_tab(
                title,
                VIEWER_TAB_ID,
                open.active,
                (
                    ShownTab::Viewer,
                    Message::Viewer(ncrs_viewer::Message::Close),
                ),
                fit,
            ),
            PageTab::Notes => page_tab(
                lang.text(Msg::ActionNotes),
                NOTES_TAB_ID,
                open.active,
                (ShownTab::Notes, Message::Notes(ncrs_notes::Message::Close)),
                fit,
            ),
            PageTab::Repos => page_tab(
                lang.text(Msg::ReposTitle),
                REPOS_TAB_ID,
                open.active,
                (ShownTab::Repos, Message::CloseRepos),
                fit,
            ),
            PageTab::Space => page_tab(
                lang.text(Msg::SpaceTabTitle),
                SPACE_TAB_ID,
                open.active,
                (ShownTab::Space, Message::CloseSpace),
                fit,
            ),
            PageTab::Git(id, title, name) => page_tab_named(
                title,
                name,
                format!("{GIT_TAB_ID}-{}", id.0),
                open.active,
                (
                    ShownTab::Git(*id),
                    Message::GitView(*id, ncrs_git_view::Message::Close),
                ),
                fit,
            ),
            PageTab::Filing(title) => page_tab(
                title,
                FILING_TAB_ID,
                open.active,
                (
                    ShownTab::Filing,
                    Message::FilingView(ncrs_filing_view::Message::Cancel),
                ),
                fit,
            ),
            PageTab::Hermes => page_tab(
                lang.text(Msg::HermesChatTitle),
                HERMES_TAB_ID,
                open.active,
                (
                    ShownTab::Hermes,
                    Message::HermesChat(HermesChatMessage::Close),
                ),
                fit,
            ),
        });
    }

    let mut bar_row = row![kit::tab_strip(
        layout,
        pinned,
        others,
        |first| Message::TitleBar(TitleBarMessage::TabsFrom(first)),
        palette,
    )]
    .height(Length::Fill)
    .align_y(alignment::Vertical::Center);
    bar_row = if OWN_WINDOW_BUTTONS {
        bar_row.push(window_buttons(palette))
    } else {
        bar_row.push(Space::new().width(RIGHT_PADDING))
    };

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

/// The tab of an open page, with a cross that closes it; a click shows it.
fn page_tab<'a>(
    title: &str,
    id: impl Into<iced::widget::Id>,
    active: bool,
    shown_and_close: (ShownTab, Message),
    fit: TabFit<'_>,
) -> Element<'a, Message> {
    page_tab_named(title, title, id, active, shown_and_close, fit)
}

/// [`page_tab`] for a title with a prefix: once the tab is narrow, `name`
/// alone is shown, and cut at the end only when that is still too long.
fn page_tab_named<'a>(
    title: &str,
    name: &str,
    id: impl Into<iced::widget::Id>,
    active: bool,
    (shown, close): (ShownTab, Message),
    fit: TabFit<'_>,
) -> Element<'a, Message> {
    let label = match fit.chars {
        Some(chars) if title.chars().count() <= chars => title.to_owned(),
        Some(chars) => ellipsize_end(name, chars),
        None => title.to_owned(),
    };
    kit::title_tab(id, label, active, Message::ShowTab(shown), Some(close))
        .width(fit.width)
        .view(fit.palette)
}

/// What a page tab gets of the strip: its width, and the characters its title
/// is cut to once the width is below the full tab width.
#[derive(Clone, Copy)]
struct TabFit<'p> {
    palette: &'p Palette,
    width: f32,
    chars: Option<usize>,
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
#[cfg(test)]
mod tests {
    use std::path::MAIN_SEPARATOR;

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
