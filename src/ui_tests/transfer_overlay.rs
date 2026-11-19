use super::*;

fn app_with(open: impl FnOnce(&mut App), lang: Language) -> App {
    let mut app = App::with_fixed_panels();
    open(&mut app);
    app.in_language(lang)
}

fn assert_snapshot(app: &App, name: &str) {
    let matches = simulator(app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image(format!("tests/snapshots/{name}.png"))
        .expect("snapshot comparison");
    assert!(matches, "the dialog does not match {name}.png");
}

#[test]
fn the_confirmation_of_a_few_entries_lists_their_names() {
    for (lang, name) in [
        (Language::English, "copy_confirm_few_en"),
        (Language::German, "copy_confirm_few_de"),
    ] {
        assert_snapshot(
            &app_with(|a| a.open_transfer_dialog_for_test(4), lang),
            name,
        );
    }
}

#[test]
fn the_confirmation_of_many_entries_sums_them_up() {
    for (lang, name) in [
        (Language::English, "copy_confirm_many_en"),
        (Language::German, "copy_confirm_many_de"),
    ] {
        assert_snapshot(
            &app_with(|a| a.open_transfer_dialog_for_test(36), lang),
            name,
        );
    }
}

#[test]
fn the_progress_shows_how_far_the_copy_is() {
    for (lang, name) in [
        (Language::English, "copy_progress_en"),
        (Language::German, "copy_progress_de"),
    ] {
        assert_snapshot(&app_with(|a| a.show_progress_for_test(37, 120), lang), name);
    }
}
