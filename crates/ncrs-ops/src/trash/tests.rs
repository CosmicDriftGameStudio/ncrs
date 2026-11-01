use super::*;

const VISIBLE: usize = 3;

fn item(name: &str, deleted_at: i64) -> TrashedItem {
    TrashedItem {
        original: format!("/home/me/{name}").into(),
        deleted_at,
        key: format!("/trash/{name}").into(),
    }
}

fn view_of(names: &[&str]) -> TrashView {
    let mut view = TrashView::new(TrashReach::Everything);
    let items = names
        .iter()
        .enumerate()
        .map(|(index, name)| item(name, i64::try_from(names.len() - index).unwrap()))
        .collect();
    view.set_items(Ok(items), VISIBLE);
    view
}

#[test]
fn a_new_view_is_reading_until_the_listing_arrives() {
    let mut view = TrashView::new(TrashReach::OwnDeletions);
    assert!(view.loading);

    view.set_items(Ok(vec![item("a", 1)]), VISIBLE);

    assert!(!view.loading);
    assert_eq!(view.items.len(), 1);
}

#[test]
fn the_cursor_moves_inside_the_list_and_scrolls_with_it() {
    let mut view = view_of(&["a", "b", "c", "d", "e"]);

    view.move_cursor(-1, VISIBLE);
    assert_eq!(view.cursor, 0);
    view.last(VISIBLE);
    assert_eq!(view.cursor, 4);
    assert_eq!(view.window.offset(5, VISIBLE), 2);
    view.page(-1, VISIBLE);
    assert_eq!(view.cursor, 1);
    view.first(VISIBLE);
    assert_eq!(view.cursor, 0);
    assert_eq!(view.window.offset(5, VISIBLE), 0);
}

#[test]
fn enter_acts_on_the_cursor_row_when_nothing_is_tagged() {
    let mut view = view_of(&["a", "b", "c"]);
    view.move_cursor(1, VISIBLE);

    assert_eq!(view.action_items(), vec![item("b", 2)]);
}

#[test]
fn enter_acts_on_the_tagged_rows_only() {
    let mut view = view_of(&["a", "b", "c"]);
    view.tag_and_move(1, VISIBLE);
    view.move_cursor(1, VISIBLE);
    view.toggle_tag();

    assert_eq!(view.action_items(), vec![item("a", 3), item("c", 1)]);
    assert_eq!(view.tagged_count(), 2);
    assert!(view.is_tagged(0) && !view.is_tagged(1) && view.is_tagged(2));
}

#[test]
fn tagging_twice_untags_and_all_and_none_work() {
    let mut view = view_of(&["a", "b"]);
    view.toggle_tag();
    view.toggle_tag();
    assert_eq!(view.tagged_count(), 0);

    view.tag_all();
    assert_eq!(view.tagged_count(), 2);
    view.clear_tags();
    assert_eq!(view.tagged_count(), 0);
}

#[test]
fn a_new_listing_clamps_the_cursor_and_drops_the_tags() {
    let mut view = view_of(&["a", "b", "c"]);
    view.last(VISIBLE);
    view.tag_all();

    view.set_items(Ok(vec![item("a", 3)]), VISIBLE);

    assert_eq!(view.cursor, 0);
    assert_eq!(view.tagged_count(), 0);
}

#[test]
fn a_failed_listing_shows_the_reason_and_no_rows() {
    let mut view = view_of(&["a"]);

    view.set_items(Err("no access".to_string()), VISIBLE);

    assert!(view.items.is_empty());
    assert_eq!(view.error.as_deref(), Some("no access"));
    assert!(view.action_items().is_empty());
}

#[test]
fn scrolling_holds_the_cursor_inside_the_window() {
    let mut view = view_of(&["a", "b", "c", "d", "e", "f"]);

    view.scroll(ScrollDelta::Lines { x: 0.0, y: -3.0 }, VISIBLE);

    assert_eq!(view.window.offset(6, VISIBLE), 3);
    assert_eq!(view.cursor, 3);
}
