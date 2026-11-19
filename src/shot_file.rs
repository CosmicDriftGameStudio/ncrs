//! Writes a view as a PNG file, for the generators of the README, store and
//! trailer pictures and the Hermes protocol.

use std::path::PathBuf;

use iced_test::Simulator;

use crate::app::App;
use crate::messages::Message;

/// `matches_image` writes a missing reference and appends the renderer's name
/// to the file name, so it renders into an empty directory and the single
/// file that appears there is copied to the wanted name.
pub fn write_screenshot_to(app: &App, directory: &str, size: iced::Size, file_name: &str) {
    write_view_to(app.view(), directory, size, file_name);
}

pub fn write_view_to(
    view: iced::Element<'_, Message>,
    directory: &str,
    size: iced::Size,
    file_name: &str,
) {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let mut simulator = Simulator::with_size(iced::Settings::default(), size, view);
    simulator
        .snapshot(&iced::Theme::Dark)
        .expect("the view should render")
        .matches_image(scratch.path().join("shot.png"))
        .expect("the screenshot should be written");
    let written = std::fs::read_dir(scratch.path())
        .expect("scratch directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|extension| extension == "png"))
        .expect("the renderer wrote no png");

    std::fs::create_dir_all(directory).expect("output directory");
    let target = PathBuf::from(directory).join(file_name);
    std::fs::copy(&written, &target).expect("copy the screenshot");
    assert!(target.exists(), "{} was not written", target.display());
}
