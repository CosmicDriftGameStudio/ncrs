use super::*;
use crate::ui_tests::simulator;

#[test]
fn a_refused_name_shows_its_reason_under_the_input() -> Result<(), Box<dyn std::error::Error>> {
    let app = App::with_fixed_rename_refused();
    let matches = simulator(&app)
        .snapshot(&iced::Theme::Dark)?
        .matches_reference_image("tests/snapshots/rename_refused_en.png")?;
    if matches {
        Ok(())
    } else {
        Err("the view does not match tests/snapshots/rename_refused_en.png".into())
    }
}
