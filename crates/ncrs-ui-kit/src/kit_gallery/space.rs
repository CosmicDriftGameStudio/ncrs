//! The building blocks of the Platz tab: the bar of the drive, the size row,
//! the colored chip, the confirm dialog, the meter by level and the keyed chip.

use iced::widget::{column, row};
use iced::{Element, Size};
use ncrs_i18n::Language;

use super::matches_picture;
use crate::color_chip::color_chip;
use crate::confirm::{confirm_dialog, Confirm, ConfirmButton, ConfirmFocus};
use crate::git_state::RunOutcome;
use crate::hermes_kit::{chat_chip, chat_chip_keyed};
use crate::kit::{self, Check, Tone};
use crate::meter::{meter_by_level, meter_level_from, Meter};
use crate::palette::{Palette, ThemePreset};
use crate::size_row::{size_row, SizeRow};
use crate::stacked_bar::{stacked_bar, BarSegment};
use crate::theme::{self, spacing};

type Cell<'a> = Element<'a, ()>;

fn palette() -> Palette {
    ThemePreset::Dracula.palette()
}

#[test]
fn the_bar_of_the_drive_and_its_chips_look_the_same() {
    let p = palette();
    let segment = |portion: u64, color| BarSegment { portion, color };
    let bar = stacked_bar(
        &[
            segment(134, theme::space::safe(&p)),
            segment(22, theme::space::worktrees()),
            segment(139, theme::space::decide()),
            segment(5, theme::space::in_use(&p)),
            segment(514, theme::space::rest(&p)),
            segment(78, theme::space::free(&p)),
        ],
        &p,
    );
    let chips: Cell<'_> = row![
        color_chip("safe 134 GB", theme::space::safe(&p), None),
        color_chip("worktrees 22 GB", theme::space::worktrees(), None),
        color_chip("decide 139 GB", theme::space::decide(), Some(())),
        color_chip("in use 5 GB", theme::space::in_use(&p), None),
    ]
    .spacing(spacing::XS)
    .into();
    let content = column![bar, chips]
        .spacing(spacing::L)
        .padding(spacing::XXL);
    matches_picture("space-bar", Size::new(900.0, 120.0), content.into());
}

#[test]
fn the_size_rows_look_the_same() {
    let p = palette();
    let item = |name: &str, fraction: f32, check, cursor, outcome| -> Cell<'_> {
        size_row(
            SizeRow {
                check,
                name: name.to_owned(),
                path: "~/.cargo-target/ncrs-a, ncrs-eval, ncrs-fix-rest-a".to_owned(),
                size_text: "11 GB".to_owned(),
                fraction,
                color: theme::space::safe(&p),
                reason: "Rust build, worktrees merged".to_owned(),
                outcome,
                cursor,
                on_press: Some(()),
                on_toggle: Some(()),
            },
            &p,
        )
    };
    let content = column![
        item(
            "Rust builds of old worktrees",
            1.0,
            Some(Check::On),
            false,
            None
        ),
        item("Yarn cache", 0.4, Some(Check::Off), false, None),
        item("DerivedData", 0.02, Some(Check::On), true, None),
        item("npm cache", 0.1, None, false, None),
        item(
            "Gradle cache",
            0.3,
            None,
            false,
            Some((RunOutcome::Updated, "freed 3.1 GB".to_owned())),
        ),
        item(
            "sccache",
            0.2,
            None,
            false,
            Some((RunOutcome::Skipped, "a build is running".to_owned())),
        ),
    ];
    matches_picture("space-size-rows", Size::new(1000.0, 260.0), content.into());
}

#[test]
fn the_confirm_dialog_and_the_keyed_chip_look_the_same() {
    let p = palette();
    let list: Cell<'_> = kit::name_list(
        vec!["Yarn cache".to_owned(), "DerivedData".to_owned()],
        Some("and 4 more".to_owned()),
        None,
        &p,
    );
    let dialog = confirm_dialog(
        Confirm {
            title: "Delete 6 folders for good?".to_owned(),
            body: "These are made again by their tools. There is no undo.".to_owned(),
            list: Some(list),
            cancel: ConfirmButton {
                label: "Cancel",
                key: Some("Esc"),
            },
            confirm: ConfirmButton {
                label: "Delete for good",
                key: Some("Mod+Enter"),
            },
            tone: Tone::Danger,
            focus: ConfirmFocus::Cancel,
            on_cancel: (),
            on_confirm: (),
            window: Size::new(700.0, 420.0),
            lang: Language::English,
        },
        &p,
    );
    matches_picture("space-confirm", Size::new(700.0, 420.0), dialog);
}

#[test]
fn the_meter_by_level_and_the_keyed_chips_look_the_same() {
    let p = palette();
    let gauge = |percent: f32| -> Cell<'_> {
        meter_by_level(
            Meter {
                title: "Drive",
                value: "",
                fraction: percent / 100.0,
                level: meter_level_from(percent, 90.0),
                detail: "of 926 GB",
            },
            &p,
        )
    };
    let chips: Cell<'_> = row![
        chat_chip("Measure", false, (), &p),
        chat_chip_keyed("Delete all safe ones", true, Some("Mod+Enter"), (), &p),
    ]
    .spacing(spacing::M)
    .into();
    let content = column![gauge(62.0), gauge(91.0), chips]
        .spacing(spacing::L)
        .padding(spacing::XXL);
    matches_picture("space-meter-chips", Size::new(700.0, 220.0), content.into());
}

#[test]
fn the_space_card_looks_the_same() {
    use crate::hermes_kit::{space_card, SpaceCardRow, SpaceCardView};

    let p = palette();
    let segment = |portion: u64, color| BarSegment { portion, color };
    let row = |name: &str, path: &str, size: &str, group: &str, color, hint: &str| SpaceCardRow {
        name: name.to_owned(),
        path: path.to_owned(),
        size: size.to_owned(),
        group: group.to_owned(),
        group_color: color,
        hint: hint.to_owned(),
    };
    let view = SpaceCardView {
        title: "Where the space is".to_owned(),
        drive: "Macintosh HD · 926 GB".to_owned(),
        bar: vec![
            segment(134, theme::space::safe(&p)),
            segment(22, theme::space::worktrees()),
            segment(139, theme::space::decide()),
            segment(5, theme::space::in_use(&p)),
            segment(514, theme::space::rest(&p)),
            segment(78, theme::space::free(&p)),
        ],
        legend: vec![
            ("safe 134 GB".to_owned(), theme::space::safe(&p)),
            ("worktrees 22 GB".to_owned(), theme::space::worktrees()),
            ("you decide 139 GB".to_owned(), theme::space::decide()),
        ],
        after: Some("After \"all safe ones\": 213 GB free, 77 % used".to_owned()),
        columns: [
            "WHAT".to_owned(),
            "SIZE".to_owned(),
            "GROUP".to_owned(),
            "HINT".to_owned(),
        ],
        rows: vec![
            row(
                "Rust builds of old worktrees",
                "~/.cargo-target",
                "44 GB",
                "Safe",
                theme::space::safe(&p),
                "Rust build, worktrees merged",
            ),
            row(
                "Backups",
                "~/Backups",
                "46 GB",
                "You decide",
                theme::space::decide(),
                "Old backups; only you know if you still need them",
            ),
        ],
        also: Some("Also: 3 more Rust builds 11 GB (safe)".to_owned()),
        footer: "Safe means: made again when needed. Nothing is deleted before your OK.".to_owned(),
    };
    let card: Cell<'_> = space_card(&view, &p);
    matches_picture("space-card", Size::new(900.0, 360.0), card);
}
