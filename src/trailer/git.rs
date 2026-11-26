//! The "git" scene of the trailer: the history of a made-up repository, then the
//! list of every repository, as the Mac App Store build shows them: read only.

use iced::keyboard::{key::Named, Key, Modifiers};
use ncrs_i18n::Language;

use super::{Focus, Recorder};
use crate::repos_scene::repos_tab;
use crate::screenshot_scenes::Scenes;
use crate::trailer_data::dense_showcase_app;

const WHOLE_TAB: Focus = Focus::new(0.0, 60.0, 1920.0, 1020.0);
const REPOS_PER_MARK: usize = 3;

pub(in crate::trailer) fn git(recorder: &mut Recorder) {
    recorder.begin_scene(
        "git",
        "Git, built in",
        "History, branches and changes · every repository",
    );
    let scenes = Scenes::new();
    recorder.show_app(
        dense_showcase_app()
            .showing_repos_for_test(repos_tab(Language::English))
            .in_sandbox_build_for_test()
            .showing_git_for_test(scenes.git_read_only()),
    );
    recorder.aim(WHOLE_TAB);
    recorder.frame(2.8);

    recorder.press(
        Key::Character("k".into()),
        Modifiers::COMMAND | Modifiers::SHIFT,
        1.2,
    );
    for _ in 0..REPOS_PER_MARK {
        recorder.plain(Named::ArrowDown, 0.3);
    }
    for _ in 0..REPOS_PER_MARK {
        recorder.press(Key::Named(Named::Space), Modifiers::default(), 0.4);
        recorder.plain(Named::ArrowDown, 0.3);
    }
    recorder.frame(1.6);
}
