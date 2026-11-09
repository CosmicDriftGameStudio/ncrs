//! Snapshots of the question before a file that could run is opened.

use crate::app::*;

#[test]
fn the_question_matches_its_snapshots() {
    for (language, name) in [
        (Language::English, "open_file"),
        (Language::German, "open_file_de"),
    ] {
        let mut app = App::with_fixed_panels().in_language(language);
        app.open_open_file_dialog_for_test();
        let matches = iced_test::Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(1200.0, 760.0),
            app.view(),
        )
        .snapshot(&iced::Theme::Dark)
        .expect("the view renders")
        .matches_image(format!("tests/snapshots/{name}.png"))
        .expect("snapshot comparison");
        assert!(matches, "the question does not match {name}.png");
    }
}
