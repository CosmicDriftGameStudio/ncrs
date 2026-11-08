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
fn the_four_dialog_button_states_look_different_in_every_preset() {
    for preset in ThemePreset::ALL {
        let p = preset.palette();
        let theme = Theme::Dark;
        let look = |primary: bool, status: button::Status| {
            let style = dialog_button(&p, primary)(&theme, status);
            (style.background, style.text_color, style.border.color)
        };
        let states = [
            look(true, button::Status::Active),
            look(false, button::Status::Active),
            look(false, button::Status::Hovered),
            look(false, button::Status::Disabled),
        ];
        for (index, first) in states.iter().enumerate() {
            for second in states.iter().skip(index + 1) {
                assert_ne!(first, second, "{preset:?}");
            }
        }
        let outline = look(false, button::Status::Active).2;
        assert!(contrast_ratio(outline, p.dialog) >= 3.0, "{preset:?}");
    }
}

#[test]
fn the_default_preset_styles_the_window_as_it_always_looked() {
    let p = ThemePreset::Teal.palette();
    let theme = Theme::Dark;
    let background = rgb(0.035, 0.047, 0.098);
    let panel_fill = rgb(0.047, 0.086, 0.200);
    let text = rgb(0.800, 0.840, 0.940);
    let dark_text = rgb(0.020, 0.040, 0.090);
    let teal = rgb(0.110, 0.420, 0.520);
    let bar = rgb(0.070, 0.125, 0.270);
    let cursor = rgb(0.180, 0.690, 0.760);

    let style_root = root(&p)(&theme);
    assert_eq!(
        (fill_of(&style_root), style_root.text_color),
        (Some(background), Some(text))
    );
    let style_status = statusbar(&p)(&theme);
    assert_eq!(
        (fill_of(&style_status), style_status.text_color),
        (Some(bar), Some(text))
    );

    let style_panel = panel(&p)(&theme);
    assert_eq!(
        (fill_of(&style_panel), style_panel.text_color),
        (Some(panel_fill), Some(text))
    );

    let tab_status = button::Status::Active;
    let style_tab_active = title_tab(&p, true)(&theme, tab_status);
    assert_eq!(
        (row_fill(&style_tab_active), style_tab_active.text_color),
        (Some(panel_fill), text)
    );
    let style_tab_inactive = title_tab(&p, false)(&theme, tab_status);
    assert_eq!(
        (row_fill(&style_tab_inactive), style_tab_inactive.text_color),
        (None, rgb(0.450, 0.500, 0.640))
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

    assert_eq!(row_text_color(&p, false, true, false, true), dark_text);
    assert_eq!(
        row_text_color(&p, true, false, false, true),
        rgb(1.0, 1.0, 1.0)
    );
    assert_eq!(row_text_color(&p, false, false, false, true), text);
    assert_eq!(row_text_color(&p, false, true, false, false), text);

    let style_dialog_box = dialog(&p)(&theme);
    assert_eq!(
        (
            fill_of(&style_dialog_box),
            style_dialog_box.text_color,
            style_dialog_box.border.color
        ),
        (Some(teal), Some(text), p.border_inactive)
    );
    let style_menu_box = menu(&p)(&theme);
    assert_eq!(
        (
            fill_of(&style_menu_box),
            style_menu_box.text_color,
            style_menu_box.border.color
        ),
        (Some(bar), Some(text), p.border_inactive)
    );
}
