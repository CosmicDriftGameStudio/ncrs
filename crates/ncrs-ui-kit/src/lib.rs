//! The building blocks every screen of NC-rs is made from: colors, sizes,
//! rows, buttons, inputs, menus and dialog frames. Generic over the message
//! type, so a feature crate maps them into its own messages.

/// Family name inside the embedded DejaVu Sans Mono files.
pub const BUNDLED_FAMILY: &str = "DejaVu Sans Mono";

/// The font of all text in the app: the window's default font and the base of
/// every weight variant, so a bold part never switches family. It is the
/// generic monospace font; `font::apply_font` decides the family behind it
/// (the system's font, the embedded DejaVu Sans Mono where the system has
/// none to ship, or the user's choice).
pub const APP_FONT: iced::Font = iced::Font::MONOSPACE;

/// Font files the app must load for `APP_FONT`; empty on macOS.
pub const BUNDLED_FONTS: &[&[u8]] = if cfg!(target_os = "macos") {
    &[]
} else {
    &[BUNDLED_REGULAR, BUNDLED_BOLD]
};

const BUNDLED_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSansMono.ttf");
const BUNDLED_BOLD: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSansMono-Bold.ttf");

pub mod badge_dot;
pub mod choice_rows;
pub mod column_header;
pub mod data_table;
pub mod file_icon;
pub mod file_row;
pub mod fkeys;
pub mod focus;
pub mod font;
pub mod format;
pub mod frame;
pub mod hermes_kit;
pub mod hermes_state;
pub mod key_intercept;
pub mod kit;
pub mod layout;
pub mod list_kit;
pub mod list_window;
mod menu_button;
pub mod meter;
pub mod nav_list;
pub mod numbered_hint;
pub mod open_list;
pub mod page_status_bar;
pub mod page_text;
pub mod palette;
pub mod panel_footer;
pub mod permission_grid;
pub mod picker;
pub mod popup_menu;
pub mod rename_row;
pub mod row_scroll;
pub mod setting_rows;
pub mod tags;
pub mod theme;

// A failing assertion in a test is the signal, so `unwrap` belongs here.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;
    use ttf_parser::{name_id, Face, GlyphId};

    fn face(bytes: &[u8]) -> Face<'_> {
        Face::parse(bytes, 0).unwrap()
    }

    fn family_name(face: &Face<'_>) -> String {
        face.names()
            .into_iter()
            .find(|name| name.name_id == name_id::FAMILY && name.is_unicode())
            .and_then(|name| name.to_string())
            .unwrap()
    }

    fn advance_of_m(face: &Face<'_>) -> u16 {
        let glyph: GlyphId = face.glyph_index('M').unwrap();
        face.glyph_hor_advance(glyph).unwrap()
    }

    #[test]
    fn the_embedded_files_are_the_family_app_font_names_in_regular_and_bold() {
        let regular = face(BUNDLED_REGULAR);
        let bold = face(BUNDLED_BOLD);
        assert_eq!(family_name(&regular), BUNDLED_FAMILY);
        assert_eq!(family_name(&bold), BUNDLED_FAMILY);
        assert!(!regular.is_bold());
        assert!(bold.is_bold());
        assert_eq!(advance_of_m(&regular), advance_of_m(&bold));
        assert!(regular.is_monospaced() && bold.is_monospaced());
    }
}
