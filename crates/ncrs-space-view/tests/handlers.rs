//! The tab without a window: keys, the dialog, the run, the result and the decide pane,
//! driven by messages on a scan that was written down.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod common;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use common::{ctx, entry, log_of, path, scan_of, send, small_scan, tab, GIB, HOME};
use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use ncrs_space::decide::{DecideKind, DownloadGroups, DownloadItem, Matched};
use ncrs_space::{EntryKind, Group, LogRow, Skip, UnknownUse};
use ncrs_space_view::{
    commands, message_of_command, space_key, Action, ConfirmKind, DecideMessage, Finished, KeyMode,
    Message, RowKind, SpaceKeys,
};
use ncrs_ui_kit::confirm::ConfirmFocus;

const CACHE_ROW: &str = "/Users/marc/.cargo-target";

fn row_of(state: &ncrs_space_view::State, wanted: &str) -> ncrs_space_view::DisplayRow {
    state
        .rows()
        .iter()
        .find(|row| row.path == path(wanted))
        .cloned()
        .unwrap()
}

#[test]
fn a_new_tab_measures_and_the_answer_fills_the_groups() {
    let ctx = ctx();
    let (state, actions) = tab(small_scan(), &ctx);
    assert!(!state.is_measuring());
    assert!(matches!(actions.as_slice(), [Action::Measured(_)]));
    let groups: Vec<Group> = state.rows().iter().map(|row| row.group).collect();
    assert_eq!(
        groups,
        [
            Group::Safe,
            Group::Safe,
            Group::Worktrees,
            Group::Decide,
            Group::Decide,
            Group::InUse
        ]
    );
    let downloads = state
        .rows()
        .iter()
        .filter(|row| row.kind == RowKind::Downloads)
        .count();
    assert_eq!(downloads, 1, "the items of Downloads are one row");
}

#[test]
fn an_answer_of_an_older_measure_is_dropped() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    send(&mut state, Message::Rescan, &ctx);
    let stale = state.scan_run() - 1;
    let actions = send(
        &mut state,
        Message::Scanned {
            run: stale,
            scan: Some(small_scan()),
        },
        &ctx,
    );
    assert!(actions.is_empty());
    assert!(state.is_measuring());
}

#[test]
fn the_cursor_walks_heads_and_rows_and_the_ends_are_reachable() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    assert_eq!(state.cursor_index(), Some(0));
    send(&mut state, Message::Down, &ctx);
    assert_eq!(state.cursor_index(), Some(1));
    send(&mut state, Message::Up, &ctx);
    assert_eq!(state.cursor_index(), Some(0));
    send(&mut state, Message::Last, &ctx);
    assert_eq!(state.cursor_index(), Some(state.items().len() - 1));
    send(&mut state, Message::First, &ctx);
    assert_eq!(state.cursor_index(), Some(0));
    send(&mut state, Message::PageDown, &ctx);
    assert_eq!(state.cursor_index(), Some(state.items().len() - 1));
    send(&mut state, Message::PageUp, &ctx);
    assert_eq!(state.cursor_index(), Some(0));
}

#[test]
fn left_closes_a_group_and_right_opens_it() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    let open_items = state.items().len();
    send(&mut state, Message::Left, &ctx);
    assert!(state.is_folded(Group::Safe));
    assert!(state.items().len() < open_items);
    send(&mut state, Message::Right, &ctx);
    assert!(!state.is_folded(Group::Safe));
    send(&mut state, Message::Down, &ctx);
    send(&mut state, Message::Left, &ctx);
    assert_eq!(
        state.cursor_index(),
        Some(0),
        "from a row, left goes to the head"
    );
}

#[test]
fn space_unticks_a_row_and_the_total_follows() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    let before = state.ticked_bytes(Group::Safe);
    send(&mut state, Message::RowPressed(path(CACHE_ROW)), &ctx);
    send(&mut state, Message::Toggle, &ctx);
    assert_eq!(state.ticked_bytes(Group::Safe), before - 40 * GIB);
    send(&mut state, Message::Toggle, &ctx);
    assert_eq!(state.ticked_bytes(Group::Safe), before);
}

#[test]
fn a_row_in_use_cannot_be_ticked() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    send(
        &mut state,
        Message::RowPressed(path("/Users/marc/.gradle")),
        &ctx,
    );
    send(&mut state, Message::Toggle, &ctx);
    assert!(!state.is_ticked(&row_of(&state, "/Users/marc/.gradle")));
}

#[test]
fn the_dialog_opens_with_the_focus_on_cancel_and_escape_closes_it_without_deleting() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    send(&mut state, Message::DeleteSafe, &ctx);
    let ask = state.confirm().unwrap();
    assert_eq!(ask.kind, ConfirmKind::Safe);
    assert_eq!(ask.focus, ConfirmFocus::Cancel);
    assert_eq!(ask.safe.len(), 2);
    let actions = send(&mut state, Message::Escape, &ctx);
    assert!(state.confirm().is_none());
    assert!(state.running().is_none());
    assert!(actions.is_empty(), "the first Esc only closes the dialog");
}

#[test]
fn enter_on_cancel_closes_and_enter_on_the_delete_button_starts_the_run() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    send(&mut state, Message::DeleteSafe, &ctx);
    send(&mut state, Message::ConfirmEnter, &ctx);
    assert!(state.confirm().is_none());
    assert!(state.running().is_none());
    send(&mut state, Message::DeleteSafe, &ctx);
    send(&mut state, Message::ConfirmSwitchFocus, &ctx);
    assert_eq!(state.confirm().unwrap().focus, ConfirmFocus::Confirm);
    let actions = send(&mut state, Message::ConfirmEnter, &ctx);
    let running = state.running().unwrap();
    assert_eq!(running.plan.safe.len(), 2);
    assert_eq!(running.plan.unknown, UnknownUse::Skip);
    assert!(matches!(actions.as_slice(), [Action::SetJobProgress(_)]));
}

#[test]
fn without_a_process_list_the_dialog_says_so_and_the_run_goes_on_after_the_confirm() {
    let mut ctx = ctx();
    ctx.processes_known = false;
    let (mut state, _) = tab(small_scan(), &ctx);
    send(&mut state, Message::DeleteSafe, &ctx);
    assert!(state.confirm().unwrap().unknown_use);
    send(&mut state, Message::ConfirmAccept, &ctx);
    assert_eq!(
        state.running().unwrap().plan.unknown,
        UnknownUse::AllowAfterConfirm
    );
}

#[test]
fn the_store_build_has_no_worktrees() {
    let mut ctx = ctx();
    ctx.store_build = true;
    let (mut state, _) = tab(small_scan(), &ctx);
    assert!(state.rows().iter().all(|row| row.group != Group::Worktrees));
    send(&mut state, Message::RemoveWorktrees, &ctx);
    assert!(state.confirm().is_none());
    send(&mut state, Message::DeleteSafe, &ctx);
    send(&mut state, Message::ConfirmWithWorktreesToggled, &ctx);
    assert!(!state.confirm().unwrap().with_worktrees);
}

#[test]
fn the_checkbox_takes_the_merged_worktrees_along() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    send(&mut state, Message::DeleteSafe, &ctx);
    assert!(state.confirm().unwrap().plan().worktrees.is_empty());
    send(&mut state, Message::ConfirmWithWorktreesToggled, &ctx);
    assert_eq!(state.confirm().unwrap().plan().worktrees.len(), 1);
}

#[test]
fn the_worktree_button_asks_only_about_worktrees() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    send(&mut state, Message::RemoveWorktrees, &ctx);
    let ask = state.confirm().unwrap();
    assert_eq!(ask.kind, ConfirmKind::Worktrees);
    assert!(ask.safe.is_empty());
    assert_eq!(ask.worktrees.len(), 1);
}

fn finished_run(state: &mut ncrs_space_view::State, ctx: &ncrs_space_view::Ctx) -> Vec<Action> {
    send(state, Message::DeleteSafe, ctx);
    send(state, Message::ConfirmAccept, ctx);
    let run = state.running().unwrap().run;
    let log = log_of(vec![
        LogRow {
            path: path(CACHE_ROW),
            size: 40 * GIB,
            outcome: Ok(40 * GIB),
            last_used: None,
            use_checked: false,
        },
        LogRow {
            path: path("/Users/marc/.yarn/cache"),
            size: 6 * GIB,
            outcome: Err(Skip::UseUnknown),
            last_used: None,
            use_checked: false,
        },
    ]);
    send(
        state,
        Message::RunFinished(Finished {
            run,
            log,
            free_after: Some(118 * GIB),
            log_error: None,
        }),
        ctx,
    )
}

#[test]
fn a_finished_run_shows_what_it_freed_and_what_it_skipped() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    let actions = finished_run(&mut state, &ctx);
    assert!(matches!(
        actions.as_slice(),
        [Action::ClearJobProgress, Action::RunDone(_)]
    ));
    let result = state.result().unwrap();
    assert_eq!(result.freed(), 40 * GIB);
    assert_eq!(result.deleted(), 1);
    assert_eq!(result.skipped(), 1);
    assert_eq!(result.free_before, 78 * GIB);
    assert_eq!(result.free_after, Some(118 * GIB));
    assert!(state.running().is_none());
    assert!(
        state.is_measuring(),
        "the drive is measured again after a run"
    );
}

#[test]
fn a_run_of_an_older_number_is_not_taken_for_the_current_one() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    send(&mut state, Message::DeleteSafe, &ctx);
    send(&mut state, Message::ConfirmAccept, &ctx);
    let run = state.running().unwrap().run;
    let actions = send(
        &mut state,
        Message::RunFinished(Finished {
            run: run + 7,
            log: log_of(Vec::new()),
            free_after: None,
            log_error: None,
        }),
        &ctx,
    );
    assert!(actions.is_empty());
    assert!(state.running().is_some());
    assert!(state.result().is_none());
}

#[test]
fn try_again_asks_once_more_about_the_skipped_rows_only() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    finished_run(&mut state, &ctx);
    let run = state.scan_run();
    send(
        &mut state,
        Message::Scanned {
            run,
            scan: Some(small_scan()),
        },
        &ctx,
    );
    send(&mut state, Message::Retry, &ctx);
    let ask = state.confirm().unwrap();
    assert_eq!(ask.focus, ConfirmFocus::Cancel);
    let paths: Vec<_> = ask.safe.iter().map(|entry| entry.path.clone()).collect();
    assert_eq!(paths, [path("/Users/marc/.yarn/cache")]);
}

#[test]
fn reading_the_log_fills_the_result_and_escape_closes_it() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    finished_run(&mut state, &ctx);
    let text =
        "# run\tstarted=1790000000\tfinished=1790000005\tfreed=10\n1790000001\t/a\t10\tfreed\t10";
    send(
        &mut state,
        Message::LogRead(text.lines().map(str::to_owned).collect()),
        &ctx,
    );
    assert!(state.result().unwrap().log_run.is_some());
    send(&mut state, Message::Escape, &ctx);
    assert!(state.result().is_none());
}

#[test]
fn escape_stops_a_measure_and_only_then_closes_the_tab() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    send(&mut state, Message::Rescan, &ctx);
    let stop = Arc::clone(&state.measuring().unwrap().stop);
    assert!(!stop.load(Ordering::Relaxed));
    let actions = send(&mut state, Message::Escape, &ctx);
    assert!(stop.load(Ordering::Relaxed));
    assert!(actions.is_empty());
    let run = state.scan_run();
    send(&mut state, Message::Scanned { run, scan: None }, &ctx);
    assert!(state.failure().is_none(), "a stopped measure is no failure");
    let closing = send(&mut state, Message::Escape, &ctx);
    assert_eq!(closing, [Action::Closed]);
}

#[test]
fn after_a_run_the_banner_the_drive_and_the_status_bar_share_the_new_measure() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    let before = state.volume_now().unwrap().available;
    send(&mut state, Message::DeleteSafe, &ctx);
    send(&mut state, Message::ConfirmAccept, &ctx);
    let run = state.running().unwrap().run;
    let after = before + 5 * GIB;
    send(
        &mut state,
        Message::RunFinished(ncrs_space_view::Finished {
            run,
            log: log_of(Vec::new()),
            free_after: Some(before),
            log_error: None,
        }),
        &ctx,
    );
    let mut again = (*small_scan()).clone();
    again.volume.available = after;
    let again_run = state.scan_run();
    send(
        &mut state,
        Message::Scanned {
            run: again_run,
            scan: Some(Arc::new(again)),
        },
        &ctx,
    );
    assert_eq!(state.volume_now().unwrap().available, after);
}

#[test]
fn a_measure_that_ends_without_a_result_shows_a_failure() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    send(&mut state, Message::Rescan, &ctx);
    let run = state.scan_run();
    send(&mut state, Message::Scanned { run, scan: None }, &ctx);
    assert!(state
        .failure()
        .is_some_and(|text| text.contains("measured")));
}

#[test]
fn the_merged_worktrees_fold_after_the_first_ones_until_expanded() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    assert_eq!(state.row_limit(ncrs_space::Group::Worktrees), 8);
    send(&mut state, Message::ExpandWorktrees, &ctx);
    assert_eq!(state.row_limit(ncrs_space::Group::Worktrees), usize::MAX);
    assert_eq!(state.row_limit(ncrs_space::Group::Safe), usize::MAX);
}

#[test]
fn the_filter_narrows_the_rows_and_escape_clears_it() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    send(&mut state, Message::FocusFilter, &ctx);
    assert!(state.filter_focused());
    send(&mut state, Message::FilterChanged("yarn".to_owned()), &ctx);
    let rows = state
        .items()
        .iter()
        .filter(|item| matches!(item, ncrs_space_view::Item::Row(_)))
        .count();
    assert_eq!(rows, 1);
    send(&mut state, Message::FilterSubmitted, &ctx);
    send(&mut state, Message::Escape, &ctx);
    assert_eq!(state.filter(), "");
}

#[test]
fn the_row_menu_opens_from_the_keyboard_and_delete_only_this_asks_about_that_row() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    send(&mut state, Message::RowPressed(path(CACHE_ROW)), &ctx);
    send(&mut state, Message::OpenMenuKey, &ctx);
    assert!(state.menu_open());
    send(&mut state, Message::MenuDown, &ctx);
    send(&mut state, Message::MenuDown, &ctx);
    send(&mut state, Message::MenuEnter, &ctx);
    assert!(!state.menu_open());
    let ask = state.confirm().unwrap();
    assert_eq!(ask.kind, ConfirmKind::One);
    assert_eq!(ask.safe.len(), 1);
    assert_eq!(ask.safe[0].path, path(CACHE_ROW));
}

#[test]
fn the_menu_entries_show_in_panel_and_ask_hermes_answer_with_actions() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    send(&mut state, Message::RowPressed(path(CACHE_ROW)), &ctx);
    let actions = send(&mut state, Message::ShowInPanelAtCursor, &ctx);
    assert_eq!(actions, [Action::ShowInPanel(path(CACHE_ROW))]);
    assert_eq!(
        send(&mut state, Message::AskHermes, &ctx),
        [Action::AskHermes]
    );
    assert_eq!(
        send(&mut state, Message::GrantHome, &ctx),
        [Action::GrantHome]
    );
}

fn open_downloads(state: &mut ncrs_space_view::State, ctx: &ncrs_space_view::Ctx) {
    send(
        state,
        Message::RowPressed(path(&format!("{HOME}/Downloads"))),
        ctx,
    );
    send(state, Message::Enter, ctx);
}

fn item(name: &str, bytes: u64) -> DownloadItem {
    DownloadItem {
        path: path(&format!("{HOME}/Downloads/{name}")),
        size: bytes,
        modified: None,
        touched: None,
        last_opened: None,
        open_count: None,
        is_folder: false,
    }
}

fn downloads() -> DownloadGroups {
    DownloadGroups {
        installed: vec![Matched {
            item: item("Setup.dmg", 3 * GIB),
            with: path("/Applications/Setup.app"),
            installed: None,
        }],
        untouched_year: vec![item("notes.pdf", GIB)],
        ..DownloadGroups::default()
    }
}

#[test]
fn enter_on_downloads_opens_the_pane_and_only_what_exists_elsewhere_is_chosen() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    open_downloads(&mut state, &ctx);
    assert!(state.decide().unwrap().is_downloads());
    send(
        &mut state,
        Message::Decide(DecideMessage::Loaded {
            folder: path(&format!("{HOME}/Downloads")),
            groups: Box::new(downloads()),
        }),
        &ctx,
    );
    let pane = state.decide().unwrap();
    assert_eq!(pane.chosen_count(), 1);
    assert!(pane.is_chosen(&path(&format!("{HOME}/Downloads/Setup.dmg"))));
    assert!(!pane.is_chosen(&path(&format!("{HOME}/Downloads/notes.pdf"))));
}

#[test]
fn the_list_keys_drive_the_pane_while_it_is_open_and_escape_goes_back() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    open_downloads(&mut state, &ctx);
    send(
        &mut state,
        Message::Decide(DecideMessage::Loaded {
            folder: path(&format!("{HOME}/Downloads")),
            groups: Box::new(downloads()),
        }),
        &ctx,
    );
    send(&mut state, Message::Down, &ctx);
    send(&mut state, Message::Down, &ctx);
    send(&mut state, Message::Down, &ctx);
    assert_eq!(
        state
            .decide()
            .unwrap()
            .cursor_item()
            .map(|entry| entry.item.path.clone()),
        Some(path(&format!("{HOME}/Downloads/notes.pdf")))
    );
    send(&mut state, Message::Toggle, &ctx);
    assert_eq!(state.decide().unwrap().chosen_count(), 2);
    send(&mut state, Message::DeleteSafe, &ctx);
    assert!(state.confirm().is_none(), "no delete from the pane");
    send(&mut state, Message::Escape, &ctx);
    assert!(state.decide().is_none());
}

#[test]
fn the_trash_gets_the_chosen_files_and_undo_asks_the_app_to_put_them_back() {
    let mut ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    open_downloads(&mut state, &ctx);
    send(
        &mut state,
        Message::Decide(DecideMessage::Loaded {
            folder: path(&format!("{HOME}/Downloads")),
            groups: Box::new(downloads()),
        }),
        &ctx,
    );
    let actions = send(&mut state, Message::Decide(DecideMessage::Trash), &ctx);
    let [Action::TrashOwn {
        root,
        items,
        empty_after,
    }] = actions.as_slice()
    else {
        panic!("one TrashOwn action, got {actions:?}");
    };
    assert_eq!(root, &path(&format!("{HOME}/Downloads")));
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].size, 3 * GIB);
    assert!(!empty_after);
    assert!(send(&mut state, Message::Decide(DecideMessage::Undo), &ctx).is_empty());
    send(
        &mut state,
        Message::Decide(DecideMessage::Trashed(vec![items[0].path.clone()])),
        &ctx,
    );
    let pane = state.decide().unwrap();
    assert_eq!(pane.chosen_count(), 0);
    ctx.can_undo_trash = true;
    assert_eq!(
        send(&mut state, Message::Decide(DecideMessage::Undo), &ctx),
        [Action::UndoTrash]
    );
}

#[test]
fn emptying_the_trash_afterwards_is_only_offered_where_the_trash_can_do_it() {
    let mut ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    open_downloads(&mut state, &ctx);
    send(
        &mut state,
        Message::Decide(DecideMessage::Loaded {
            folder: path(&format!("{HOME}/Downloads")),
            groups: Box::new(downloads()),
        }),
        &ctx,
    );
    send(
        &mut state,
        Message::Decide(DecideMessage::ToggleEmptyTrash),
        &ctx,
    );
    assert!(!state.decide().unwrap().empty_after());
    ctx.trash_can_purge = true;
    send(
        &mut state,
        Message::Decide(DecideMessage::ToggleEmptyTrash),
        &ctx,
    );
    assert!(state.decide().unwrap().empty_after());
    let actions = send(&mut state, Message::Decide(DecideMessage::Trash), &ctx);
    assert!(matches!(
        actions.as_slice(),
        [Action::TrashOwn {
            empty_after: true,
            ..
        }]
    ));
}

fn press(keys: &SpaceKeys, key: Key, modifiers: Modifiers) -> Option<Message> {
    space_key(keys, &key, modifiers, KeyMode::default(), None)
}

#[test]
fn every_key_of_the_tab_does_what_the_plan_says() {
    let keys = SpaceKeys::defaults_for(true);
    let command = Modifiers::LOGO;
    let letter = |text: &str| Key::Character(text.into());
    assert_eq!(
        press(&keys, Key::Named(Named::Enter), command),
        Some(Message::DeleteSafe)
    );
    assert_eq!(
        press(&keys, Key::Named(Named::Enter), command | Modifiers::SHIFT),
        Some(Message::RemoveWorktrees)
    );
    assert_eq!(press(&keys, letter("r"), command), Some(Message::Rescan));
    assert_eq!(
        press(&keys, letter("r"), command | Modifiers::SHIFT),
        Some(Message::Retry)
    );
    assert_eq!(
        press(&keys, Key::Named(Named::F2), Modifiers::empty()),
        Some(Message::AskHermes)
    );
    assert_eq!(
        press(&keys, letter("f"), command | Modifiers::ALT),
        Some(Message::ShowInPanelAtCursor)
    );
    assert_eq!(
        press(&keys, Key::Named(Named::F8), Modifiers::empty()),
        Some(Message::DeleteOnlyThis)
    );
    assert_eq!(
        press(&keys, letter("/"), Modifiers::empty()),
        Some(Message::FocusFilter)
    );
    let plain = |named| press(&keys, Key::Named(named), Modifiers::empty());
    assert_eq!(plain(Named::Space), Some(Message::Toggle));
    assert_eq!(plain(Named::Enter), Some(Message::Enter));
    assert_eq!(plain(Named::ArrowLeft), Some(Message::Left));
    assert_eq!(plain(Named::ArrowRight), Some(Message::Right));
    assert_eq!(plain(Named::ArrowUp), Some(Message::Up));
    assert_eq!(plain(Named::ArrowDown), Some(Message::Down));
    assert_eq!(plain(Named::PageUp), Some(Message::PageUp));
    assert_eq!(plain(Named::PageDown), Some(Message::PageDown));
    assert_eq!(plain(Named::Home), Some(Message::First));
    assert_eq!(plain(Named::End), Some(Message::Last));
}

#[test]
fn the_context_menu_key_of_the_app_opens_the_row_menu_and_a_dialog_takes_the_keys() {
    let keys = SpaceKeys::defaults_for(true);
    let tap = space_key(
        &keys,
        &Key::Named(Named::ContextMenu),
        Modifiers::empty(),
        KeyMode::default(),
        Some(ncrs_space_view::AppKey::ContextMenu),
    );
    assert_eq!(tap, Some(Message::OpenMenuKey));
    let in_dialog = KeyMode {
        confirm_open: true,
        ..KeyMode::default()
    };
    assert_eq!(
        space_key(
            &keys,
            &Key::Named(Named::Tab),
            Modifiers::empty(),
            in_dialog,
            None
        ),
        Some(Message::ConfirmSwitchFocus)
    );
    assert_eq!(
        space_key(
            &keys,
            &Key::Named(Named::ArrowDown),
            Modifiers::empty(),
            in_dialog,
            None
        ),
        None
    );
}

#[test]
fn the_command_list_has_every_action_and_each_answers_with_its_message() {
    let keys = SpaceKeys::defaults_for(true);
    let listed = commands(ncrs_i18n::Language::English, &keys);
    assert!(listed.len() >= 8);
    assert!(listed.iter().all(|(label, _)| !label.is_empty()));
    assert_eq!(message_of_command(0), Some(Message::DeleteSafe));
    assert_eq!(message_of_command(listed.len()), None);
}

#[test]
fn the_simulators_row_cannot_go_to_the_trash_but_the_backups_row_can() {
    let simulators = format!("{HOME}/Library/Developer/CoreSimulator");
    let backups = format!("{HOME}/Backups");
    let place =
        |folder: &str, kind: DecideKind| entry(folder, Group::Decide, EntryKind::Decide(kind), GIB);
    let scan = scan_of(
        vec![
            place(&simulators, DecideKind::Simulators),
            place(&backups, DecideKind::Backups),
        ],
        true,
    );
    let app = ctx();
    let (mut state, _) = tab(scan, &app);
    assert!(!row_of(&state, &simulators).can_trash());
    assert!(row_of(&state, &backups).can_trash());

    send(&mut state, Message::RowPressed(path(&simulators)), &app);
    send(&mut state, Message::Enter, &app);
    assert!(state.decide().is_none());

    send(&mut state, Message::RowPressed(path(&backups)), &app);
    send(&mut state, Message::Enter, &app);
    assert!(state.decide().is_some());
}

#[test]
fn the_side_overview_does_not_open_the_simulators_either() {
    let simulators = format!("{HOME}/Library/Developer/CoreSimulator");
    let backups = format!("{HOME}/Backups");
    let place =
        |folder: &str, kind: DecideKind| entry(folder, Group::Decide, EntryKind::Decide(kind), GIB);
    let scan = scan_of(
        vec![
            place(&simulators, DecideKind::Simulators),
            place(&backups, DecideKind::Backups),
        ],
        true,
    );
    let app = ctx();
    let (mut state, _) = tab(scan, &app);
    send(&mut state, Message::RowPressed(path(&backups)), &app);
    send(&mut state, Message::Enter, &app);
    send(
        &mut state,
        Message::Decide(DecideMessage::SidePressed(path(&simulators))),
        &app,
    );
    assert_eq!(state.decide().unwrap().folder(), path(&backups));
}

#[test]
fn a_second_trash_press_waits_for_the_answer_of_the_first() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    open_downloads(&mut state, &ctx);
    send(
        &mut state,
        Message::Decide(DecideMessage::Loaded {
            folder: path(&format!("{HOME}/Downloads")),
            groups: Box::new(downloads()),
        }),
        &ctx,
    );
    let first = send(&mut state, Message::Decide(DecideMessage::Trash), &ctx);
    assert_eq!(first.len(), 1);
    assert!(send(&mut state, Message::Decide(DecideMessage::Trash), &ctx).is_empty());
    send(
        &mut state,
        Message::Decide(DecideMessage::Trashed(Vec::new())),
        &ctx,
    );
    assert!(!state.decide().unwrap().is_trashing());
}

#[test]
fn free_now_is_the_free_space_after_the_run_while_the_measure_runs() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    finished_run(&mut state, &ctx);
    assert!(state.is_measuring());
    assert_eq!(state.volume_now().unwrap().available, 118 * GIB);
}

#[test]
fn a_second_run_records_the_free_space_of_the_newer_measure_as_before() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    finished_run(&mut state, &ctx);
    let newer = scan_with_free(118 * GIB);
    let run = state.scan_run();
    send(
        &mut state,
        Message::Scanned {
            run,
            scan: Some(newer),
        },
        &ctx,
    );
    send(&mut state, Message::Retry, &ctx);
    send(&mut state, Message::ConfirmAccept, &ctx);
    assert_eq!(state.running().unwrap().free_before, 118 * GIB);
}

fn scan_with_free(free: u64) -> Arc<ncrs_space::Scan> {
    let mut scan = (*small_scan()).clone();
    scan.volume.available = free;
    Arc::new(scan)
}

#[test]
fn open_decide_waits_for_the_measure_and_then_opens_the_downloads() {
    let ctx = ctx();
    let (mut state, _task) = ncrs_space_view::open(&ctx);
    send(
        &mut state,
        Message::OpenDecide(ncrs_space_view::DecideTarget::Downloads),
        &ctx,
    );
    assert!(state.decide().is_none());
    let run = state.scan_run();
    send(
        &mut state,
        Message::Scanned {
            run,
            scan: Some(small_scan()),
        },
        &ctx,
    );
    assert_eq!(
        state.decide().unwrap().folder(),
        path(&format!("{HOME}/Downloads"))
    );
}

#[test]
fn open_decide_settles_the_result_banner_and_opens_the_pane() {
    let ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    finished_run(&mut state, &ctx);
    let run = state.scan_run();
    send(
        &mut state,
        Message::Scanned {
            run,
            scan: Some(small_scan()),
        },
        &ctx,
    );
    assert!(state.result().is_some());
    send(
        &mut state,
        Message::OpenDecide(ncrs_space_view::DecideTarget::Downloads),
        &ctx,
    );
    assert!(state.result().is_none());
    assert!(state.decide().is_some());
}

#[test]
fn undo_follows_what_the_app_can_bring_back_not_the_pane() {
    let mut ctx = ctx();
    let (mut state, _) = tab(small_scan(), &ctx);
    open_downloads(&mut state, &ctx);
    send(
        &mut state,
        Message::Decide(DecideMessage::Loaded {
            folder: path(&format!("{HOME}/Downloads")),
            groups: Box::new(downloads()),
        }),
        &ctx,
    );
    assert!(send(&mut state, Message::Decide(DecideMessage::Undo), &ctx).is_empty());
    ctx.can_undo_trash = true;
    assert_eq!(
        send(&mut state, Message::Decide(DecideMessage::Undo), &ctx),
        [Action::UndoTrash]
    );
    send(&mut state, Message::Escape, &ctx);
    open_downloads(&mut state, &ctx);
    assert_eq!(
        send(&mut state, Message::Decide(DecideMessage::Undo), &ctx),
        [Action::UndoTrash],
        "a reopened pane still offers the undo"
    );
}
