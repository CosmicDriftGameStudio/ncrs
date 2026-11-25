//! The header picker with its list, the state chips, the ref labels, the
//! work-in-progress row and the worktree line.

use iced::widget::{column, container, row, Column};
use iced::{Element, Length, Size};
use ncrs_i18n::Language;

use super::matches_picture;
use crate::commit_graph::{graph_cell, GraphCell};
use crate::git_lines::{
    mark_head, summarize_refs, wip_row, worktree_line, WipRow, WorktreeLine, WorktreeState,
};
use crate::git_rows::{commit_row, ref_chip, CommitColumns, CommitRow, RefChipKind};
use crate::git_state::{outcome_mark, standing_chip, RunOutcome, Standing};
use crate::header_picker::{
    filter_rows, header_picker, picker_list, picker_separator, PickerLine, PickerMark, PickerRow,
};
use crate::kit;
use crate::palette::{Palette, ThemePreset};
use crate::theme::git::PickerLook;
use crate::theme::spacing;

type Cell<'a> = Element<'a, ()>;

const LANGUAGE: Language = Language::English;

fn palette() -> Palette {
    ThemePreset::Dracula.palette()
}

#[test]
fn the_header_pickers_look_the_same() {
    let p = palette();
    let picked = |look: PickerLook, repo_label: &str, branch: &str| -> Cell<'_> {
        row![
            header_picker("Repository", repo_label, look, (), &p),
            picker_separator(&p),
            header_picker("Branch", branch, PickerLook::Idle, (), &p),
        ]
        .spacing(spacing::XS)
        .align_y(iced::alignment::Vertical::Center)
        .into()
    };
    let open: Cell<'_> = row![
        header_picker("Repository", "ncrs", PickerLook::Idle, (), &p),
        picker_separator(&p),
        header_picker("Branch", "main", PickerLook::Open, (), &p),
    ]
    .spacing(spacing::XS)
    .align_y(iced::alignment::Vertical::Center)
    .into();
    let content = column![
        kit::dim("idle".to_owned(), &p),
        picked(PickerLook::Idle, "ncrs", "main"),
        kit::dim("focus".to_owned(), &p),
        kit::focused(
            picked(PickerLook::Idle, "kumiko-studio-framework", "feat/git-tab"),
            true,
            &p
        ),
        kit::dim("open".to_owned(), &p),
        open,
    ]
    .spacing(spacing::M)
    .padding(spacing::XXL);
    matches_picture("header-picker", Size::new(560.0, 330.0), content.into());
}

fn branch(label: &str, trail: Option<&str>, current: bool, highlighted: bool) -> PickerRow<()> {
    PickerRow {
        mark: if current {
            PickerMark::Current
        } else {
            PickerMark::None
        },
        label: label.to_owned(),
        trail: trail.map(str::to_owned),
        key: None,
        current,
        highlighted,
        on_press: (),
    }
}

#[test]
fn the_open_picker_list_looks_the_same() {
    let p = palette();
    let local = vec![
        branch("main", Some("\u{2193}2"), true, false),
        branch("release/1.7.1", None, false, false),
        branch("feat/git-tab", Some("\u{2191}3"), false, false),
        branch("fix/filing-plan", None, false, false),
    ];
    let remote = vec![
        branch("origin/site/1.5.0", None, false, false),
        branch("origin/feat/hero-trailer", None, false, false),
        branch("origin/release/1.7.1", None, false, false),
    ];
    let all: Vec<PickerRow<()>> = local.into_iter().chain(remote).collect();
    let shown = filter_rows(all, "", 6);
    let (local_rows, remote_rows) = shown.split_at(4);
    let new_branch = PickerRow {
        mark: PickerMark::Action,
        label: "New branch\u{2026}".to_owned(),
        trail: None,
        key: Some("\u{21e7}\u{2318}T".to_owned()),
        current: false,
        highlighted: false,
        on_press: (),
    };
    let lines: Vec<PickerLine<()>> = [PickerLine::Group("Local".to_owned())]
        .into_iter()
        .chain(local_rows.iter().cloned().map(PickerLine::Row))
        .chain([PickerLine::Group("Remote".to_owned())])
        .chain(remote_rows.iter().cloned().map(PickerLine::Row))
        .chain([PickerLine::Rule, PickerLine::Row(new_branch)])
        .collect();
    let search = kit::text_field(
        "Search branches\u{2026}",
        "",
        "gallery-picker-search",
        None::<fn(String) -> ()>,
        (),
        &p,
    );
    let list = picker_list(
        search,
        lines,
        &[
            ("Enter", "switches"),
            ("\u{2318}Enter", "switches with a stash"),
        ],
        420.0,
        &p,
    );
    let content: Cell<'_> = container(list).padding(spacing::XXL).into();
    matches_picture("picker-list", Size::new(520.0, 520.0), content);
}

#[test]
fn the_ref_labels_look_the_same() {
    let p = palette();
    let chips = |items: &[(&str, RefChipKind)]| -> Cell<'static> {
        let shown = summarize_refs(
            items
                .iter()
                .map(|(label, kind)| ((*label).to_owned(), *kind))
                .collect(),
        );
        chips_in_row(
            shown
                .into_iter()
                .map(|(label, kind)| ref_chip(label, kind, &p)),
        )
    };
    let lonely: Cell<'_> = chips(&[
        ("main", RefChipKind::Head),
        ("release/1.7.1", RefChipKind::Branch),
        ("origin/hotfix", RefChipKind::Remote),
        ("v1.7.1", RefChipKind::Tag),
    ]);
    let merged: Cell<'_> = chips(&[
        ("main", RefChipKind::Head),
        ("origin/main", RefChipKind::Remote),
    ]);
    let marked: Vec<(String, RefChipKind)> = mark_head(
        vec![("feat/a".to_owned(), RefChipKind::Branch)],
        Some("feat/a"),
    );
    assert_eq!(marked.first().map(|chip| chip.1), Some(RefChipKind::Head));
    let columns = CommitColumns::STANDARD;
    let commit = |head: bool, message: &str, kind: RefChipKind, label: &str| -> Cell<'_> {
        commit_row(
            CommitRow {
                graph: graph_cell(
                    GraphCell {
                        lane: 0,
                        lines: Vec::new(),
                        merge: false,
                        dimmed: false,
                    },
                    1,
                    &p,
                ),
                chips: vec![(label.to_owned(), kind)],
                message: message.to_owned(),
                author: "Marc".to_owned(),
                date: "2026-11-24".to_owned(),
                sha: "584a5b6".to_owned(),
                columns,
                message_width: 330.0,
                show_sha: true,
                chosen: false,
                dimmed: false,
                is_head: head,
                on_press: (),
            },
            &p,
        )
    };
    let content = column![
        kit::dim("head, local, remote, tag".to_owned(), &p),
        lonely,
        kit::dim("local and remote on one commit".to_owned(), &p),
        merged,
        kit::dim("the row of the checked-out commit".to_owned(), &p),
        commit(true, "Prepare release 1.7.1", RefChipKind::Head, "main"),
        commit(
            false,
            "Keep the decoded folder list",
            RefChipKind::Branch,
            "fix/folders"
        ),
    ]
    .spacing(spacing::M)
    .padding(spacing::XXL);
    matches_picture("ref-labels", Size::new(760.0, 260.0), content.into());
}

fn chips_in_row<'a>(children: impl Iterator<Item = Cell<'a>>) -> Cell<'a> {
    iced::widget::Row::with_children(children)
        .spacing(spacing::XS)
        .into()
}

#[test]
fn the_work_in_progress_row_looks_the_same() {
    let p = palette();
    let columns = CommitColumns::STANDARD;
    let wip = |changed: u32, staged: u32, chosen: bool| -> Cell<'_> {
        wip_row(
            WipRow {
                changed,
                staged,
                chosen,
                lanes: 2,
                columns,
                show_sha: true,
                on_press: (),
            },
            LANGUAGE,
            &p,
        )
    };
    let content = column![wip(1, 0, false), wip(12, 5, false), wip(3, 3, true)]
        .spacing(spacing::XS)
        .padding(spacing::XXL)
        .width(Length::Fill);
    matches_picture("wip-row", Size::new(760.0, 140.0), content.into());
}

#[test]
fn the_worktree_lines_look_the_same() {
    let p = palette();
    let line = |state: WorktreeState,
                branch: &str,
                folder: &str,
                (ahead, behind, changes): (u32, u32, u32),
                chosen: bool|
     -> Cell<'_> {
        worktree_line(
            WorktreeLine {
                state,
                branch: branch.to_owned(),
                folder: folder.to_owned(),
                ahead,
                behind,
                changes,
                chosen,
                on_press: (),
            },
            LANGUAGE,
            &p,
        )
    };
    let lines = Column::new()
        .push(line(
            WorktreeState::Current,
            "main",
            "ncrs",
            (0, 2, 1),
            true,
        ))
        .push(line(
            WorktreeState::Current,
            "main",
            "ncrs",
            (0, 2, 1),
            false,
        ))
        .push(line(
            WorktreeState::Other,
            "feat/git-tab",
            "ncrs-git-tab",
            (3, 0, 0),
            false,
        ))
        .push(line(
            WorktreeState::Other,
            "feat/dirty",
            "ncrs-dirty",
            (1, 4, 12),
            false,
        ))
        .push(line(
            WorktreeState::Detached,
            "9aeef74",
            "ncrs-probe",
            (0, 0, 0),
            false,
        ))
        .push(line(
            WorktreeState::Merged,
            "fix/filing-plan",
            "ncrs-fix",
            (0, 0, 0),
            false,
        ))
        .width(Length::Fill);
    let content: Cell<'_> = container(lines).padding(spacing::XXL).width(520.0).into();
    matches_picture("worktree-line", Size::new(600.0, 240.0), content);
}

#[test]
fn the_state_chips_look_the_same() {
    let p = palette();
    let standings = [
        Standing::Behind {
            count: 2,
            upstream: "origin/main".to_owned(),
        },
        Standing::Ahead { count: 3 },
        Standing::Changes { count: 1 },
        Standing::Changes { count: 12 },
        Standing::Conflicts { count: 2 },
        Standing::UpToDate,
    ];
    let clickable: Cell<'_> = chips_in_row(
        standings
            .iter()
            .map(|standing| standing_chip(standing, LANGUAGE, &p, Some(()))),
    );
    let labels: Cell<'_> = chips_in_row(
        standings
            .iter()
            .map(|standing| standing_chip(standing, Language::German, &p, None)),
    );
    let outcomes: Cell<'_> = chips_in_row(
        RunOutcome::ALL
            .iter()
            .map(|outcome| outcome_mark(*outcome, LANGUAGE, &p)),
    );
    let content = column![
        kit::dim("buttons".to_owned(), &p),
        clickable,
        kit::dim("labels, German".to_owned(), &p),
        labels,
        kit::dim("run results".to_owned(), &p),
        outcomes,
    ]
    .spacing(spacing::M)
    .padding(spacing::XXL);
    matches_picture("state-chips", Size::new(1000.0, 220.0), content.into());
}
