//! Colors, spacing and reusable widget styles (dark, Norton Commander inspired).

use iced::widget::{button, checkbox, container};
use iced::{Background, Border, Color, Theme};

pub mod colors {
    use iced::Color;

    pub const BACKGROUND: Color = Color::from_rgb(0.035, 0.047, 0.098);
    pub const PANEL_BACKGROUND: Color = Color::from_rgb(0.047, 0.086, 0.200);
    pub const PANEL_TITLE_BG: Color = Color::from_rgb(0.070, 0.125, 0.270);
    pub const PANEL_TITLE_ACTIVE_BG: Color = Color::from_rgb(0.110, 0.420, 0.520);
    pub const HEADER_BACKGROUND: Color = Color::from_rgb(0.110, 0.420, 0.520);
    pub const STATUSBAR_BACKGROUND: Color = Color::from_rgb(0.070, 0.125, 0.270);

    pub const SELECTED_BG: Color = Color::from_rgb(0.180, 0.690, 0.760);
    pub const SELECTED_INACTIVE_BG: Color = Color::from_rgb(0.130, 0.210, 0.360);
    pub const SELECTED_TEXT: Color = Color::from_rgb(0.020, 0.040, 0.090);

    pub const ACTIVE_BORDER: Color = Color::from_rgb(0.310, 0.840, 0.910);
    pub const INACTIVE_BORDER: Color = Color::from_rgb(0.165, 0.210, 0.390);

    pub const TEXT: Color = Color::from_rgb(0.800, 0.840, 0.940);
    pub const DISABLED_TEXT: Color = Color::from_rgb(0.330, 0.380, 0.520);
    pub const DIM_TEXT: Color = Color::from_rgb(0.450, 0.500, 0.640);
    pub const DIR_COLOR: Color = Color::from_rgb(1.000, 1.000, 1.000);
    pub const ACCENT: Color = Color::from_rgb(1.000, 0.850, 0.300);
    pub const ERROR: Color = Color::from_rgb(1.000, 0.450, 0.450);

    /// The digit of a function key slot: plain light text on the window
    /// background, as in Norton Commander.
    pub const FKEY_NUMBER: Color = Color::from_rgb(0.850, 0.870, 0.920);
    /// The label box behind a function key's name, with dark text on it.
    pub const FKEY_LABEL_BG: Color = Color::from_rgb(0.180, 0.690, 0.760);
    pub const FKEY_LABEL_TEXT: Color = Color::from_rgb(0.020, 0.040, 0.090);
    /// Behind a modal dialog: darkens the panels without hiding them.
    pub const SCRIM: Color = Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.65,
    };
}

pub mod spacing {
    pub const OUTER_PADDING: f32 = 6.0;
    pub const SECTION_GAP: f32 = 6.0;
    pub const PANEL_GAP: f32 = 6.0;
    pub const CELL_PADDING_X: f32 = 8.0;
    pub const BORDER_WIDTH: f32 = 1.0;
    pub const BORDER_RADIUS: f32 = 3.0;
}

pub mod font_size {
    pub const HEADER: f32 = 14.0;
    pub const TITLE: f32 = 14.0;
    pub const ROW: f32 = 14.0;
    pub const COLUMN_HEADER: f32 = 12.0;
    pub const STATUS: f32 = 13.0;
}

/// Base theme handed to iced (our widgets override most colors).
pub fn app_theme() -> Theme {
    Theme::Dark
}

fn filled(bg: Color, text: Color) -> container::Style {
    container::Style {
        background: Some(Background::Color(bg)),
        text_color: Some(text),
        ..container::Style::default()
    }
}

pub fn root(_theme: &Theme) -> container::Style {
    filled(colors::BACKGROUND, colors::TEXT)
}

pub fn header(_theme: &Theme) -> container::Style {
    filled(colors::HEADER_BACKGROUND, colors::SELECTED_TEXT)
}

/// The label box of a function key slot, filled or empty.
pub fn fkey_label(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(colors::FKEY_LABEL_BG)),
        text_color: Some(colors::FKEY_LABEL_TEXT),
        ..container::Style::default()
    }
}

pub fn statusbar(_theme: &Theme) -> container::Style {
    filled(colors::STATUSBAR_BACKGROUND, colors::TEXT)
}

/// Outer frame of a file panel; highlighted border when active.
pub fn panel(active: bool) -> impl Fn(&Theme) -> container::Style {
    move |_theme| container::Style {
        border: Border {
            color: if active {
                colors::ACTIVE_BORDER
            } else {
                colors::INACTIVE_BORDER
            },
            width: spacing::BORDER_WIDTH,
            radius: spacing::BORDER_RADIUS.into(),
        },
        ..filled(colors::PANEL_BACKGROUND, colors::TEXT)
    }
}

pub fn panel_title(active: bool) -> impl Fn(&Theme) -> container::Style {
    move |_theme| {
        if active {
            filled(colors::PANEL_TITLE_ACTIVE_BG, colors::SELECTED_TEXT)
        } else {
            filled(colors::PANEL_TITLE_BG, colors::DIM_TEXT)
        }
    }
}

/// Background of a file row (flat button, no hover chrome).
pub fn row(selected: bool, panel_active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, _status| {
        let background = match (selected, panel_active) {
            (true, true) => Some(Background::Color(colors::SELECTED_BG)),
            (true, false) => Some(Background::Color(colors::SELECTED_INACTIVE_BG)),
            _ => None,
        };
        button::Style {
            background,
            text_color: colors::TEXT,
            border: Border::default(),
            ..button::Style::default()
        }
    }
}

/// Text color for a row depending on entry kind and selection state.
pub fn row_text_color(is_dir: bool, selected: bool, panel_active: bool) -> Color {
    match (selected && panel_active, is_dir) {
        (true, _) => colors::SELECTED_TEXT,
        (false, true) => colors::DIR_COLOR,
        (false, false) => colors::TEXT,
    }
}

/// The prompt panel: a raised box over the scrim.
pub fn dialog(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(colors::PANEL_TITLE_ACTIVE_BG)),
        text_color: Some(colors::TEXT),
        border: Border {
            color: colors::ACTIVE_BORDER,
            width: 2.0,
            radius: spacing::BORDER_RADIUS.into(),
        },
        ..container::Style::default()
    }
}

/// Buttons in the prompt. `primary` is the action the user most likely wants.
pub fn dialog_button(primary: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, _status| {
        let background = primary.then_some(Background::Color(colors::SELECTED_BG));
        button::Style {
            background,
            text_color: if primary {
                colors::SELECTED_TEXT
            } else {
                colors::DIM_TEXT
            },
            border: Border {
                color: if primary {
                    colors::ACTIVE_BORDER
                } else {
                    colors::INACTIVE_BORDER
                },
                width: 1.0,
                radius: spacing::BORDER_RADIUS.into(),
            },
            ..button::Style::default()
        }
    }
}

/// The star in front of a row. Flat, so it does not look like a button until it
/// is one: a tagged row is the only one with a visible mark.
pub fn tag_button(
    tagged: bool,
    _panel_active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, _status| button::Style {
        background: None,
        // Tagged rows are accent-coloured, an untagged star is dim. Which panel
        // is active does not change this — the cursor highlight elsewhere says
        // that.
        text_color: if tagged {
            colors::ACCENT
        } else {
            colors::DIM_TEXT
        },
        border: Border::default(),
        ..button::Style::default()
    }
}

/// The "for all files" checkbox in the conflict dialog.
pub fn dialog_checkbox(_theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    let checked = match status {
        checkbox::Status::Active { is_checked } | checkbox::Status::Hovered { is_checked } => {
            is_checked
        }
        checkbox::Status::Disabled { is_checked } => is_checked,
    };
    checkbox::Style {
        background: Background::Color(if checked {
            colors::SELECTED_BG
        } else {
            colors::PANEL_BACKGROUND
        }),
        icon_color: colors::SELECTED_TEXT,
        border: Border {
            color: colors::ACTIVE_BORDER,
            width: 1.0,
            radius: spacing::BORDER_RADIUS.into(),
        },
        text_color: Some(colors::TEXT),
    }
}

/// One entry of the context menu: flat, the highlighted one filled like the
/// cursor row of a panel.
pub fn menu_item(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, _status| button::Style {
        background: selected.then_some(Background::Color(colors::SELECTED_BG)),
        text_color: colors::TEXT,
        border: Border::default(),
        ..button::Style::default()
    }
}

/// The line between two groups of the context menu.
pub fn menu_separator(_theme: &Theme) -> container::Style {
    filled(colors::ACTIVE_BORDER, colors::TEXT)
}
