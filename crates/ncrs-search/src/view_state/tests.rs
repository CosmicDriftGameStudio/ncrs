use super::*;

fn line(number: u64) -> LineMatch {
    LineMatch {
        number,
        text: format!("line {number}"),
        ranges: std::iter::once(0..4).collect(),
    }
}

fn found(path: &str, lines: &[u64]) -> SearchEvent {
    SearchEvent::File {
        path: PathBuf::from(path),
        lines: lines.iter().map(|&number| line(number)).collect(),
        size: 0,
        modified: None,
    }
}

fn view_with(events: Vec<SearchEvent>) -> SearchView {
    let mut view = SearchView::new(PathBuf::from("/root"));
    view.begin(1);
    for event in events {
        view.apply_event(event);
    }
    view
}

fn paths(view: &SearchView) -> Vec<&str> {
    view.results
        .iter()
        .map(|result| result.path.to_str().unwrap())
        .collect()
}

#[test]
fn the_fields_cycle() {
    assert_eq!(SearchField::Names.next(true), SearchField::Folder);
    assert_eq!(SearchField::Folder.next(true), SearchField::Query);
    assert_eq!(SearchField::Query.next(true), SearchField::Exclude);
    assert_eq!(SearchField::Exclude.next(true), SearchField::Names);
}

#[test]
fn the_text_field_is_skipped_while_it_is_not_shown() {
    assert_eq!(SearchField::Names.next(false), SearchField::Folder);
    assert_eq!(SearchField::Folder.next(false), SearchField::Exclude);
    assert_eq!(SearchField::Exclude.next(false), SearchField::Names);
}

#[test]
fn found_files_lists_every_file_in_display_order() {
    let view = view_with(vec![
        found("/root/b.txt", &[1]),
        found("/root/a.txt", &[2, 3]),
    ]);
    assert_eq!(
        view.found_files(),
        [PathBuf::from("/root/a.txt"), PathBuf::from("/root/b.txt")]
    );
}

fn three_files() -> SearchView {
    view_with(vec![
        found("/root/a.txt", &[1, 2]),
        found("/root/b.txt", &[3]),
        found("/root/c.txt", &[]),
    ])
}

#[test]
fn without_tags_the_actions_take_the_cursors_file() {
    let mut view = three_files();
    view.select_row(1, 10);
    assert_eq!(view.cursor, Some(1));
    assert_eq!(view.action_files(), [PathBuf::from("/root/b.txt")]);
}

#[test]
fn with_tags_the_actions_take_exactly_the_tagged_files_in_display_order() {
    let mut view = three_files();
    view.tagged.insert(PathBuf::from("/root/c.txt"));
    view.tagged.insert(PathBuf::from("/root/a.txt"));
    assert_eq!(
        view.action_files(),
        [PathBuf::from("/root/a.txt"), PathBuf::from("/root/c.txt")]
    );
}

#[test]
fn a_tagged_folder_hides_the_tagged_entries_inside_it() {
    let mut view = view_with(vec![
        found("/root/photos", &[]),
        found("/root/photos/2026/a.jpg", &[]),
        found("/root/photos2", &[]),
    ]);
    view.tag_all();
    assert_eq!(
        view.action_files(),
        [
            PathBuf::from("/root/photos"),
            PathBuf::from("/root/photos2")
        ]
    );
}

#[test]
fn without_a_cursor_and_tags_the_actions_take_nothing() {
    let mut view = three_files();
    view.cursor = None;
    assert!(view.action_files().is_empty());
}

#[test]
fn tagging_moves_to_the_next_file() {
    let mut view = three_files();
    view.tag_and_move(1, 10);
    assert_eq!(view.cursor, Some(1));
    view.tag_and_move(1, 10);
    assert_eq!(view.cursor, Some(2));
    assert!(view.is_tagged(0) && view.is_tagged(1) && !view.is_tagged(2));
    // At the last file the tag still toggles and the cursor stays.
    view.tag_and_move(1, 10);
    assert_eq!(view.cursor, Some(2));
    assert!(view.is_tagged(2));
}

#[test]
fn tagging_upwards_goes_to_the_previous_file() {
    let mut view = three_files();
    view.select_row(1, 10);
    view.tag_and_move(-1, 10);
    assert_eq!(view.cursor, Some(0));
    assert!(view.is_tagged(1));
}

#[test]
fn tagging_twice_removes_the_tag() {
    let mut view = three_files();
    view.tag_and_move(1, 10);
    view.first(10);
    view.tag_and_move(1, 10);
    assert!(!view.is_tagged(0));
}

#[test]
fn tag_all_and_a_new_search_clear_it() {
    let mut view = three_files();
    view.tag_all();
    assert_eq!(view.tagged.len(), 3);
    view.tagged.clear();
    assert_eq!(view.action_files().len(), 1);
    view.tag_all();
    view.begin(2);
    assert!(view.tagged.is_empty());
}

#[test]
fn files_are_kept_sorted_by_path_whatever_order_they_arrive_in() {
    let view = view_with(vec![
        found("/root/b.txt", &[1]),
        found("/root/a.txt", &[1]),
        found("/root/sub/c.txt", &[1]),
    ]);
    assert_eq!(
        paths(&view),
        ["/root/a.txt", "/root/b.txt", "/root/sub/c.txt"]
    );
}

#[test]
fn the_cursor_stays_on_the_first_row_until_the_user_moves_it() {
    let mut view = view_with(vec![found("/root/m.txt", &[1])]);
    assert_eq!(view.cursor, Some(0));
    view.apply_event(found("/root/a.txt", &[1]));
    assert_eq!(view.cursor, Some(0));
    assert_eq!(
        view.cursor_target().unwrap().0,
        PathBuf::from("/root/a.txt")
    );
}

#[test]
fn after_the_user_moved_the_cursor_it_stays_on_its_file_when_files_sort_in_front() {
    let mut view = view_with(vec![found("/root/m.txt", &[4, 9])]);
    view.move_cursor(0, 10);
    assert_eq!(view.cursor, Some(0));

    view.apply_event(found("/root/a.txt", &[1]));
    view.apply_event(found("/root/z.txt", &[1]));

    assert_eq!(view.cursor, Some(1));
    assert_eq!(
        view.cursor_target(),
        Some((PathBuf::from("/root/m.txt"), Some(4)))
    );
}

#[test]
fn the_cursor_is_clamped_and_scrolls_into_view() {
    let events = (0..20)
        .map(|i| found(&format!("/root/f{i:02}.txt"), &[]))
        .collect();
    let mut view = view_with(events);
    view.move_cursor(-5, 5);
    assert_eq!((view.cursor, view.scroll_offset), (Some(0), 0));
    view.move_cursor(7, 5);
    assert_eq!((view.cursor, view.scroll_offset), (Some(7), 3));
    view.last(5);
    assert_eq!((view.cursor, view.scroll_offset), (Some(19), 15));
    view.page(-1, 5);
    assert_eq!(view.cursor, Some(14));
    view.first(5);
    assert_eq!((view.cursor, view.scroll_offset), (Some(0), 0));
}

#[test]
fn an_empty_result_list_does_not_panic() {
    let mut view = SearchView::new(PathBuf::from("/root"));
    view.move_cursor(3, 5);
    view.last(5);
    view.scroll(ScrollDelta::Lines { x: 0.0, y: -3.0 }, 5);
    assert_eq!(view.cursor, None);
    assert_eq!(view.cursor_target(), None);
}

#[test]
fn the_wheel_scrolls_by_rows_and_holds_the_cursor_inside() {
    let events = (0..20)
        .map(|i| found(&format!("/root/f{i:02}.txt"), &[]))
        .collect();
    let mut view = view_with(events);
    view.scroll(ScrollDelta::Lines { x: 0.0, y: -3.0 }, 5);
    assert_eq!(view.scroll_offset, 3);
    assert_eq!(view.cursor, Some(3));
    view.scroll(ScrollDelta::Lines { x: 0.0, y: -100.0 }, 5);
    assert_eq!(view.scroll_offset, 15);
    let half_a_row = HIT_LIST_ROW_HEIGHT / 2.0;
    view.scroll(
        ScrollDelta::Pixels {
            x: 0.0,
            y: half_a_row,
        },
        5,
    );
    assert_eq!(view.scroll_offset, 15, "half a row does not move yet");
    view.scroll(
        ScrollDelta::Pixels {
            x: 0.0,
            y: half_a_row,
        },
        5,
    );
    assert_eq!(view.scroll_offset, 14);
}

#[test]
fn the_index_answer_is_cut_at_the_limit_and_the_rest_is_counted() {
    let mut view = SearchView::new(PathBuf::from("archive"));
    view.filters = Some(crate::filters::tests::loaded_search());

    view.mirror_index_within(10, 2);

    assert_eq!((view.results.len(), view.not_shown), (2, 4));
    view.mirror_index_within(10, 10);
    assert_eq!((view.results.len(), view.not_shown), (6, 0));
}

#[test]
fn progress_and_the_end_are_recorded() {
    let mut view = view_with(vec![]);
    view.apply_event(SearchEvent::Progress {
        scanned: 40,
        skipped: 2,
    });
    assert_eq!((view.scanned, view.skipped, view.running), (40, 2, true));
    let totals = SearchTotals {
        scanned: 90,
        files: 1,
        folders: 0,
        matches: 3,
        skipped: 5,
        truncated: false,
    };
    view.apply_event(SearchEvent::Finished(totals));
    assert_eq!(view.totals, Some(totals));
    assert_eq!((view.scanned, view.skipped, view.running), (90, 5, false));
}

#[test]
fn the_engine_query_carries_the_fields() {
    let mut view = SearchView::new(PathBuf::from("/root"));
    view.query = "needle".into();
    view.names = "*.rs".into();
    view.in_files = true;
    view.exclude = "target/".into();
    view.regex = true;
    view.case_sensitive = true;
    let query = view.query_for_engine();
    assert_eq!(query.root, PathBuf::from("/root"));
    assert_eq!(query.text, "needle");
    assert_eq!(query.include, "*.rs");
    assert_eq!(query.exclude, "target/");
    assert!(query.regex && query.case_sensitive && !query.whole_word);
    assert!(query.same_file_system);
}

#[test]
fn only_the_macos_root_leaves_its_file_system() {
    assert!(stays_on_file_system(Path::new("/Users")));
    assert!(stays_on_file_system(Path::new("/tmp/x")));
    assert_eq!(
        stays_on_file_system(Path::new("/")),
        !cfg!(target_os = "macos")
    );
}

#[test]
fn the_cursor_target_names_the_file_and_its_first_match() {
    let view = view_with(vec![
        found("/root/a.txt", &[7, 9]),
        found("/root/b.txt", &[]),
    ]);
    assert_eq!(
        view.cursor_target(),
        Some((PathBuf::from("/root/a.txt"), Some(7)))
    );
    let mut moved = view;
    moved.move_cursor(1, 10);
    assert_eq!(
        moved.cursor_target(),
        Some((PathBuf::from("/root/b.txt"), None))
    );
}

#[test]
fn a_content_row_shows_the_first_match_and_counts_the_others() {
    let view = view_with(vec![
        found("/root/a.txt", &[7, 9, 12]),
        found("/root/b.txt", &[3]),
    ]);
    let several = &view.results[0];
    assert_eq!(several.first_match().map(|m| m.number), Some(7));
    assert_eq!(several.more_matches(), 2);
    let single = &view.results[1];
    assert_eq!(single.first_match().map(|m| m.number), Some(3));
    assert_eq!(single.more_matches(), 0);
}

#[test]
fn a_name_search_has_no_matching_line_to_show() {
    let view = view_with(vec![found("/root/a.txt", &[])]);
    assert_eq!(view.results[0].first_match(), None);
    assert_eq!(view.results[0].more_matches(), 0);
}

#[test]
fn a_content_search_has_three_line_rows_and_a_name_search_two() {
    let height_is =
        |view: &SearchView, expected: f32| view.row_height().total_cmp(&expected).is_eq();
    let mut view = SearchView::new(PathBuf::from("/root"));
    view.begin(1);
    assert!(height_is(&view, HIT_LIST_ROW_HEIGHT));
    view.query = "needle".into();
    view.in_files = true;
    view.begin(2);
    assert!(height_is(&view, SEARCH_CONTENT_ROW_HEIGHT));
    view.query.clear();
    assert!(
        height_is(&view, SEARCH_CONTENT_ROW_HEIGHT),
        "the height is fixed for the result set"
    );
    view.begin(3);
    assert!(height_is(&view, HIT_LIST_ROW_HEIGHT));
}

#[test]
fn the_entry_of_a_result_carries_its_size_and_date() {
    let modified = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(86_400);
    let result = FileResult {
        path: PathBuf::from("/root/sub/a.txt"),
        is_dir: false,
        lines: Vec::new(),
        size: 42,
        modified: Some(modified),
    };
    let entry = result.entry().unwrap();
    assert_eq!(entry.name, std::ffi::OsString::from("a.txt"));
    assert_eq!((entry.size, entry.modified), (42, Some(modified)));
    assert!(!entry.is_dir);
}

#[test]
fn the_text_is_searched_only_while_the_box_is_ticked() {
    let mut view = SearchView::new(PathBuf::from("/root"));
    view.names = "*.md".into();
    view.query = "needle".into();
    assert!(view.is_name_search());
    assert_eq!(view.query_for_engine().text, "");
    view.in_files = true;
    assert!(!view.is_name_search());
    assert_eq!(view.query_for_engine().text, "needle");
    assert_eq!(view.query_for_engine().include, "*.md");
}

#[test]
fn names_ignore_the_case_toggle_while_the_box_is_off() {
    let mut view = SearchView::new(PathBuf::from("/root"));
    view.case_sensitive = true;
    assert!(!view.query_for_engine().case_sensitive);
    view.in_files = true;
    assert!(view.query_for_engine().case_sensitive);
}

#[test]
fn a_folder_hit_is_a_folder_row() {
    let mut view = view_with(vec![]);
    view.apply_event(SearchEvent::Folder {
        path: PathBuf::from("/root/docs"),
        modified: None,
    });
    view.apply_event(found("/root/a.txt", &[]));
    assert_eq!(paths(&view), ["/root/a.txt", "/root/docs"]);
    assert!(!view.results[0].entry().unwrap().is_dir);
    assert!(view.results[1].entry().unwrap().is_dir);
}

#[test]
fn the_hit_list_is_named_after_the_text_or_else_the_names() {
    let mut view = SearchView::new(PathBuf::from("/root"));
    view.names = " *.pdf ".into();
    view.query = "needle".into();
    assert_eq!(view.hit_list_title(), "*.pdf");
    view.in_files = true;
    assert_eq!(view.hit_list_title(), "needle");
}
