//! The program hands the bundled fonts to iced; `APP_FONT` asks for a family
//! that only exists there off macOS.

use std::path::PathBuf;

use super::*;

#[test]
fn the_program_loads_exactly_the_bundled_fonts() {
    let application = crate::program(crate::ui::layout::INITIAL_WINDOW_SIZE, || {
        App::starting_in(PathBuf::from("/"), PathBuf::from("/"))
    });

    let settings = iced::Program::settings(&application);
    let loaded: Vec<&[u8]> = settings.fonts.iter().map(|font| font.as_ref()).collect();

    assert_eq!(loaded, ncrs_ui_kit::BUNDLED_FONTS);
    assert_eq!(
        loaded.is_empty(),
        cfg!(target_os = "macos"),
        "only macOS takes the system's font"
    );
}
