//! The pane in which the user decides: column titles, group heads, file rows, the "more" row
//! and the narrow overview.

use iced::widget::{column, row};
use iced::{Element, Size};

use super::matches_picture;
use crate::decide_row::{
    file_columns_header, file_row, group_head, more_row, side_head, side_row, FileRow, GroupHead,
    SideLook, SideRow,
};
use crate::kit::Check;
use crate::palette::{Palette, ThemePreset};
use crate::theme;

type Cell<'a> = Element<'a, ()>;

fn palette() -> Palette {
    ThemePreset::Dracula.palette()
}

#[test]
fn the_decide_list_looks_the_same() {
    let p = palette();
    let head = |title: &str, hint: &str, check, dimmed, focused, open| -> Cell<'_> {
        group_head(
            GroupHead {
                title: title.to_owned(),
                hint: hint.to_owned(),
                count: 12,
                size_text: "6,4 GB".to_owned(),
                fraction: 0.6,
                color: theme::space::decide(),
                open,
                check,
                dimmed,
                focused,
                on_toggle: (),
                on_check: (),
            },
            &p,
        )
    };
    let file = |name: &str, on: bool, cursor: bool, focused: bool| -> Cell<'_> {
        file_row(
            FileRow {
                check: Some(if on { Check::On } else { Check::Off }),
                name: name.to_owned(),
                reason: "Liegt schon in ~/Documents".to_owned(),
                date: "2026-08-30".to_owned(),
                size_text: "2,1 GB".to_owned(),
                fraction: 0.4,
                color: theme::space::decide(),
                dimmed: !on,
                cursor,
                focused,
                on_press: Some(()),
                on_toggle: Some(()),
            },
            &p,
        )
    };
    let content = column![
        file_columns_header(["Name", "Grund", "Geändert", "Größe"], &p),
        head(
            "Archive, schon entpackt",
            "Inhalt liegt daneben",
            Check::On,
            false,
            false,
            true
        ),
        file("qwen2.5-7b-gguf.zip", true, true, true),
        file("Brother-Scans-2025-09.zip", true, false, false),
        more_row(
            "5 weitere".to_owned(),
            "1,1 GB".to_owned(),
            false,
            false,
            (),
            &p
        ),
        head(
            "Lange nicht geöffnet",
            "du wählst",
            Check::Mixed,
            false,
            true,
            true
        ),
        file("ubuntu-24.04-desktop-arm64.iso", false, false, false),
        head("Rest", "bleibt liegen", Check::Off, true, false, false),
    ];
    matches_picture("decide-list", Size::new(1000.0, 260.0), content.into());
}

#[test]
fn the_decide_overview_looks_the_same() {
    let p = palette();
    let side = |name: &str, look, focused| -> Cell<'_> {
        side_row(
            SideRow {
                name: name.to_owned(),
                size_text: "35 GB".to_owned(),
                fraction: 0.5,
                color: theme::space::decide(),
                look,
                focused,
                on_press: Some(()),
            },
            &p,
        )
    };
    let content = row![column![
        side_head("Sicher", "134 GB".to_owned(), &p),
        side("Rust-Build ncrs", SideLook::Normal, false),
        side_head("Du entscheidest", "139 GB".to_owned(), &p),
        side("~/Downloads", SideLook::Current, true),
        side("Backups", SideLook::Normal, false),
        side_head("In Benutzung", "5 GB".to_owned(), &p),
        side("Browser-Caches Google", SideLook::Dim, false),
    ]
    .width(300.0)];
    matches_picture("decide-side", Size::new(300.0, 230.0), content.into());
}
