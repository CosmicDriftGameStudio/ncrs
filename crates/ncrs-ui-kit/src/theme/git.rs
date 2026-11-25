//! The styles of the Git tab and the repository list: the pane splitter, the
//! header pickers, the state chips, the section heads, the ref labels and the
//! rows of the repository list.
//!
//! Purple is the "on" role of the palette ([`Palette::primary_button`]), the
//! same as a chosen segment; yellow stays for keyboard focus alone.

use iced::widget::{button, container};
use iced::{Background, Border, Color, Theme};

use super::{control_outline, focus_border, hover_fill, radius, spacing, DISABLED_OPACITY};
use crate::git_mark::{MODIFIED_ORANGE, UNTRACKED_CYAN};
use crate::palette::Palette;

/// How strongly a marked row is tinted.
const MARKED_TINT: f32 = 0.2;
/// How strongly a filled label shows its color on the ground.
const FILL_TINT: f32 = 0.18;

/// The purple of the palette: the current branch, what is ahead, the splitter
/// while it is dragged.
pub fn purple(palette: &Palette) -> Color {
    palette.primary_button().0
}

/// The orange of what is behind, of a detached head and of a skipped repository.
pub const fn orange() -> Color {
    MODIFIED_ORANGE
}

/// The cyan of local changes and of the work in progress.
pub const fn cyan() -> Color {
    UNTRACKED_CYAN
}

/// `ink` as faint as the text of a disabled control.
pub fn faint(ink: Color) -> Color {
    ink.scale_alpha(DISABLED_OPACITY)
}

/// What the pointer does to a splitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitterLook {
    Idle,
    Hovered,
    Dragged,
}

/// The thin bar between two panes: quiet at rest, purple under the pointer and
/// while it is dragged.
pub fn splitter(palette: &Palette, look: SplitterLook) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| {
        let fill = match look {
            SplitterLook::Idle => colors.border_inactive,
            SplitterLook::Hovered | SplitterLook::Dragged => purple(&colors),
        };
        container::Style {
            background: Some(Background::Color(fill)),
            ..container::Style::default()
        }
    }
}

/// The state of a header picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerLook {
    Idle,
    /// The keys are on it.
    Focused,
    /// Its list is open.
    Open,
}

/// The button of a header picker: a small label over the bold value.
pub fn header_picker(
    palette: &Palette,
    look: PickerLook,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let background = match look {
            PickerLook::Open => Some(Background::Color(colors.cursor)),
            PickerLook::Idle | PickerLook::Focused if hovered => Some(hover_fill(&colors)),
            PickerLook::Idle | PickerLook::Focused => None,
        };
        let border = match look {
            PickerLook::Focused => focus_border(&colors),
            PickerLook::Idle | PickerLook::Open => Border {
                radius: radius::CORNER.into(),
                ..Border::default()
            },
        };
        button::Style {
            background,
            text_color: match look {
                PickerLook::Open => colors.cursor_text,
                PickerLook::Idle | PickerLook::Focused => colors.text,
            },
            border,
            ..button::Style::default()
        }
    }
}

/// A chip in the color of what it says, outlined, with that color as its ink.
/// It is a button: a click on a chip asks for what it names.
pub fn state_chip(ink: Color) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hovered.then(|| Background::Color(ink.scale_alpha(FILL_TINT))),
            text_color: ink,
            border: Border {
                color: ink,
                width: spacing::BORDER_WIDTH,
                radius: radius::CORNER.into(),
            },
            ..button::Style::default()
        }
    }
}

/// The same chip without a click: the text of a state that has no action.
pub fn state_label(ink: Color) -> impl Fn(&Theme) -> container::Style {
    move |_theme| container::Style {
        text_color: Some(ink),
        border: Border {
            color: ink,
            width: spacing::BORDER_WIDTH,
            radius: radius::CORNER.into(),
        },
        ..container::Style::default()
    }
}

/// The dark bar that heads a group of rows. The rules above and below it are
/// the caller's; an empty group is dimmed.
pub fn section_head(palette: &Palette, empty: bool) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| container::Style {
        background: Some(Background::Color(colors.background)),
        text_color: Some(if empty { colors.dim_text } else { colors.text }),
        ..container::Style::default()
    }
}

/// The head of a group while it has the keys.
pub fn section_head_focused(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| container::Style {
        background: Some(Background::Color(colors.background)),
        text_color: Some(colors.text),
        border: focus_border(&colors),
        ..container::Style::default()
    }
}

/// What a ref label on a commit stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefLook {
    /// The checked-out branch: filled.
    Head,
    /// A local branch: outlined and inked in purple.
    Local,
    /// A branch of a remote: dim.
    Remote,
    Tag,
}

/// The ink of a ref label.
pub fn ref_ink(palette: &Palette, look: RefLook) -> Color {
    match look {
        RefLook::Head => palette.cursor_text,
        RefLook::Local => purple(palette),
        RefLook::Remote => palette.dim_text,
        RefLook::Tag => orange(),
    }
}

/// The box of a ref label.
pub fn ref_label(palette: &Palette, look: RefLook) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| {
        let outline = match look {
            RefLook::Head => purple(&colors),
            RefLook::Local => purple(&colors),
            RefLook::Remote => control_outline(&colors, colors.panel),
            RefLook::Tag => orange(),
        };
        container::Style {
            background: matches!(look, RefLook::Head).then(|| Background::Color(purple(&colors))),
            text_color: Some(ref_ink(&colors, look)),
            border: Border {
                color: outline,
                width: spacing::BORDER_WIDTH,
                radius: radius::CORNER.into(),
            },
            ..container::Style::default()
        }
    }
}

/// The row of the work in progress above the commits: a ring in cyan.
pub fn wip_ring() -> impl Fn(&Theme) -> container::Style {
    move |_theme| container::Style {
        text_color: Some(cyan()),
        border: Border {
            color: cyan(),
            width: spacing::BORDER_WIDTH,
            radius: radius::CORNER.into(),
        },
        ..container::Style::default()
    }
}

/// The bar of a run's result under the header of the repository list.
pub fn result_banner(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
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

/// The 3 px rule at the left of the result banner.
pub fn banner_rule(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| container::Style {
        background: Some(Background::Color(purple(&colors))),
        ..container::Style::default()
    }
}

/// A row of a list the user marks rows in: the cursor row is filled, a marked
/// one tinted.
pub fn marked_row(
    palette: &Palette,
    cursor: bool,
    marked: bool,
) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| {
        if cursor {
            container::Style {
                background: Some(Background::Color(colors.cursor)),
                text_color: Some(colors.cursor_text),
                ..container::Style::default()
            }
        } else if marked {
            container::Style {
                background: Some(Background::Color(colors.cursor.scale_alpha(MARKED_TINT))),
                text_color: Some(colors.text),
                ..container::Style::default()
            }
        } else {
            container::Style::default()
        }
    }
}

// --- splitter, section head, toolbar and menu (#366 panes) ---

/// One dot of the grip of a splitter.
pub fn splitter_dot(palette: &Palette, look: SplitterLook) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| {
        let ink = match look {
            SplitterLook::Idle => colors.dim_text,
            SplitterLook::Hovered | SplitterLook::Dragged => colors.cursor_text,
        };
        container::Style {
            background: Some(Background::Color(ink)),
            border: Border {
                radius: 1.5.into(),
                ..Border::default()
            },
            ..container::Style::default()
        }
    }
}

// --- commit rows of the graph (#366 refs) ---

/// A row of the commit list: the cursor row is filled; the row of the
/// checked-out commit is tinted in purple.
pub fn graph_row(
    palette: &Palette,
    chosen: bool,
    head: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    let plain = super::menu_item(palette, chosen);
    move |theme, status| {
        let mut style = plain(theme, status);
        if head && !chosen {
            style.background = Some(Background::Color(purple(&colors).scale_alpha(FILL_TINT)));
        }
        style
    }
}
