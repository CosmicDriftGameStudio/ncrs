//! The folder row of the search view: the button that asks for a folder, the
//! title that follows it, and the two empty states that name the folder.

use iced_test::futures::futures::StreamExt as _;
use iced_test::runtime::{task, Action};
use iced_test::Simulator;

use crate::app::*;
use crate::i18n::Language;

const WINDOW: iced::Size = iced::Size::new(1200.0, 760.0);
const LANGUAGES: [(Language, &str); 2] = [(Language::German, "de"), (Language::English, "en")];

fn matches_snapshot(app: &App, name: &str) {
    let matches = Simulator::with_size(
        iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        },
        WINDOW,
        app.view(),
    )
    .snapshot(&iced::Theme::Dark)
    .unwrap()
    .matches_image(format!("tests/snapshots/{name}.png"))
    .expect("snapshot comparison");
    assert!(
        matches,
        "the view does not match tests/snapshots/{name}.png"
    );
}

fn answers_with_the_temp_dir(_: &Path) -> Task<Option<PathBuf>> {
    Task::done(Some(std::env::temp_dir()))
}

#[tokio::test]
async fn the_choose_button_leads_to_the_picked_folder_and_the_title_follows() {
    let mut app = App::with_fixed_panels();
    app.pick_folder = answers_with_the_temp_dir;
    drop(app.update(Message::OpenSearch));

    let mut stream = task::into_stream(app.update(Message::Search(SearchMessage::ChooseFolder)))
        .expect("the button starts the dialog");
    let mut answers = Vec::new();
    while let Some(action) = stream.next().await {
        if let Action::Output(message) = action {
            answers.push(message);
        }
    }
    assert_eq!(
        answers,
        [Message::Search(SearchMessage::FolderPicked(Some(
            std::env::temp_dir()
        )))]
    );

    for answer in answers {
        drop(app.update(answer));
    }
    let view = app.search_for_test().unwrap();
    assert_eq!(view.root, std::env::temp_dir());
    let title = ncrs_search::title(view, Language::English);
    assert!(
        title.contains(&std::env::temp_dir().display().to_string()),
        "{title}"
    );
}

#[test]
fn a_missing_folder_says_so_instead_of_searching() {
    for (language, suffix) in LANGUAGES {
        let mut app = App::with_fixed_panels().in_language(language);
        drop(app.update(Message::OpenSearch));
        drop(app.update(Message::Search(SearchMessage::FolderChanged(
            "/home/test/projects/nowhere".to_owned(),
        ))));
        assert!(app.search_for_test().unwrap().folder_missing);
        matches_snapshot(&app, &format!("search_folder_missing_{suffix}"));
    }
}

#[test]
fn an_empty_result_names_the_folder_and_what_is_left_out() {
    for (language, suffix) in LANGUAGES {
        let mut app = App::with_fixed_panels().in_language(language);
        app.show_search_results_for_test("hello", Vec::new());
        matches_snapshot(&app, &format!("search_no_results_{suffix}"));
    }
}
