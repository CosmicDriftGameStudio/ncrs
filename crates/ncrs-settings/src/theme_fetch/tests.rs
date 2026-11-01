use super::fixtures::fetched;
use super::*;

fn typed(text: &str) -> ThemeFetch {
    let mut card = ThemeFetch::new();
    card.set_text(text.to_owned());
    card
}

#[test]
fn a_valid_id_starts_the_download_and_locks_the_field() {
    let mut card = typed("example.dark-themes");
    let id = card.start().unwrap();
    assert_eq!(id.to_string(), "example.dark-themes");
    assert_eq!(card.stage(), &Stage::Fetching);

    card.set_text("other.thing".to_owned());
    assert_eq!(
        card.text(),
        "example.dark-themes",
        "typing is ignored while it runs"
    );
    assert_eq!(card.start(), None, "no second download");
}

#[test]
fn an_invalid_text_starts_nothing_and_says_why() {
    let mut card = typed("../etc");
    assert_eq!(card.start(), None);
    assert_eq!(card.stage(), &Stage::Input);
    assert_eq!(card.error(), Some(&FetchError::InvalidId));

    card.set_text("example.dark-themes".to_owned());
    assert_eq!(card.error(), None, "typing clears the complaint");
}

#[test]
fn a_failed_download_returns_to_the_input_with_the_text_kept() {
    let mut card = typed("example.dark-themes");
    card.start();
    assert_eq!(card.finish_fetching(Err(FetchError::Timeout)), None);
    assert_eq!(card.stage(), &Stage::Input);
    assert_eq!(card.text(), "example.dark-themes");
    assert_eq!(card.error(), Some(&FetchError::Timeout));
    assert!(card.start().is_some(), "and it can be tried again");
}

#[test]
fn one_theme_is_installed_without_asking() {
    let mut card = typed("example.dark-themes");
    card.start();
    let (extension, theme) = card
        .finish_fetching(Ok(fetched(&["Example Dark"])))
        .unwrap();
    assert_eq!(extension.extension.display_name, "Example Themes");
    assert_eq!(theme.label, "Example Dark");
    assert_eq!(card.stage(), &Stage::Installing);
}

#[test]
fn several_themes_are_chosen_from_and_the_package_stays() {
    let mut card = typed("example.dark-themes");
    card.start();
    assert_eq!(
        card.finish_fetching(Ok(fetched(&["Example A", "Example B", "Example C"]))),
        None
    );
    assert!(matches!(card.stage(), Stage::Choosing { cursor: 0, .. }));

    card.move_cursor(1);
    card.move_cursor(1);
    card.move_cursor(1);
    assert!(
        matches!(card.stage(), Stage::Choosing { cursor: 2, .. }),
        "stops at the end"
    );
    card.move_cursor(-5);
    assert!(
        matches!(card.stage(), Stage::Choosing { cursor: 0, .. }),
        "and at the top"
    );

    card.select(1);
    card.select(9);
    let (extension, theme) = card.choose().unwrap();
    assert_eq!(theme.label, "Example B");
    assert_eq!(extension.themes.len(), 3, "the package comes along");
    assert_eq!(card.stage(), &Stage::Installing);
    assert_eq!(card.choose(), None);
}

#[test]
fn a_result_nobody_waits_for_changes_nothing() {
    let mut card = typed("example.dark-themes");
    assert_eq!(card.finish_fetching(Ok(fetched(&["Example Dark"]))), None);
    assert_eq!(card.stage(), &Stage::Input);
}

#[test]
fn a_failed_installation_returns_to_the_input() {
    let mut card = typed("example.dark-themes");
    card.start();
    card.finish_fetching(Ok(fetched(&["Example Dark"])));
    card.fail(FetchError::NoConfigPath);
    assert_eq!(card.stage(), &Stage::Input);
    assert_eq!(card.text(), "example.dark-themes");
    assert_eq!(card.error(), Some(&FetchError::NoConfigPath));
}
