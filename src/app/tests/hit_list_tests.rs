use crate::app::*;
use crate::fs::search::{LineMatch, SearchEvent};
use crate::messages::SearchMessage;

struct Hits {
    app: App,
    folder: tempfile::TempDir,
    files: Vec<PathBuf>,
}

/// An app whose search found `names` (made as real files in two folders),
/// opened from the left panel.
fn searched(names: &[&str]) -> Hits {
    let folder = tempfile::tempdir().unwrap();
    let mut files = Vec::new();
    for (index, name) in names.iter().enumerate() {
        let dir = folder.path().join(format!("dir{index}"));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(name);
        std::fs::write(&file, name).unwrap();
        files.push(file);
    }
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::OpenSearch));
    let generation = app.search_for_test().unwrap().generation;
    let events = files
        .iter()
        .map(|path| SearchEvent::File {
            path: path.clone(),
            lines: vec![LineMatch {
                number: 1,
                text: "x".to_owned(),
                ranges: std::iter::once(0..1).collect(),
            }],
            size: 0,
            modified: None,
        })
        .collect();
    drop(app.update(Message::Search(SearchMessage::Events {
        generation,
        events,
    })));
    app.search.view.as_mut().unwrap().query = "needle".to_owned();
    Hits { app, folder, files }
}

fn read_all(app: &mut App, side: PanelSide, files: &[PathBuf]) {
    let request_id = app.panel(side).request_id;
    let entries = files
        .iter()
        .filter_map(|path| fs::FileEntry::from_path(path))
        .collect();
    drop(app.update(Message::HitList(HitListMessage::Read {
        side,
        request_id,
        entries,
    })));
}

fn opened(names: &[&str], side: PanelSide) -> Hits {
    let mut hits = searched(names);
    drop(
        hits.app
            .update(Message::Search(SearchMessage::OpenHitList(side))),
    );
    let files = hits.files.clone();
    read_all(&mut hits.app, side, &files);
    hits
}

#[test]
fn opening_on_the_search_side_closes_the_search_and_lists_the_files() {
    let hits = opened(&["a.txt", "b.txt"], PanelSide::Left);
    let panel = hits.app.panel(PanelSide::Left);
    assert!(!hits.app.search.open);
    assert!(panel.is_hit_list() && !panel.is_virtual() && !panel.loading);
    assert_eq!(panel.entries.len(), 2);
    assert!(panel.entries.iter().all(|entry| !entry.is_parent));
    let list = panel.hit_list.as_ref().unwrap();
    assert_eq!(list.query, "needle");
    assert_eq!(list.return_to, PathBuf::from("/home/test/links"));
    assert_eq!(panel.path, list.scope);
    assert_eq!(hits.app.search_for_test().unwrap().query, "needle");
}

#[test]
fn opening_on_the_other_side_keeps_the_search_and_activates_the_list() {
    let hits = opened(&["a.txt"], PanelSide::Right);
    assert!(hits.app.search.open);
    assert_eq!(hits.app.panels.active, PanelSide::Right);
    assert!(hits.app.panel(PanelSide::Right).is_hit_list());
    assert!(!hits.app.panel(PanelSide::Left).is_hit_list());
}

#[test]
fn nothing_opens_without_results() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::OpenSearch));
    drop(app.update(Message::Search(SearchMessage::OpenHitList(
        PanelSide::Right,
    ))));
    assert!(!app.panel(PanelSide::Right).is_hit_list());
    assert!(app.search.open);
}

#[test]
fn a_read_of_an_older_open_is_ignored() {
    let mut hits = searched(&["a.txt"]);
    drop(hits.app.update(Message::Search(SearchMessage::OpenHitList(
        PanelSide::Right,
    ))));
    let stale = hits.app.panel(PanelSide::Right).request_id;
    drop(hits.app.update(Message::Search(SearchMessage::OpenHitList(
        PanelSide::Right,
    ))));
    assert_ne!(hits.app.panel(PanelSide::Right).request_id, stale);
    drop(hits.app.update(Message::HitList(HitListMessage::Read {
        side: PanelSide::Right,
        request_id: stale,
        entries: Vec::new(),
    })));
    assert!(hits.app.panel(PanelSide::Right).loading);
    let files = hits.files.clone();
    read_all(&mut hits.app, PanelSide::Right, &files);
    assert_eq!(hits.app.panel(PanelSide::Right).entries.len(), 1);
}

#[test]
fn a_file_that_is_gone_is_not_listed() {
    let hits = searched(&["a.txt"]);
    std::fs::remove_file(&hits.files[0]).unwrap();
    assert!(fs::FileEntry::from_path(&hits.files[0]).is_none());
}

#[test]
fn escape_goes_back_to_the_search_with_its_fields() {
    let mut hits = opened(&["a.txt"], PanelSide::Left);
    let state = hits.app.prompt_key_state();
    assert!(state.hit_list);
    assert_eq!(
        route_key(&state, Key::Named(Named::Escape), Modifiers::default()),
        Some(Message::HitList(HitListMessage::BackToSearch))
    );
    let running = PromptKeyState {
        job_running: true,
        ..state
    };
    assert_eq!(
        route_key(&running, Key::Named(Named::Escape), Modifiers::default()),
        Some(Message::AbortJob)
    );

    drop(
        hits.app
            .update(Message::HitList(HitListMessage::BackToSearch)),
    );
    let panel = hits.app.panel(PanelSide::Left);
    assert!(!panel.is_hit_list());
    assert_eq!(panel.path, PathBuf::from("/home/test/links"));
    assert!(hits.app.search.open && hits.app.search.side == PanelSide::Left);
    assert_eq!(hits.app.search_for_test().unwrap().query, "needle");
    assert!(panel.history.back().is_empty());
}

#[test]
fn the_cross_closes_the_list_without_the_search() {
    let mut hits = opened(&["a.txt"], PanelSide::Left);
    drop(
        hits.app
            .update(Message::HitList(HitListMessage::Close(PanelSide::Left))),
    );
    assert!(!hits.app.panel(PanelSide::Left).is_hit_list());
    assert!(!hits.app.search.open);
}

#[test]
fn going_up_does_nothing_in_a_list() {
    let hits = opened(&["a.txt"], PanelSide::Left);
    let path = hits.app.panel(PanelSide::Left).path.clone();
    assert!(ncrs_files::update::go_up(&hits.app.panels, PanelSide::Left).is_none());
    assert!(hits.app.panel(PanelSide::Left).is_hit_list());
    assert_eq!(hits.app.panel(PanelSide::Left).path, path);
}

#[test]
fn loading_another_folder_drops_the_list_and_loading_its_scope_refreshes_it() {
    let mut hits = opened(&["a.txt"], PanelSide::Left);
    let scope = hits.app.panel(PanelSide::Left).path.clone();
    drop(hits.app.load(PanelSide::Left, scope, None));
    assert!(hits.app.panel(PanelSide::Left).is_hit_list());
    assert!(hits.app.panel(PanelSide::Left).loading);

    let elsewhere = hits.folder.path().to_path_buf();
    drop(hits.app.load(PanelSide::Left, elsewhere, None));
    assert!(!hits.app.panel(PanelSide::Left).is_hit_list());
    assert!(hits.app.panel(PanelSide::Left).history.back().is_empty());
}

#[test]
fn a_refresh_drops_vanished_rows_and_keeps_cursor_and_tags_by_path() {
    let mut hits = opened(&["a.txt", "b.txt", "c.txt"], PanelSide::Left);
    let kept = hits.app.panel(PanelSide::Left).entries[2].path.clone();
    let cursor_panel = hits.app.panel_mut(PanelSide::Left);
    cursor_panel.selection.toggle(&kept);
    cursor_panel.selected = 2;
    std::fs::remove_file(&hits.files[0]).unwrap();
    let scope = hits.app.panel(PanelSide::Left).path.clone();
    drop(hits.app.load(PanelSide::Left, scope, None));
    let survivors = hits.files[1..].to_vec();
    read_all(&mut hits.app, PanelSide::Left, &survivors);
    let panel = hits.app.panel(PanelSide::Left);
    assert_eq!(panel.entries.len(), 2);
    assert!(panel.selection.is_tagged(&kept));
    assert_eq!(panel.selected_entry().unwrap().path, kept);
}

#[test]
fn remove_from_list_takes_the_tagged_rows_else_the_cursor_row() {
    let mut hits = opened(&["a.txt", "b.txt", "c.txt"], PanelSide::Left);
    hits.app.panel_mut(PanelSide::Left).selected = 1;
    drop(
        hits.app
            .update(Message::HitList(HitListMessage::RemoveFromList)),
    );
    assert_eq!(hits.app.panel(PanelSide::Left).entries.len(), 2);

    let first = hits.app.panel(PanelSide::Left).entries[0].path.clone();
    let second = hits.app.panel(PanelSide::Left).entries[1].path.clone();
    hits.app.panel_mut(PanelSide::Left).selection.toggle(&first);
    hits.app
        .panel_mut(PanelSide::Left)
        .selection
        .toggle(&second);
    drop(
        hits.app
            .update(Message::HitList(HitListMessage::RemoveFromList)),
    );
    assert!(hits.app.panel(PanelSide::Left).entries.is_empty());
    assert!(hits.files.iter().all(|file| file.exists()));
}

#[test]
fn show_on_the_other_side_loads_the_folder_and_keeps_the_list() {
    let mut hits = opened(&["a.txt"], PanelSide::Left);
    drop(
        hits.app
            .update(Message::HitList(HitListMessage::ShowOnOtherSide)),
    );
    assert!(hits.app.panel(PanelSide::Left).is_hit_list());
    assert_eq!(hits.app.panels.active, PanelSide::Right);
    assert!(hits.app.panel(PanelSide::Right).loading);
}

#[test]
fn making_folders_and_pasting_are_not_offered_in_a_list() {
    let mut hits = opened(&["a.txt"], PanelSide::Left);
    for message in [
        Message::CreateDirPrompt,
        Message::CreateFilePrompt,
        Message::Paste,
        Message::ComputeFolderSizes,
    ] {
        drop(hits.app.apply_unless_refused(message));
        assert!(hits.app.ops.prompt.is_none());
    }
}

#[test]
fn a_list_is_refused_as_the_target_of_a_copy() {
    let mut hits = opened(&["a.txt"], PanelSide::Right);
    let own = hits.files[0].clone();
    let folder = hits.folder.path().to_path_buf();
    let entry = fs::FileEntry::from_path(&own).unwrap();
    hits.app
        .panel_mut(PanelSide::Left)
        .apply_listing(folder, vec![entry], None, None, 12);
    hits.app.panels.active = PanelSide::Left;
    hits.app.search.open = false;
    drop(hits.app.open_transfer_dialog_to(TransferKind::Copy, None));
    assert!(hits.app.ops.transfer_dialog.is_none());
    let shown = hits.app.job_error.clone().unwrap();
    assert_eq!(shown, hits.app.lang.text(Msg::StatusHitListNotAFolder));
}

#[tokio::test]
async fn equal_names_from_several_folders_are_numbered_in_the_transfer() {
    let mut hits = searched(&["same.txt", "same.txt"]);
    let target = tempfile::tempdir().unwrap();
    let sources = hits.files.clone();
    let target_path = target.path().to_path_buf();
    drop(hits.app.with_ops(|ops, ctx| {
        ncrs_ops::begin_transfer(
            ops,
            TransferKind::Copy,
            sources,
            target_path,
            Policy::Ask,
            ctx,
        )
    }));
    let transfer = hits.app.ops.transfer.clone().unwrap();
    assert_eq!(transfer.name_in_target(0), None);
    assert_eq!(
        transfer.name_in_target(1),
        Some(OsString::from("same (2).txt"))
    );
}

#[tokio::test]
async fn f5_on_two_tagged_results_with_equal_names_numbers_the_second() {
    let mut hits = searched(&["same.txt", "same.txt"]);
    let marking = |message| Message::Search(message);
    drop(hits.app.update(marking(SearchMessage::ToggleTag)));
    drop(hits.app.update(marking(SearchMessage::ToggleTag)));
    let sources = hits.app.action_sources();
    assert_eq!(sources, hits.files);

    let target = tempfile::tempdir().unwrap();
    let target_path = target.path().to_path_buf();
    drop(hits.app.with_ops(|ops, ctx| {
        ncrs_ops::begin_transfer(
            ops,
            TransferKind::Copy,
            sources,
            target_path,
            Policy::Ask,
            ctx,
        )
    }));
    let transfer = hits.app.ops.transfer.clone().unwrap();
    assert_eq!(transfer.name_in_target(0), None);
    assert_eq!(
        transfer.name_in_target(1),
        Some(OsString::from("same (2).txt"))
    );
}

#[test]
fn numbered_names_cannot_go_into_an_archive() {
    let mut hits = searched(&["same.txt", "same.txt"]);
    let target = tempfile::tempdir().unwrap();
    let archive = target.path().join("x.zip");
    let first = hits.files[0].clone();
    let mut progress = fs::transfer::Counting;
    fs::archive::pack(&[first], &archive, &mut progress).unwrap();
    let sources = hits.files.clone();
    drop(hits.app.with_ops(|ops, ctx| {
        ncrs_ops::start_validated_transfer(
            ops,
            TransferKind::Copy,
            sources,
            archive,
            Policy::Ask,
            ctx,
        )
    }));
    assert!(hits.app.ops.transfer.is_none() && hits.app.ops.archive_write.is_none());
    assert_eq!(
        hits.app.job_error.clone().unwrap(),
        hits.app.lang.text(Msg::StatusNumberingNeedsFolder)
    );
}

#[test]
fn a_cursor_past_half_the_rows_stays_in_view_in_a_list() {
    let names: Vec<String> = (0..10).map(|n| format!("f{n}.txt")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut hits = opened(&refs, PanelSide::Left);
    let rows = hits
        .app
        .panels
        .visible_rows_of(PanelSide::Left, hits.app.visible_rows);
    assert_eq!(rows, 6);
    let folder_rows = hits.app.visible_rows;
    let panel = hits.app.panel_mut(PanelSide::Left);
    panel.select(rows + 1, folder_rows);
    assert!(panel.scroll_offset <= panel.selected);
    assert!(panel.selected < panel.scroll_offset + rows);
}

#[test]
fn the_archive_name_comes_from_a_plain_query_only() {
    assert_eq!(
        ncrs_files::hit_list::hit_archive_name(" invoice "),
        Some("invoice.zip".to_owned())
    );
    assert_eq!(ncrs_files::hit_list::hit_archive_name("rech.*nung"), None);
    assert_eq!(ncrs_files::hit_list::hit_archive_name("a/b"), None);
    assert_eq!(ncrs_files::hit_list::hit_archive_name(".."), None);
    assert_eq!(ncrs_files::hit_list::hit_archive_name(""), None);
}

fn pack_prompt_of(hits: &mut Hits) {
    for file in hits.files.clone() {
        hits.app.panel_mut(PanelSide::Left).selection.toggle(&file);
    }
    drop(hits.app.open_pack_prompt());
}

#[test]
fn the_pack_prompt_of_a_list_suggests_the_query_and_lists_equal_names() {
    let mut hits = opened(&["same.txt", "same.txt"], PanelSide::Left);
    pack_prompt_of(&mut hits);
    let prompt = hits.app.ops.prompt.as_ref().unwrap();
    assert_eq!(
        Path::new(prompt.name()).file_name(),
        Some(std::ffi::OsStr::new("needle.zip"))
    );
    let pack = prompt.hits().unwrap();
    assert!(!pack.keep_folders);
    assert_eq!(pack.numbered.len(), 2);
}

#[test]
fn a_query_that_is_no_file_name_falls_back_to_the_folder_name() {
    let mut hits = opened(&["a.txt", "b.txt"], PanelSide::Left);
    hits.app
        .panel_mut(PanelSide::Left)
        .hit_list
        .as_mut()
        .unwrap()
        .query = "rech.*nung".to_owned();
    pack_prompt_of(&mut hits);
    let name = hits.app.ops.prompt.as_ref().unwrap().name().to_owned();
    assert_eq!(
        Path::new(&name).file_name(),
        Some(std::ffi::OsStr::new(&ncrs_ops::suggested_archive_name(
            &hits.files
        )))
    );
}

#[test]
fn keep_folders_toggles_in_the_prompt_and_is_off_at_first() {
    let mut hits = opened(&["a.txt", "b.txt"], PanelSide::Left);
    pack_prompt_of(&mut hits);
    drop(hits.app.update(Message::PromptToggleKeepFolders));
    let prompt = hits.app.ops.prompt.as_ref().unwrap();
    assert!(prompt.hits().unwrap().keep_folders);
    assert_eq!(
        prompt.kept_folders_under(),
        Some(
            hits.app
                .panel(PanelSide::Left)
                .hit_list
                .as_ref()
                .unwrap()
                .scope
                .as_path()
        )
    );
    drop(hits.app.update(Message::PromptToggleKeepFolders));
    assert!(hits
        .app
        .ops
        .prompt
        .as_ref()
        .unwrap()
        .kept_folders_under()
        .is_none());
}

#[test]
fn the_status_line_of_a_list_counts_its_files() {
    let mut hits = opened(&["a.txt", "b.txt"], PanelSide::Left);
    let many = hits.app.hit_line().unwrap();
    assert!(many.text.contains("needle") && many.text.contains("2"));
    assert_eq!(many.back_key, "Esc");
    let gone = hits.files[1].clone();
    hits.app.panel_mut(PanelSide::Left).remove_hits(&[gone], 12);
    let one = hits.app.hit_line().unwrap();
    assert_ne!(one.text, many.text);
    assert!(one.text.contains('1'));
}

#[test]
fn a_list_has_no_status_line_outside_a_list() {
    assert!(App::with_fixed_panels().hit_line().is_none());
}

#[test]
fn a_tagged_row_of_a_list_is_marked() {
    let mut hits = opened(&["a.txt", "b.txt"], PanelSide::Left);
    let panel = hits.app.panel_mut(PanelSide::Left);
    let tagged = panel.entries[1].path.clone();
    panel.selection.toggle(&tagged);
    let marks: Vec<bool> = (0..panel.entries.len())
        .map(|index| {
            panel
                .selection
                .state_of(index, &panel.entries, panel.selected)
                .tagged
        })
        .collect();
    assert_eq!(marks, [false, true]);
}

#[test]
fn the_key_bar_of_a_list_leaves_out_new_folder() {
    let mut hits = opened(&["a.txt"], PanelSide::Left);
    assert!(hits.app.function_key_bar()[keymap::NEW_FOLDER_SLOT].is_none());
    hits.app.panel_mut(PanelSide::Left).hit_list = None;
    assert!(hits.app.function_key_bar()[keymap::NEW_FOLDER_SLOT].is_some());
}
