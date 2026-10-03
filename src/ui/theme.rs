//! Colors, spacing and reusable widget styles (dark, Norton Commander inspired).

use iced::widget::{button, checkbox, container, text_input};
use iced::{Background, Border, Color, Theme};

use crate::palette::Palette;

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

pub fn root(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| filled(colors.background, colors.text)
}

pub fn header(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| filled(colors.header, colors.cursor_text)
}

/// The label box of a function key slot, filled or empty.
pub fn fkey_label(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| container::Style {
        background: Some(Background::Color(colors.fkey_label)),
        text_color: Some(colors.fkey_label_text),
        ..container::Style::default()
    }
}

pub fn statusbar(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| filled(colors.status_bar, colors.text)
}

/// Outer frame of a file panel; highlighted border when active.
pub fn panel(palette: &Palette, active: bool) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| container::Style {
        border: Border {
            color: if active {
                colors.border
            } else {
                colors.border_inactive
            },
            width: spacing::BORDER_WIDTH,
            radius: spacing::BORDER_RADIUS.into(),
        },
        ..filled(colors.panel, colors.text)
    }
}

pub fn panel_title(palette: &Palette, active: bool) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| {
        if active {
            filled(colors.title, colors.cursor_text)
        } else {
            filled(colors.title_inactive, colors.dim_text)
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
        button::Style {
            background,
            text_color: colors.text,
            border: Border::default(),
            ..button::Style::default()
        }
    }
}

/// Text color for a row depending on entry kind and selection state.
pub fn row_text_color(colors: &Palette, is_dir: bool, selected: bool, panel_active: bool) -> Color {
    match (selected && panel_active, is_dir) {
        (true, _) => colors.cursor_text,
        (false, true) => colors.directory,
        (false, false) => colors.text,
    }
}

/// The prompt panel: a raised box over the scrim.
pub fn dialog(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| container::Style {
        background: Some(Background::Color(colors.dialog)),
        text_color: Some(colors.text),
        border: Border {
            color: colors.border,
            width: 2.0,
            radius: spacing::BORDER_RADIUS.into(),
        },
        ..container::Style::default()
    }
}

/// Buttons in the prompt. `primary` is the action the user most likely wants.
pub fn dialog_button(
    palette: &Palette,
    primary: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    move |_theme, _status| {
        let background = primary.then_some(Background::Color(colors.cursor));
        button::Style {
            background,
            text_color: if primary {
                colors.cursor_text
            } else {
                colors.dim_text
            },
            border: Border {
                color: if primary {
                    colors.border
                } else {
                    colors.border_inactive
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
    palette: &Palette,
    tagged: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    // Tagged rows are accent-coloured, an untagged star is dim. Which panel is
    // active does not change this — the cursor highlight elsewhere says that.
    move |_theme, _status| button::Style {
        background: None,
        text_color: if tagged {
            colors.accent
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
    move |_theme, status| {
        let checked = match status {
            checkbox::Status::Active { is_checked }
            | checkbox::Status::Hovered { is_checked }
            | checkbox::Status::Disabled { is_checked } => is_checked,
        };
        checkbox::Style {
            background: Background::Color(if checked { colors.cursor } else { colors.panel }),
            icon_color: colors.cursor_text,
            border: Border {
                color: colors.border,
                width: 1.0,
                radius: spacing::BORDER_RADIUS.into(),
            },
            text_color: Some(colors.text),
        }
    }
}

/// The box of the context menu.
pub fn menu(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    let dialog = dialog(&colors);
    move |theme| container::Style {
        background: Some(Background::Color(colors.menu)),
        ..dialog(theme)
    }
}

/// One entry of the context menu: flat, the highlighted one filled like the
/// cursor row of a panel.
pub fn menu_item(
    palette: &Palette,
    selected: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    move |_theme, _status| button::Style {
        background: selected.then_some(Background::Color(colors.cursor)),
        text_color: colors.text,
        border: Border::default(),
        ..button::Style::default()
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
        icon: colors.accent,
        placeholder: colors.dim_text,
        value: colors.accent,
        selection: colors.cursor,
    }
}

/// A flat icon button; lit while the pointer is over it.
pub fn icon_button(palette: &Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hovered.then_some(Background::Color(colors.title)),
            text_color: colors.accent,
            border: Border {
                radius: spacing::BORDER_RADIUS.into(),
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
            color: colors.accent,
            width: spacing::BORDER_WIDTH,
            radius: 1.0.into(),
        },
        ..container::Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::{contrast_ratio, ThemePreset};

    fn rgb(r: f32, g: f32, b: f32) -> Color {
        Color::from_rgb(r, g, b)
    }

    fn fill_of(style: &container::Style) -> Option<Color> {
        match style.background {
            Some(Background::Color(color)) => Some(color),
            _ => None,
        }
    }

    fn row_fill(style: &button::Style) -> Option<Color> {
        match style.background {
            Some(Background::Color(color)) => Some(color),
            _ => None,
        }
    }

    #[test]
    fn menu_shortcuts_are_legible_and_quieter_than_labels() {
        for preset in ThemePreset::ALL {
            let p = preset.palette();
            assert!(contrast_ratio(p.menu_shortcut, p.menu) >= 4.5, "{preset:?}");
            assert!(
                contrast_ratio(p.menu_shortcut_on_cursor, p.cursor) >= 4.5,
                "{preset:?}"
            );
            assert!(
                contrast_ratio(p.menu_shortcut, p.menu) < contrast_ratio(p.text, p.menu),
                "{preset:?}"
            );
        }
    }

    #[test]
    fn disabled_menu_entries_are_legible_but_dimmer_than_shortcuts() {
        for preset in ThemePreset::ALL {
            let p = preset.palette();
            let disabled = contrast_ratio(p.menu_disabled, p.menu);
            assert!(disabled >= 3.0, "{preset:?}");
            assert!(
                disabled < contrast_ratio(p.menu_shortcut, p.menu),
                "{preset:?}"
            );
        }
    }

    #[test]
    fn the_default_palette_styles_the_window_as_it_always_looked() {
        let p = Palette::default();
        let theme = Theme::Dark;
        let background = rgb(0.035, 0.047, 0.098);
        let panel_fill = rgb(0.047, 0.086, 0.200);
        let text = rgb(0.800, 0.840, 0.940);
        let dark_text = rgb(0.020, 0.040, 0.090);
        let teal = rgb(0.110, 0.420, 0.520);
        let bar = rgb(0.070, 0.125, 0.270);
        let cursor = rgb(0.180, 0.690, 0.760);
        let border = rgb(0.310, 0.840, 0.910);
        let border_inactive = rgb(0.165, 0.210, 0.390);

        let style_root = root(&p)(&theme);
        assert_eq!(
            (fill_of(&style_root), style_root.text_color),
            (Some(background), Some(text))
        );
        let style_header_bar = header(&p)(&theme);
        assert_eq!(
            (fill_of(&style_header_bar), style_header_bar.text_color),
            (Some(teal), Some(dark_text))
        );
        let style_fkey = fkey_label(&p)(&theme);
        assert_eq!(
            (fill_of(&style_fkey), style_fkey.text_color),
            (Some(cursor), Some(dark_text))
        );
        let style_status = statusbar(&p)(&theme);
        assert_eq!(
            (fill_of(&style_status), style_status.text_color),
            (Some(bar), Some(text))
        );

        let style_panel_active = panel(&p, true)(&theme);
        assert_eq!(
            (
                fill_of(&style_panel_active),
                style_panel_active.text_color,
                style_panel_active.border.color
            ),
            (Some(panel_fill), Some(text), border)
        );
        let style_panel_inactive = panel(&p, false)(&theme);
        assert_eq!(style_panel_inactive.border.color, border_inactive);

        let style_title_active = panel_title(&p, true)(&theme);
        assert_eq!(
            (fill_of(&style_title_active), style_title_active.text_color),
            (Some(teal), Some(dark_text))
        );
        let style_title_inactive = panel_title(&p, false)(&theme);
        assert_eq!(
            (
                fill_of(&style_title_inactive),
                style_title_inactive.text_color
            ),
            (Some(bar), Some(rgb(0.450, 0.500, 0.640)))
        );

        let status = button::Status::Active;
        let inactive_tint = rgb(0.130, 0.210, 0.360);
        assert_eq!(
            row_fill(&row(&p, false, true, true)(&theme, status)),
            Some(cursor)
        );
        assert_eq!(
            row_fill(&row(&p, true, false, true)(&theme, status)),
            Some(cursor)
        );
        assert_eq!(
            row_fill(&row(&p, true, true, true)(&theme, status)),
            Some(cursor)
        );
        assert_eq!(
            row_fill(&row(&p, true, false, false)(&theme, status)),
            Some(inactive_tint)
        );
        assert_eq!(row_fill(&row(&p, false, true, false)(&theme, status)), None);
        assert_eq!(row_fill(&row(&p, false, false, true)(&theme, status)), None);
        assert_eq!(row(&p, false, false, true)(&theme, status).text_color, text);

        assert_eq!(row_text_color(&p, false, true, true), dark_text);
        assert_eq!(row_text_color(&p, true, false, true), rgb(1.0, 1.0, 1.0));
        assert_eq!(row_text_color(&p, false, false, true), text);
        assert_eq!(row_text_color(&p, false, true, false), text);

        let style_dialog_box = dialog(&p)(&theme);
        assert_eq!(
            (
                fill_of(&style_dialog_box),
                style_dialog_box.text_color,
                style_dialog_box.border.color
            ),
            (Some(teal), Some(text), border)
        );
        let style_menu_box = menu(&p)(&theme);
        assert_eq!(
            (
                fill_of(&style_menu_box),
                style_menu_box.text_color,
                style_menu_box.border.color
            ),
            (Some(bar), Some(text), border)
        );
    }
}
