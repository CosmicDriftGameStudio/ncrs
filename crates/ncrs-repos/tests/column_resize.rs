//! Dragging the head of a list column: press, move, release.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod common;

use common::fixture;
use ncrs_config::repos::ReposColumnWidths;
use ncrs_repos::{Action, Message};
use ncrs_ui_kit::repo_row::{RepoColumn, RepoColumns, REPO_COLUMN_MIN};

#[test]
fn dragging_the_name_edge_right_widens_it_and_reports_the_width_once() {
    let mut fixture = fixture(&["alpha"], 0);
    fixture.send(Message::ResizePressed(RepoColumn::Name));
    assert!(fixture.send(Message::ResizeMoved(400.0)).is_empty());
    assert!(fixture.send(Message::ResizeMoved(450.0)).is_empty());

    let actions = fixture.send(Message::ResizeReleased);

    let wanted = ReposColumnWidths {
        name: Some(310),
        ..ReposColumnWidths::default()
    };
    assert_eq!(actions, vec![Action::ColumnsResized(wanted)]);
    assert!(fixture.send(Message::ResizeReleased).is_empty());
}

#[test]
fn a_column_behind_the_location_grows_when_its_left_edge_moves_left() {
    let mut fixture = fixture(&["alpha"], 0);
    fixture.ctx.settings.columns.branch = Some(200);
    fixture.send(Message::ResizePressed(RepoColumn::Branch));
    fixture.send(Message::ResizeMoved(900.0));
    fixture.send(Message::ResizeMoved(860.0));

    let actions = fixture.send(Message::ResizeReleased);

    let Some(Action::ColumnsResized(widths)) = actions.into_iter().next() else {
        panic!("no widths reported");
    };
    assert_eq!(widths.branch, Some(240));
    assert_eq!(widths.name, None, "what was not dragged stays unremembered");
}

#[test]
fn a_column_stops_at_the_minimum() {
    let mut fixture = fixture(&["alpha"], 0);
    fixture.send(Message::ResizePressed(RepoColumn::Name));
    fixture.send(Message::ResizeMoved(500.0));
    fixture.send(Message::ResizeMoved(-5000.0));

    let actions = fixture.send(Message::ResizeReleased);

    let Some(Action::ColumnsResized(widths)) = actions.into_iter().next() else {
        panic!("no widths reported");
    };
    assert_eq!(widths.name, Some(REPO_COLUMN_MIN as u16));
}

#[test]
fn a_press_without_a_move_reports_nothing() {
    let mut fixture = fixture(&["alpha"], 0);
    fixture.send(Message::ResizePressed(RepoColumn::Standing));
    assert!(fixture.send(Message::ResizeReleased).is_empty());
}

#[test]
fn the_kit_clamps_dragged_widths() {
    let wide = RepoColumns::STANDARD.dragged(RepoColumn::Name, 10_000.0);
    assert_eq!(wide.name, ncrs_ui_kit::repo_row::REPO_COLUMN_MAX);
}
