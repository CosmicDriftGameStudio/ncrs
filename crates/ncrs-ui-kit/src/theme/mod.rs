//! Colors, spacing and reusable widget styles (dark, in the classic dual-panel style).

use iced::widget::overlay::menu;
use iced::widget::{button, checkbox, container, pick_list, text_input, Space};
use iced::{Background, Border, Color, Element, Length, Theme};

use crate::hermes_state::{FolderRole, HermesFileState};
use crate::palette::{contrast_ratio, Palette};

mod settings;
pub use settings::{feature_item, focused_setting, marked_setting};

/// Behind a modal dialog: darkens the panels without hiding them. Not part of
/// the palette, so a theme cannot make the dialog's surroundings opaque.
pub const SCRIM: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.65,
};

pub mod spacing {
    pub const OUTER_PADDING: f32 = 6.0;
    pub const SECTION_GAP: f32 = 6.0;
    pub const CELL_PADDING_X: f32 = 8.0;
    pub const BORDER_WIDTH: f32 = 1.0;

    /// The scale of the dialogs and the kit.
    pub const XS: f32 = 4.0;
    pub const M: f32 = 8.0;
    pub const L: f32 = 12.0;
    pub const XL: f32 = 16.0;
    pub const XXL: f32 = 24.0;

    /// The one gap between the lines and rows of a dialog's content.
    pub const LINE_GAP: f32 = 6.0;
    /// Room before the heading of a section.
    pub const SECTION_LEAD: f32 = L;
}

/// Heights and widths of the dialog parts, in pixels.
pub mod size {
    pub const ROW: f32 = 22.0;
    pub const ROW_TALL: f32 = 24.0;
    pub const BUTTON: f32 = 24.0;
    pub const HEADER: f32 = 32.0;
    pub const FOOTER: f32 = 40.0;
    pub const CONTROL_WIDTH: f32 = 260.0;
    pub const SIDEBAR_WIDTH: f32 = 200.0;
    pub const SIDEBAR_ITEM: f32 = 26.0;

    /// The main window: the bars across top and bottom, the column titles and
    /// the path tabs.
    pub const TITLE_BAR: f32 = 34.0;
    pub const STATUS_BAR: f32 = 22.0;
    pub const FKEY_BAR: f32 = 24.0;
    /// The row of numbered favourites under the path tabs.
    pub const FAVOURITES_BAR: f32 = 24.0;
    /// The bar of a running folder comparison under the favourites bar.
    pub const COMPARE_BAR: f32 = 32.0;
    pub const COMMAND_LINE: f32 = 24.0;
    pub const COLUMN_HEADER: f32 = 22.0;
    /// The line of totals under the rows of a file panel.
    pub const PANEL_FOOTER: f32 = 22.0;
    pub const TAB_WIDTH: f32 = 240.0;
    pub const DOT: f32 = 6.0;
    /// The side of a status or folder icon: the line height of a panel row's text.
    pub const ICON: f32 = 14.0;
}

pub mod radius {
    /// The one corner radius of frames, controls, chips and key caps. List
    /// rows, tabs and bars are square.
    pub const CORNER: f32 = 4.0;
}

/// How much of a button's look stays while it is disabled.
pub const DISABLED_OPACITY: f32 = 0.45;

pub mod font_size {
    pub const TITLE: f32 = 14.0;
    /// The title of a dialog.
    pub const HEADER: f32 = 13.0;
    pub const ROW: f32 = 14.0;
    pub const COLUMN_HEADER: f32 = 12.0;
    pub const STATUS: f32 = 13.0;

    /// The text of dialogs and the kit.
    pub const BODY: f32 = 12.0;
    pub const KEY_CAP: f32 = 10.0;
    /// Column titles, hints and the status bar of the main window.
    pub const SMALL: f32 = 11.0;
    /// File names and paths, set in the monospace font.
    pub const FILE: f32 = 12.0;
    /// A number shown large.
    pub const FIGURE: f32 = 16.0;
}

/// Base theme handed to iced (our widgets override most colors).
pub fn app_theme() -> Theme {
    Theme::Dark
}

/// How much of the accent the outline of the element with the keyboard focus
/// keeps; the rest is the panel color.
const FOCUS_ACCENT_SHARE: f32 = 0.6;

/// How strongly the cursor color shows behind a hovered control.
const HOVER_ALPHA: f32 = 0.55;

fn mix(from: Color, to: Color, share: f32) -> Color {
    Color {
        r: from.r * share + to.r * (1.0 - share),
        g: from.g * share + to.g * (1.0 - share),
        b: from.b * share + to.b * (1.0 - share),
        a: from.a * share + to.a * (1.0 - share),
    }
}

/// The one look of keyboard focus: a 1 px outline in the accent, mixed with
/// the panel color. Yellow means focus and nothing else.
pub fn focus_border(palette: &Palette) -> Border {
    Border {
        color: mix(palette.accent, palette.panel, FOCUS_ACCENT_SHARE),
        width: spacing::BORDER_WIDTH,
        radius: radius::CORNER.into(),
    }
}

/// What lies behind a control while the pointer is over it. Hover is not focus,
/// so it never takes the accent.
pub fn hover_fill(palette: &Palette) -> Background {
    Background::Color(palette.cursor.scale_alpha(HOVER_ALPHA))
}

/// The lowest contrast a control outline may have against what it sits on.
const OUTLINE_CONTRAST: f32 = 3.0;

/// The grey outline of a control: the dim text color, or, where that is too
/// faint against `ground`, blended towards the text color in steps of 10 %
/// until it reaches [`OUTLINE_CONTRAST`].
pub fn control_outline(palette: &Palette, ground: Color) -> Color {
    let mut outline = palette.dim_text;
    for step in 1..=10 {
        if contrast_ratio(outline, ground) >= OUTLINE_CONTRAST {
            break;
        }
        outline = mix(palette.text, palette.dim_text, step as f32 / 10.0);
    }
    outline
}

fn filled(bg: Color, text: Color) -> container::Style {
    container::Style {
        background: Some(Background::Color(bg)),
        text_color: Some(text),
        ..container::Style::default()
    }
}

pub fn root(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| filled(colors.background, colors.text)
}

/// The bar across the bottom and the title bar.
pub fn statusbar(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| filled(colors.status_bar, colors.text)
}

/// A file panel: flat, the panels are told apart by a line between them.
pub fn panel(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| filled(colors.panel, colors.text)
}

/// A 1 px line, drawn as a container of that thickness.
pub fn line(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_theme| filled(color, color)
}

/// A thin horizontal rule across the full width.
pub fn horizontal_rule<'a, M: 'a>(color: Color) -> Element<'a, M> {
    container(Space::new())
        .width(Length::Fill)
        .height(spacing::BORDER_WIDTH)
        .style(line(color))
        .into()
}

/// A thin vertical rule across the full height.
pub fn vertical_rule<'a, M: 'a>(color: Color) -> Element<'a, M> {
    container(Space::new())
        .width(spacing::BORDER_WIDTH)
        .height(Length::Fill)
        .style(line(color))
        .into()
}

/// A small round status dot.
pub fn status_dot<'a, M: 'a>(color: Color) -> Element<'a, M> {
    container(Space::new())
        .width(size::DOT)
        .height(size::DOT)
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                radius: (size::DOT / 2.0).into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

/// The mode field at the left end of the status bar: names the active panel.
/// Filled with the "on" role, like every other thing that is switched on.
pub fn mode_field(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let (fill, ink) = palette.primary_button();
    move |_theme| filled(fill, ink)
}

/// The 2 px line across the top of the active tab; the inactive one has none.
pub fn tab_top_line<'a, M: 'a>(palette: &Palette, active: bool) -> Element<'a, M> {
    let color = if active {
        palette.primary_button().0
    } else {
        Color::TRANSPARENT
    };
    container(Space::new())
        .width(Length::Fill)
        .height(TAB_LINE_HEIGHT)
        .style(line(color))
        .into()
}

const TAB_LINE_HEIGHT: f32 = 2.0;

/// A path tab in the title bar: the active one has the panel's fill, the
/// inactive one shows the bar through.
pub fn title_tab(
    palette: &Palette,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    move |_theme, _status| {
        let (fill, text_color) = if active {
            (Some(Background::Color(colors.panel)), colors.text)
        } else {
            (None, colors.dim_text)
        };
        button::Style {
            background: fill,
            text_color,
            ..button::Style::default()
        }
    }
}

/// The button that opens the command palette: a subtle fill, lit while the
/// pointer is over it.
pub fn title_command_button(palette: &Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(Background::Color(if hovered {
                colors.border
            } else {
                colors.border_inactive
            })),
            text_color: colors.text,
            border: Border {
                radius: radius::CORNER.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    }
}

/// A slot of the favourites bar: flat, the label in the text color while the
/// pointer is over it or the slot is the active panel's folder, dim otherwise.
pub fn favourite_slot(
    palette: &Palette,
    is_current: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hovered.then_some(Background::Color(colors.border_inactive)),
            text_color: if hovered || is_current {
                colors.text
            } else {
                colors.dim_text
            },
            border: Border {
                radius: radius::CORNER.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    }
}

/// A flat button in the title bar; lit in the title colors while the pointer
/// is over it.
pub fn title_button(palette: &Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hovered.then_some(Background::Color(colors.title)),
            text_color: if hovered {
                colors.title_text
            } else {
                colors.text
            },
            border: Border {
                radius: radius::CORNER.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    }
}

/// Background of a file row (flat button, no hover chrome). The cursor only
/// shows in the active panel; a tagged row is tinted in either.
pub fn row(
    palette: &Palette,
    tagged: bool,
    cursor: bool,
    panel_active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    move |_theme, _status| {
        let background = match (cursor && panel_active, tagged, panel_active) {
            (true, _, _) => Some(Background::Color(colors.cursor)),
            (false, true, true) => Some(Background::Color(colors.tagged)),
            (false, true, false) => Some(Background::Color(colors.tagged_inactive)),
            (false, false, _) => None,
        };
        // The cursor of the other panel is only outlined, so it can be found
        // without looking active.
        let outline = cursor && !panel_active;
        button::Style {
            background,
            text_color: if cursor && panel_active {
                colors.cursor_text
            } else {
                colors.text
            },
            border: if outline {
                Border {
                    color: colors.cursor,
                    width: spacing::BORDER_WIDTH,
                    radius: 0.0.into(),
                }
            } else {
                Border::default()
            },
            ..button::Style::default()
        }
    }
}

/// Text color for a row depending on entry kind and selection state. A tagged
/// row without the cursor is tinted in the active panel only, and its text
/// follows the tint.
pub fn row_text_color(
    colors: &Palette,
    is_dir: bool,
    selected: bool,
    tagged: bool,
    panel_active: bool,
) -> Color {
    match (selected && panel_active, tagged && panel_active, is_dir) {
        (true, _, _) => colors.cursor_text,
        (false, true, true) => colors.tagged_directory,
        (false, true, false) => colors.tagged_text,
        (false, false, true) => colors.directory,
        (false, false, false) => colors.text,
    }
}

/// The star in front of a tagged row. The tint of the tagged row in the
/// active panel has its own color; on the cursor row the star takes the cursor
/// text and in the inactive panel the plain text color.
pub fn tag_star_color(colors: &Palette, cursor_row: bool, panel_active: bool) -> Color {
    match (cursor_row, panel_active) {
        (true, _) => colors.cursor_text,
        (false, true) => colors.tagged_mark,
        (false, false) => colors.text,
    }
}

/// Plain text on a tagged row: the tint of the active panel needs its own
/// color, the inactive one keeps the normal text.
pub fn tagged_text_color(colors: &Palette, panel_active: bool) -> Color {
    if panel_active {
        colors.tagged_text
    } else {
        colors.text
    }
}

/// The prompt panel: a raised box over the scrim.
pub fn dialog(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| container::Style {
        background: Some(Background::Color(colors.dialog)),
        text_color: Some(colors.text),
        border: Border {
            color: colors.border_inactive,
            width: spacing::BORDER_WIDTH,
            radius: radius::CORNER.into(),
        },
        ..container::Style::default()
    }
}

/// Buttons in the prompt. `primary` is the action the user most likely wants
/// and is filled in the "on" role; the others rest in a grey outline and light up
/// with the hover fill. Only a disabled button is dimmed,
/// so a card without a focused button does not look switched off.
pub fn dialog_button(
    palette: &Palette,
    primary: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    let (fill, text) = colors.primary_button();
    let outline = control_outline(&colors, colors.dialog);
    move |_theme, status| {
        let (background, text_color, border_color) = match status {
            button::Status::Disabled => (None, colors.dim_text, colors.border_inactive),
            _ if primary => (Some(Background::Color(fill)), text, fill),
            button::Status::Hovered | button::Status::Pressed => {
                (Some(hover_fill(&colors)), colors.text, outline)
            }
            button::Status::Active => (None, colors.text, outline),
        };
        button::Style {
            background,
            text_color,
            border: Border {
                color: border_color,
                width: spacing::BORDER_WIDTH,
                radius: radius::CORNER.into(),
            },
            ..button::Style::default()
        }
    }
}

/// The button of a destructive action: filled in the error color.
pub fn danger_button(palette: &Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    move |_theme, status| {
        let disabled = matches!(status, button::Status::Disabled);
        button::Style {
            background: (!disabled).then_some(Background::Color(colors.error)),
            text_color: if disabled {
                colors.dim_text
            } else {
                colors.cursor_text
            },
            border: Border {
                color: if disabled {
                    colors.border_inactive
                } else {
                    colors.error
                },
                width: spacing::BORDER_WIDTH,
                radius: radius::CORNER.into(),
            },
            ..button::Style::default()
        }
    }
}

/// The close cross of a dialog: flat and dimmed, lit while the pointer is over
/// it.
pub fn close_button(palette: &Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hovered.then_some(Background::Color(colors.title)),
            text_color: if hovered {
                colors.text
            } else {
                colors.dim_text
            },
            border: Border {
                radius: radius::CORNER.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    }
}

/// A key shown as a cap: outlined, small. `on_fill` is a cap drawn on a filled
/// button, so it takes the text color of that button.
pub fn key_cap(palette: &Palette, on_fill: bool) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    let outline = control_outline(&colors, colors.panel);
    move |_theme| container::Style {
        text_color: Some(if on_fill {
            colors.cursor_text
        } else {
            colors.dim_text
        }),
        border: Border {
            color: if on_fill { colors.cursor_text } else { outline },
            width: spacing::BORDER_WIDTH,
            radius: radius::CORNER.into(),
        },
        ..container::Style::default()
    }
}

/// A chip: a short label in a grey outline, no fill. On a filled
/// (highlighted) row it takes the colors of the highlight.
pub fn chip(palette: &Palette, on_fill: bool) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    let grey = control_outline(&colors, colors.panel);
    move |_theme| {
        let (ink, outline) = if on_fill {
            (colors.cursor_text, colors.cursor_text)
        } else {
            (colors.text, grey)
        };
        container::Style {
            text_color: Some(ink),
            border: Border {
                color: outline,
                width: spacing::BORDER_WIDTH,
                radius: radius::CORNER.into(),
            },
            ..container::Style::default()
        }
    }
}

/// A chip the user added himself: outlined in the "on" role instead of grey.
pub fn own_chip(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    let (on, _) = colors.primary_button();
    move |_theme| container::Style {
        text_color: Some(colors.text),
        border: Border {
            color: on,
            width: spacing::BORDER_WIDTH,
            radius: radius::CORNER.into(),
        },
        ..container::Style::default()
    }
}

/// One option of a segmented control: the chosen one is filled with the "on"
/// role, the others rest as dim text. The outline belongs to the group.
pub fn segment_button(
    palette: &Palette,
    chosen: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    let (on, on_text) = colors.primary_button();
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: if chosen {
                Some(Background::Color(on))
            } else {
                hovered.then(|| hover_fill(&colors))
            },
            text_color: if chosen { on_text } else { colors.dim_text },
            border: Border {
                radius: radius::CORNER.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    }
}

/// The one grey outline around a group of segments.
pub fn segment_group(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    let outline = control_outline(&colors, colors.panel);
    move |_theme| container::Style {
        border: Border {
            color: outline,
            width: spacing::BORDER_WIDTH,
            radius: radius::CORNER.into(),
        },
        ..container::Style::default()
    }
}

/// A flat button in the text color, for an action inside the content.
pub fn link_button(palette: &Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hovered.then(|| hover_fill(&colors)),
            text_color: colors.text,
            border: Border::default(),
            ..button::Style::default()
        }
    }
}

/// A line between two parts of a dialog.
pub fn divider(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| filled(colors.border_inactive, colors.text)
}

/// The side column of a dialog.
pub fn sidebar(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| filled(colors.panel, colors.text)
}

/// A box of the kit with a 1 px outline and no fill: a table or a summary.
pub fn outlined(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| container::Style {
        border: Border {
            color: colors.border_inactive,
            width: spacing::BORDER_WIDTH,
            radius: radius::CORNER.into(),
        },
        ..container::Style::default()
    }
}

/// An outlined box whose outline has the given color, for a notice.
pub fn outlined_in(outline: Color) -> impl Fn(&Theme) -> container::Style {
    move |_theme| container::Style {
        border: Border {
            color: outline,
            width: spacing::BORDER_WIDTH,
            radius: radius::CORNER.into(),
        },
        ..container::Style::default()
    }
}

/// A small box filled with a color, outlined so that it shows on a ground of
/// the same color.
pub fn swatch(fill: Color, outline: Color) -> impl Fn(&Theme) -> container::Style {
    move |_theme| container::Style {
        background: Some(fill.into()),
        border: Border {
            color: outline,
            width: spacing::BORDER_WIDTH,
            radius: radius::CORNER.into(),
        },
        ..container::Style::default()
    }
}

/// An inquiry is yellow in every preset; the palette has no warning role.
const INQUIRY_YELLOW: Color = Color {
    r: 0.898,
    g: 0.753,
    b: 0.482,
    a: 1.0,
};

/// How much of the dim text color an excluded file keeps.
const EXCLUDED_OPACITY: f32 = 0.55;

/// The color of a Hermes state in the status column.
pub fn hermes_state_color(palette: &Palette, state: HermesFileState) -> Color {
    match state {
        HermesFileState::Unread | HermesFileState::Planned => palette.dim_text,
        HermesFileState::Reading => palette.primary_button().0,
        HermesFileState::Filed => palette.success,
        HermesFileState::Inquiry => INQUIRY_YELLOW,
        HermesFileState::Error => palette.error,
        HermesFileState::Excluded => Color {
            a: EXCLUDED_OPACITY,
            ..palette.dim_text
        },
    }
}

/// The color of a folder's role tag.
pub fn hermes_role_color(palette: &Palette, role: FolderRole) -> Color {
    match role {
        FolderRole::Inbox => palette.primary_button().0,
        FolderRole::FilingLocation => palette.success,
        FolderRole::Archive => palette.dim_text,
    }
}

/// A thin progress bar: the track in the border color, the fill in success.
pub fn progress(palette: &Palette) -> impl Fn(&Theme) -> iced::widget::progress_bar::Style {
    let colors = *palette;
    move |_theme| iced::widget::progress_bar::Style {
        background: Background::Color(colors.border_inactive),
        bar: Background::Color(colors.success),
        border: Border {
            radius: radius::CORNER.into(),
            ..Border::default()
        },
    }
}

/// The star in front of a row. Flat, so it does not look like a button until it
/// is one: a tagged row is the only one with a visible mark.
pub fn tag_button(
    palette: &Palette,
    tagged: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    // A tagged star takes the marked-files color, an untagged one is dim. Which
    // panel is active does not change this — the cursor highlight says that.
    move |_theme, _status| button::Style {
        background: None,
        text_color: if tagged {
            colors.tagged_mark
        } else {
            colors.dim_text
        },
        border: Border::default(),
        ..button::Style::default()
    }
}

/// The "for all files" checkbox in the conflict dialog.
pub fn dialog_checkbox(palette: &Palette) -> impl Fn(&Theme, checkbox::Status) -> checkbox::Style {
    let colors = *palette;
    let (on, on_text) = colors.primary_button();
    move |_theme, status| {
        let checked = match status {
            checkbox::Status::Active { is_checked }
            | checkbox::Status::Hovered { is_checked }
            | checkbox::Status::Disabled { is_checked } => is_checked,
        };
        checkbox::Style {
            background: Background::Color(if checked { on } else { colors.panel }),
            icon_color: on_text,
            border: Border {
                color: if checked { on } else { colors.dim_text },
                width: spacing::BORDER_WIDTH,
                radius: radius::CORNER.into(),
            },
            text_color: Some(colors.text),
        }
    }
}

/// The box of the context menu.
pub fn menu(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| container::Style {
        background: Some(Background::Color(colors.menu)),
        text_color: Some(colors.text),
        border: Border {
            color: colors.border_inactive,
            width: spacing::BORDER_WIDTH,
            radius: radius::CORNER.into(),
        },
        ..container::Style::default()
    }
}

/// One entry of the context menu: flat, the highlighted one filled like the
/// cursor row of a panel.
pub fn menu_item(
    palette: &Palette,
    selected: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: if selected {
                Some(Background::Color(colors.cursor))
            } else {
                hovered.then(|| hover_fill(&colors))
            },
            text_color: if selected {
                colors.cursor_text
            } else {
                colors.text
            },
            border: Border::default(),
            ..button::Style::default()
        }
    }
}

/// The line between two groups of the context menu.
pub fn menu_separator(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| filled(colors.border, colors.text)
}

/// The path field in the status bar: it looks like the plain text it replaces,
/// focused or not. Only the caret and the selection show that it is a field.
pub fn path_input(palette: &Palette) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    let colors = *palette;
    move |_theme, _status| text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        icon: colors.dim_text,
        placeholder: colors.dim_text,
        value: colors.text,
        selection: colors.cursor,
    }
}

/// A text field of the search view: a plain box, so it reads as an input among
/// the flat rows around it.
pub fn search_input(palette: &Palette) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    let colors = *palette;
    move |_theme, status| text_input::Style {
        background: Background::Color(colors.status_bar),
        border: if matches!(status, text_input::Status::Focused { .. }) {
            focus_border(&colors)
        } else {
            Border {
                color: colors.border_inactive,
                width: spacing::BORDER_WIDTH,
                radius: radius::CORNER.into(),
            }
        },
        icon: colors.dim_text,
        placeholder: colors.dim_text,
        value: colors.text,
        selection: colors.cursor,
    }
}

/// A text field of a dialog: like [`search_input`], with the radius of the
/// kit's controls.
pub fn field_input(palette: &Palette) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    let colors = *palette;
    let base = search_input(palette);
    let outline = control_outline(&colors, colors.status_bar);
    move |theme, status| {
        let mut style = base(theme, status);
        if !matches!(status, text_input::Status::Focused { .. }) {
            style.border.color = outline;
        }
        style
    }
}

/// The closed field of a dropdown: outlined in grey like the other controls;
/// open, it has the keys and takes the focus outline.
pub fn dropdown_field(palette: &Palette) -> impl Fn(&Theme, pick_list::Status) -> pick_list::Style {
    let colors = *palette;
    let outline = control_outline(&colors, colors.dialog);
    move |_theme, status| pick_list::Style {
        text_color: colors.text,
        placeholder_color: colors.dim_text,
        handle_color: colors.dim_text,
        background: Background::Color(colors.dialog),
        border: match status {
            pick_list::Status::Opened { .. } => focus_border(&colors),
            pick_list::Status::Active | pick_list::Status::Hovered => Border {
                color: outline,
                width: spacing::BORDER_WIDTH,
                radius: radius::CORNER.into(),
            },
        },
    }
}

/// The open list of a dropdown; the chosen entry in the cursor colors.
pub fn dropdown_menu(palette: &Palette) -> impl Fn(&Theme) -> menu::Style {
    let colors = *palette;
    move |_theme| menu::Style {
        background: Background::Color(colors.menu),
        border: Border {
            color: colors.border_inactive,
            width: spacing::BORDER_WIDTH,
            radius: radius::CORNER.into(),
        },
        text_color: colors.text,
        selected_text_color: colors.cursor_text,
        selected_background: Background::Color(colors.cursor),
        shadow: iced::Shadow::default(),
    }
}

/// One of the match options next to the search field; lit while it is on.
pub fn search_toggle(
    palette: &Palette,
    on: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: (on || hovered).then_some(Background::Color(if on {
                colors.cursor
            } else {
                colors.title
            })),
            text_color: if on { colors.cursor_text } else { colors.text },
            border: Border {
                radius: radius::CORNER.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    }
}

/// A row of the search results. Not a button: a button captures the press, and
/// the double click of the area around it would never be reported.
pub fn search_row(
    palette: &Palette,
    cursor: bool,
    tagged: bool,
    panel_active: bool,
) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| {
        // As in [`row`]: the cursor wins over the tint, and shows in the active
        // panel only.
        if cursor {
            filled(colors.cursor, colors.cursor_text)
        } else if tagged {
            let tint = if panel_active {
                colors.tagged
            } else {
                colors.tagged_inactive
            };
            filled(tint, tagged_text_color(&colors, panel_active))
        } else {
            filled_text_only(colors.text)
        }
    }
}

fn filled_text_only(text: Color) -> container::Style {
    container::Style {
        text_color: Some(text),
        ..container::Style::default()
    }
}

/// A flat icon button; lit while the pointer is over it.
pub fn icon_button(palette: &Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hovered.then(|| hover_fill(&colors)),
            text_color: colors.text,
            border: Border {
                radius: radius::CORNER.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    }
}

/// One square of the copy icon; `filled` hides what lies behind it.
pub fn icon_square(palette: &Palette, filled: bool) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| container::Style {
        background: filled.then_some(Background::Color(colors.status_bar)),
        border: Border {
            color: colors.text,
            width: spacing::BORDER_WIDTH,
            radius: 1.0.into(),
        },
        ..container::Style::default()
    }
}

#[cfg(test)]
mod tests;
