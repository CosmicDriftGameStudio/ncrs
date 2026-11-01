use crate::app::*;
use crate::fs::FileEntry;
use std::ffi::OsString;

fn panel_with(names: &[&str]) -> PanelState {
    let mut p = PanelState::new(PathBuf::from("/left"));
    p.entries = names
        .iter()
        .map(|n| FileEntry {
            name: OsString::from(*n),
            path: PathBuf::from("/left").join(n),
            is_dir: false,
            is_symlink: false,
            is_parent: false,
            size: 1,
            modified: None,
            folder_size: None,
        })
        .collect();
    p
}

/// Regression: a reload put the cursor back on row 0, so after a file
/// operation the list jumped to the top and the tagged rows left the screen.
/// The row that was selected stays selected.
#[test]
fn a_reload_keeps_the_row_under_the_cursor() {
    let mut p = panel_with(&["a", "b", "c", "d"]);
    p.selected = 2;

    p.apply_listing(
        PathBuf::from("/left"),
        panel_with(&["a", "b", "c", "d"]).entries,
        None,
        None,
        10,
    );

    assert_eq!(p.selected, 2, "the cursor jumped to the top of the list");
}

/// And it survives the reload that adds a file in front, which shifts every
/// position — the name is what has to be kept, not the index.
#[test]
fn a_reload_keeps_the_file_not_the_index() {
    let mut p = panel_with(&["a", "b", "c"]);
    p.selected = 1; // b

    p.apply_listing(
        PathBuf::from("/left"),
        panel_with(&["new", "a", "b", "c"]).entries,
        None,
        None,
        10,
    );

    assert_eq!(
        p.entries[p.selected].name.to_string_lossy(),
        "b",
        "the cursor stayed on the index and now points at a different file"
    );
}

/// A name the caller asked for still wins over the remembered one — F7
/// selects the directory it just created.
#[test]
fn an_explicit_selection_wins_over_the_remembered_row() {
    let mut p = panel_with(&["a", "b", "c"]);
    p.selected = 0;

    p.apply_listing(
        PathBuf::from("/left"),
        panel_with(&["a", "b", "c"]).entries,
        None,
        Some("c"),
        10,
    );

    assert_eq!(p.entries[p.selected].name.to_string_lossy(), "c");
}

fn reload(p: &mut PanelState, names: &[&str]) {
    p.apply_listing(
        PathBuf::from("/left"),
        panel_with(names).entries,
        None,
        None,
        10,
    );
}

fn name_under_cursor(p: &PanelState) -> String {
    p.entries[p.selected].name.to_string_lossy().into_owned()
}

#[test]
fn deleting_the_middle_file_puts_the_cursor_on_the_next_one() {
    let mut p = panel_with(&["a", "b", "c", "d", "e"]);
    p.selected = 2;
    reload(&mut p, &["a", "b", "d", "e"]);
    assert_eq!(name_under_cursor(&p), "d");
}

#[test]
fn deleting_the_last_file_puts_the_cursor_on_the_new_last() {
    let mut p = panel_with(&["a", "b", "c", "d", "e"]);
    p.selected = 4;
    reload(&mut p, &["a", "b", "c", "d"]);
    assert_eq!(name_under_cursor(&p), "d");
}

#[test]
fn a_refresh_without_changes_keeps_cursor_and_scroll() {
    let names: Vec<String> = (0..30).map(|n| format!("f{n:02}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut p = panel_with(&refs);
    p.selected = 12;
    p.scroll_offset = 8;
    reload(&mut p, &refs);
    assert_eq!(name_under_cursor(&p), "f12");
    assert_eq!(p.scroll_offset, 8);
}

#[test]
fn the_other_panel_on_the_same_directory_does_not_jump_to_the_top() {
    let mut active = panel_with(&["a", "b", "c", "d", "e"]);
    let mut other = panel_with(&["a", "b", "c", "d", "e"]);
    active.selected = 1;
    other.selected = 3;
    reload(&mut active, &["a", "c", "d", "e"]);
    reload(&mut other, &["a", "c", "d", "e"]);
    assert_eq!(name_under_cursor(&active), "c");
    assert_eq!(name_under_cursor(&other), "d");
}

#[test]
fn another_directory_starts_at_the_top() {
    let mut p = panel_with(&["a", "b", "c"]);
    p.selected = 2;
    p.apply_listing(
        PathBuf::from("/elsewhere"),
        panel_with(&["x", "y", "z"]).entries,
        None,
        None,
        10,
    );
    assert_eq!(p.selected, 0);
    assert_eq!(p.scroll_offset, 0);
}

/// The tags survive a reload too, so the files marked for copying are still
/// marked after it.
#[test]
fn tags_survive_a_reload() {
    let mut p = panel_with(&["a", "b", "c"]);
    p.selection.toggle(&p.entries[1].path.clone());

    p.apply_listing(
        PathBuf::from("/left"),
        panel_with(&["a", "b", "c"]).entries,
        None,
        None,
        10,
    );

    assert_eq!(
        p.selection.len(),
        1,
        "the tag was lost in the reload that followed the operation"
    );
}
