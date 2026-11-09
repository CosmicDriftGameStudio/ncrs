use crate::app::*;
use crate::messages::FavouritesBarMessage;

fn with_favourites(list: &[&str]) -> App {
    let mut app = App::with_fixed_panels();
    app.panels.favourites = list.iter().map(PathBuf::from).collect();
    drop(app.update(Message::NoticeExpired(0)));
    app
}

/// The bar is an input of every row count, and it counts again when it comes
/// or goes, not only when the window is resized.
#[test]
fn the_bar_takes_rows_when_it_appears_and_gives_them_back_when_it_goes() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::WindowResized(iced::Size::new(1200.0, 760.0))));
    let without = app.visible_rows;
    assert_eq!(without, layout::visible_rows(app.window_size, 0.0));
    drop(app.update(Message::OpenSearch));
    let search_without = app.search_rows_for_test();

    app.panels.favourites = vec![PathBuf::from("/nowhere/one")];
    drop(app.update(Message::NoticeExpired(0)));
    let bar = layout::FAVOURITES_BAR_HEIGHT;
    assert_eq!(app.visible_rows, layout::visible_rows(app.window_size, bar));
    assert!(app.visible_rows < without);
    assert!(app.search_rows_for_test() <= search_without);
    assert_eq!(
        app.trash_rows(),
        layout::trash_visible_rows(app.window_size, bar)
    );

    drop(app.update(Message::FavouritesBar(FavouritesBarMessage::Remove(0))));
    assert_eq!(app.visible_rows, without);
    assert_eq!(app.search_rows_for_test(), search_without);
}

fn assert_nothing_happens(app: &mut App, why: &str) {
    let before = app.panels.left.path.clone();
    drop(app.update(Message::OpenFavourite(0)));
    assert_eq!(app.panels.left.path, before, "{why}");
    assert!(app.job_error.is_none(), "{why}");
}

#[test]
fn the_keys_open_a_favourite_only_while_the_panels_have_them() {
    let mut app = with_favourites(&["/nowhere/one"]);

    // Control: with the panels free, the missing folder is reported.
    drop(app.update(Message::OpenFavourite(0)));
    assert!(app.job_error.is_some());
    app.job_error = None;

    drop(app.update(Message::CommandLine(CommandLineMessage::Focus)));
    assert_nothing_happens(&mut app, "the command line has the keys");
    drop(app.update(Message::CommandLine(CommandLineMessage::Leave)));

    app.start_over_dialog = true;
    assert_nothing_happens(&mut app, "a dialog is open");
    app.start_over_dialog = false;

    drop(app.update(Message::Settings(SettingsMessage::Toggle)));
    assert_nothing_happens(&mut app, "the settings tab is shown");
}

#[test]
fn a_number_without_a_favourite_does_nothing_and_says_nothing() {
    let mut app = with_favourites(&["/nowhere/one"]);
    for index in 1..5 {
        let before = app.panels.left.path.clone();
        drop(app.update(Message::OpenFavourite(index)));
        assert_eq!(app.panels.left.path, before);
        assert!(app.job_error.is_none());
        assert!(app.notice.is_none());
    }
}
