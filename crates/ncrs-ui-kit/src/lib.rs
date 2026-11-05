//! The building blocks every screen of NC-rs is made from: colors, sizes,
//! rows, buttons, inputs, menus and dialog frames. Generic over the message
//! type, so a feature crate maps them into its own messages.

/// Family name inside the embedded DejaVu Sans Mono files.
const BUNDLED_FAMILY: &str = "DejaVu Sans Mono";

/// The font of all text in the app: the window's default font and the base of
/// every weight variant, so a bold part never switches family. macOS keeps its
/// system monospace font (Menlo) because Menlo's license does not allow
/// shipping it; the other systems get the embedded DejaVu Sans Mono.
pub const APP_FONT: iced::Font = if cfg!(target_os = "macos") {
    iced::Font::MONOSPACE
} else {
    iced::Font::with_name(BUNDLED_FAMILY)
};

/// Font files the app must load for `APP_FONT`; empty on macOS.
pub const BUNDLED_FONTS: &[&[u8]] = if cfg!(target_os = "macos") {
    &[]
} else {
    &[BUNDLED_REGULAR, BUNDLED_BOLD]
};

const BUNDLED_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSansMono.ttf");
const BUNDLED_BOLD: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSansMono-Bold.ttf");

pub mod column_header;
pub mod file_row;
pub mod fkeys;
pub mod focus;
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
pub mod numbered_hint;
pub mod palette;
pub mod picker;
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
