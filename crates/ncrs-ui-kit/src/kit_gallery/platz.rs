//! The pieces of the Platz tab beyond the plain size row: the lines of the confirm dialog,
//! the card of the rules page, and the size row with check, chip, gone look and button.

use iced::widget::column;
use iced::{Element, Size};

use super::matches_picture;
use crate::confirm_list::{confirm_list, ConfirmLine, ConfirmSum};
use crate::git_state::RunOutcome;
use crate::info_card::{card_note, card_text, dot_line, info_card, key_row};
use crate::kit::Check;
use crate::palette::{Palette, ThemePreset};
use crate::size_row::{
    size_row_ext, size_row_header, CheckTone, SizeRow, SizeRowExt, SizeRowHeader,
};
use crate::theme;

type Cell<'a> = Element<'a, ()>;

fn palette() -> Palette {
    ThemePreset::Dracula.palette()
}

#[test]
fn the_confirm_list_keeps_its_columns_apart() {
    let p = palette();
    let lines = (0..4_u64)
        .map(|place| ConfirmLine {
            name: format!("Rust build ncrs-{place}"),
            path: format!("~/.cargo-target/ncrs-{place}"),
            check: Some("no cargo/rustc is running".to_owned()),
            size_text: format!("{}.{} GB", 9 - place, place),
            fraction: 1.0 - place as f32 / 5.0,
            color: theme::space::safe(&p),
        })
        .collect();
    let list: Cell<'_> = confirm_list(
        lines,
        Some(ConfirmSum {
            label: "Total".to_owned(),
            size_text: "30.6 GB".to_owned(),
        }),
        160.0,
        &p,
    );
    matches_picture("platz-confirm-list", Size::new(760.0, 260.0), list);
}

#[test]
fn the_folded_row_names_what_it_hides_and_the_key() {
    let p = palette();
    let folded: Cell<'_> = crate::folded_row::folded_row(
        "31 more merged worktrees \u{b7} 19.9 GB",
        "\u{2192}",
        "expand",
        (),
        &p,
    );
    matches_picture("platz-folded-row", Size::new(700.0, 40.0), folded);
}

#[test]
fn the_info_card_shows_title_lines_and_keys() {
    let p = palette();
    let card: Cell<'_> = info_card(
        "Safe (deleted by \"Delete all safe ones\")",
        Some(theme::space::safe(&p)),
        vec![
            card_text("Safe means: a tool makes it again.", &p),
            dot_line(card_text("target folders of Rust projects", &p), &p),
            key_row("Space", "Entry on/off", &p),
            card_note("What is not in the catalog is never safe.", &p),
        ],
        &p,
    );
    matches_picture("platz-info-card", Size::new(620.0, 240.0), card);
}

#[test]
fn the_size_row_has_check_chip_gone_look_and_button() {
    let p = palette();
    let plain = |name: &str, fraction: f32| SizeRow {
        check: Some(Check::On),
        name: name.to_owned(),
        path: "~/.cargo-target/ncrs".to_owned(),
        size_text: "8.9 GB".to_owned(),
        fraction,
        color: theme::space::safe(&p),
        reason: "Rust build".to_owned(),
        outcome: None,
        cursor: false,
        on_press: None,
        on_toggle: None,
    };
    let header: Cell<'_> = size_row_header(
        SizeRowHeader {
            bar: "SIZE".to_owned(),
            name: "FOLDER".to_owned(),
            reason: "WHAT IT IS".to_owned(),
            check: Some("CHECK".to_owned()),
            size: "USED".to_owned(),
            action_column: true,
        },
        &p,
    );
    let checked = size_row_ext(
        plain("Rust build ncrs-1", 0.9),
        SizeRowExt {
            check_column: true,
            action_column: true,
            check: Some((CheckTone::Ok, "no cargo/rustc is running".to_owned())),
            chip: Some("feat/branch".to_owned()),
            ..SizeRowExt::default()
        },
        &p,
    );
    let gone = size_row_ext(
        plain("Rust build ncrs-2", 0.6),
        SizeRowExt {
            check_column: true,
            action_column: true,
            gone: true,
            ..SizeRowExt::default()
        },
        &p,
    );
    let mut skipped_row = plain("Rust build ncrs-3", 0.4);
    skipped_row.check = None;
    skipped_row.outcome = Some((RunOutcome::Failed, "skipped: in use".to_owned()));
    let skipped = size_row_ext(
        skipped_row,
        SizeRowExt {
            check_column: true,
            action_column: true,
            action: Some(("Try again".to_owned(), ())),
            ..SizeRowExt::default()
        },
        &p,
    );
    matches_picture(
        "platz-size-row-ext",
        Size::new(1100.0, 220.0),
        column![header, checked, gone, skipped].into(),
    );
}
