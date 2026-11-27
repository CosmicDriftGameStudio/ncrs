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

/// [`APP_FONT`] in bold: headings and the sorted column.
pub const APP_FONT_BOLD: iced::Font = iced::Font {
    weight: iced::font::Weight::Bold,
    ..APP_FONT
};

/// Font files the app must load for `APP_FONT`; empty on macOS.
pub const BUNDLED_FONTS: &[&[u8]] = if cfg!(target_os = "macos") {
    &[]
} else {
    &[BUNDLED_REGULAR, BUNDLED_BOLD]
};

const BUNDLED_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSansMono.ttf");
const BUNDLED_BOLD: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSansMono-Bold.ttf");

pub mod action_rows;
pub mod badge_dot;
pub mod blame_rows;
pub mod check_rows;
pub mod choice_rows;
pub mod color_chip;
pub mod column_header;
pub mod commit_graph;
pub mod compare_bar;
pub mod compare_cell;
pub mod confirm;
pub mod credential_dialog;
pub mod data_table;
pub mod day_bars;
pub mod favourites_bar;
pub mod file_icon;
pub mod file_row;
pub mod fkeys;
pub mod focus;
pub mod font;
pub mod format;
pub mod frame;
#[cfg(test)]
mod gallery_tests;
pub mod git_lines;
pub mod git_mark;
pub mod git_rows;
pub mod git_state;
pub mod header_picker;
pub mod hermes_kit;
pub mod hermes_state;
pub mod io_reason;
pub mod key_intercept;
pub mod kit;
#[cfg(test)]
mod kit_gallery;
pub mod layout;
pub mod list_below;
pub mod list_kit;
pub mod list_window;
pub mod markdown_view;
pub mod marked_list;
mod menu_button;
pub mod meter;
pub mod nav_list;
pub mod numbered_hint;
pub mod open_list;
pub mod page_scroll;
pub mod page_status_bar;
pub mod page_text;
pub use ncrs_core::palette;
pub mod panel_footer;
pub mod permission_grid;
pub mod picker;
pub mod plan_rows;
pub mod popup_menu;
pub mod rename_row;
pub mod repo_row;
pub mod row_scroll;
pub mod section_head;
pub mod setting_rows;
pub mod size_row;
#[cfg(any(test, feature = "test-support"))]
pub mod snapshot_support;
pub mod split_button;
pub mod splitter;
pub mod stacked_bar;
pub use ncrs_core::tags;
pub mod theme;
pub mod tree_list;

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

    /// The signs the Git views draw as text. The bundled font draws them on
    /// Linux and Windows, where there is no system fallback. A sign it lacks
    /// is an icon: the branch symbol (`file_icon::git_branch_icon`), the pause
    /// and the cross (`Icon::CirclePause`, `Icon::CircleX`; color emoji on a
    /// Mac) and the star (`Icon::Star`).
    const TEXT_SIGNS: [char; 9] = [
        '\u{2713}', '\u{2193}', '\u{2191}', '\u{25cf}', '!', '\u{203a}', '\u{25be}', '\u{25b8}',
        '\u{25a2}',
    ];

    #[test]
    fn the_bundled_font_has_every_sign_the_git_views_draw_as_text() {
        for (name, bytes) in [("regular", BUNDLED_REGULAR), ("bold", BUNDLED_BOLD)] {
            let font = face(bytes);
            for sign in TEXT_SIGNS {
                assert!(
                    font.glyph_index(sign).is_some(),
                    "{sign} is missing in the {name} font"
                );
            }
        }
    }
}
