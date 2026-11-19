use super::*;

#[test]
fn a_move_inside_an_archive_says_the_archive_is_read_only() {
    for (lang, name) in [
        (Language::English, "archive_read_only_en"),
        (Language::German, "archive_read_only_de"),
    ] {
        let mut app = App::with_fixed_panels().in_language(lang);
        app.show_archive_read_only_for_test();
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_reference_image(format!("tests/snapshots/{name}.png"))
            .expect("snapshot comparison");
        assert!(matches, "the panel does not match {name}.png");
    }
}
