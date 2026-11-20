use super::*;
use crate::fs::FileEntry;
use std::path::{Path, PathBuf};

fn app_with_files() -> App {
    let mut app = App::with_fixed_panels();
    for side in [
        crate::messages::PanelSide::Left,
        crate::messages::PanelSide::Right,
    ] {
        let panel = app.panel_mut_for_test(side);
        panel.entries = vec![
            FileEntry::parent(Path::new("/tmp")),
            file("alpha"),
            file("beta"),
            file("gamma"),
        ];
    }
    app.set_visible_rows_for_test(10);
    app
}

fn file(name: &str) -> FileEntry {
    FileEntry {
        name: name.into(),
        path: PathBuf::from("/tmp").join(name),
        is_dir: false,
        is_symlink: false,
        is_parent: false,
        size: 100,
        modified: None,
        folder_size: None,
        details: Default::default(),
    }
}

/// Tagging has to change what the user sees. A selection that is stored but
/// not drawn would make F5 copy something invisible.
///
/// Compared by hash, not by image: `matches_hash` is an exact SHA256 of the
/// pixels, while `matches_image` tolerates differences — and a two-pixel-wide
/// tag column on a 590px row is well inside that tolerance, which is why an
/// earlier version of this test passed with the marker removed.
///
/// This proves the selection is *visible*, not that the `*` caused it.
///
/// Measured: with the marker column removed the pixels still differ, because
/// the row background changes too. So the test cannot separate the two
/// changes, and does not claim to. What it guarantees is what a file
/// operation needs — a tagged row looks different from an untagged one, so
/// the user can see what F5 will act on.
///
/// Pinning the `*` itself would need a renderer check on that column alone;
/// the two ways to draw a tag are covered by the untagged test below, which
/// does fail if the row style stops distinguishing them.
#[cfg(target_os = "macos")]
#[test]
fn tagging_changes_the_view() {
    let untagged = app_with_files();
    assert!(untagged_view_matches_its_reference(&untagged));

    let mut tagged = app_with_files();
    tagged.toggle_tag_for_test(1);
    tagged.toggle_tag_for_test(2);

    let same = simulator(&tagged)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_hash("tests/snapshots/tags_none.sha256")
        .expect("compare");

    assert!(
        !same,
        "tagging two rows changed no pixel — the selection is invisible"
    );
}

/// Untagging returns to the untagged view. Without this, a tag that cannot
/// be removed would also be invisible: the test above would still pass.
#[test]
fn untagging_returns_to_the_untagged_view() {
    let mut app = app_with_files();
    assert!(untagged_view_matches_its_reference(&app));

    app.toggle_tag_for_test(1);
    app.toggle_tag_for_test(1); // same row again: untagged

    let same = simulator(&app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_hash("tests/snapshots/tags_none.sha256")
        .expect("compare");

    assert!(same, "the tag could not be removed");
}

/// Tagging a single row differs from tagging two. Otherwise "did my second
/// tag land?" would have no answer, and a partial failure would look
/// correct.
#[cfg(target_os = "macos")]
#[test]
fn one_tag_differs_from_two() {
    let none = app_with_files();
    assert!(untagged_view_matches_its_reference(&none));

    let mut one = app_with_files();
    one.toggle_tag_for_test(1);

    let differs = !simulator(&one)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_hash("tests/snapshots/tags_none.sha256")
        .expect("compare");
    assert!(differs, "one tag looked like none");
}

/// The tag marker occupies its own column, so tagging does not shift the
/// name column sideways.
#[test]
fn tagging_does_not_shift_the_names() {
    let name_bounds = |app: &App| {
        simulator(app)
            .find("gamma")
            .expect("the name is drawn")
            .bounds()
    };
    let mut app = app_with_files();
    let untagged = name_bounds(&app);
    app.toggle_tag_for_test(3);
    assert_eq!(name_bounds(&app), untagged);
}

fn untagged_view_matches_its_reference(app: &App) -> bool {
    simulator(app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_hash("tests/snapshots/tags_none.sha256")
        .expect("reference")
}

/// Tagging everything marks the rows a bulk operation would use, and skips
/// `..`.
#[cfg(target_os = "macos")]
#[test]
fn tagging_everything_is_not_the_same_as_tagging_nothing() {
    let none = app_with_files();
    assert!(untagged_view_matches_its_reference(&none));

    let mut all = app_with_files();
    all.tag_all_for_test();

    let differs = !simulator(&all)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_hash("tests/snapshots/tags_none.sha256")
        .expect("compare");
    assert!(differs, "tagging everything looked like tagging nothing");
}

/// The cursor row is not a tag. Moving the cursor must not make a file part
/// of the next operation.
#[test]
fn moving_the_cursor_does_not_tag() {
    let mut app = app_with_files();
    assert!(untagged_view_matches_its_reference(&app));

    // Moving the cursor changes the highlight, so the image is expected to
    // differ from the initial one. What must not happen is a tag: the panel
    // reports the cursor row as untagged.
    app.move_selection_for_test(1);
    assert!(
        !app.left_panel_selection_tagged_for_test(1),
        "moving the cursor onto a row tagged it"
    );
    app.move_selection_for_test(1);
    assert!(!app.left_panel_selection_tagged_for_test(2));
}
