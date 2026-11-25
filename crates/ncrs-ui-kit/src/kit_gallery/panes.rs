//! The splitter, the section head, the toolbar and the menu with a submenu.

use iced::widget::{column, container, row, stack, text, Space};
use iced::{alignment, Element, Length, Point, Size};
use ncrs_i18n::Language;

use super::matches_picture;
use crate::kit::{self, Tone};
use crate::layout::MENU_WIDTH;
use crate::palette::{Palette, ThemePreset};
use crate::popup_menu::{popup_menu_rows_in, MenuRow, PopupEntry};
use crate::section_head::{section_head, SectionHead};
use crate::splitter::{splitter, width_label, SplitterMessages};
use crate::theme::git::SplitterLook;
use crate::theme::{font_size, spacing};

type Cell<'a> = Element<'a, ()>;

fn palette() -> Palette {
    ThemePreset::Dracula.palette()
}

fn pane<'a>(title: &str, width: f32, p: &Palette) -> Cell<'a> {
    container(
        column![
            text(title.to_owned()).size(font_size::ROW),
            kit::dim("feat/git-tab".to_owned(), p),
            kit::dim("release/1.7.1".to_owned(), p),
        ]
        .spacing(spacing::XS),
    )
    .padding(spacing::M)
    .width(width)
    .height(84.0)
    .into()
}

fn splitter_state<'a>(label: &str, look: SplitterLook, has_focus: bool, p: &Palette) -> Cell<'a> {
    let messages = SplitterMessages {
        press: (),
        reset: (),
        hover: (),
        leave: (),
    };
    let bar = splitter(look, has_focus, messages, Language::English, p);
    let shown: Cell<'a> = if look == SplitterLook::Dragged {
        stack![
            row![pane("Commits", 130.0, p), bar, pane("Detail", 130.0, p)],
            container(width_label(420.0, Language::English, p))
                .padding([spacing::M, 0.0])
                .width(Length::Fill)
                .align_x(alignment::Horizontal::Center)
                .align_y(alignment::Vertical::Bottom)
                .height(84.0),
        ]
        .into()
    } else {
        row![pane("Commits", 130.0, p), bar, pane("Detail", 130.0, p)].into()
    };
    column![kit::dim(label.to_owned(), p), shown]
        .spacing(spacing::XS)
        .into()
}

#[test]
fn the_splitter_looks_the_same() {
    let p = palette();
    let content = column![
        row![
            splitter_state("ruhig", SplitterLook::Idle, false, &p),
            splitter_state("Maus darüber", SplitterLook::Hovered, false, &p),
            splitter_state("ziehen · 420 px", SplitterLook::Dragged, false, &p),
            splitter_state("Fokus", SplitterLook::Idle, true, &p),
        ]
        .spacing(spacing::XXL),
        Space::new().height(Length::Fill),
    ]
    .padding(spacing::XXL);
    matches_picture("splitter", Size::new(1180.0, 180.0), content.into());
}

fn head<'a>(title: &'a str, count: usize, open: bool, has_focus: bool, p: &Palette) -> Cell<'a> {
    section_head(
        SectionHead {
            title,
            count,
            open,
            has_focus,
            on_toggle: (),
        },
        p,
    )
}

fn branch_row<'a>(label: &str, p: &Palette) -> Cell<'a> {
    container(text(label.to_owned()).size(font_size::ROW))
        .padding([2.0, spacing::XXL])
        .width(Length::Fill)
        .style(crate::theme::git::marked_row(p, false, false))
        .into()
}

#[test]
fn the_section_head_looks_the_same() {
    let p = palette();
    let content = column![
        head("Lokal", 3, true, false, &p),
        branch_row("main", &p),
        branch_row("feat/git-tab", &p),
        branch_row("release/1.7.1", &p),
        head("Remote", 12, false, false, &p),
        head("Tags", 0, false, false, &p),
        head("Worktrees", 2, true, true, &p),
        branch_row("../ncrs-1-8-0  feat/1-8-0", &p),
        branch_row("../ncrs-hotfix  hotfix/store", &p),
        head("Stashes", 4, false, true, &p),
    ];
    matches_picture("section-head", Size::new(420.0, 330.0), content.into());
}

fn bar<'a>(sandbox: bool, behind: bool, p: &Palette) -> Cell<'a> {
    let on = |enabled: bool| (!sandbox && enabled).then_some(());
    let pull_tone = if behind {
        Tone::Primary
    } else {
        Tone::Secondary
    };
    let pull_label = if behind { "Pull ↓2" } else { "Pull" };
    row![
        kit::toolbar_button("Holen", Some("⌘⇧F"), "Fetch", on(true), Tone::Secondary, p),
        kit::toolbar_button("Pull ↓2", Some("⌘⇧L"), "Pull", on(true), Tone::Secondary, p),
        kit::toolbar_button(pull_label, Some("⌘⇧L"), "Pull", on(true), pull_tone, p),
        kit::toolbar_button("Push", Some("⌘⇧K"), "Push", on(false), Tone::Secondary, p),
        kit::toolbar_button("Branch", None, "New branch", on(true), Tone::Secondary, p),
        kit::toolbar_button("Stash", Some("⌘⇧H"), "Stash", on(true), Tone::Secondary, p),
        kit::toolbar_button(
            "Rückgängig",
            Some("⌘Z"),
            "Undo",
            on(true),
            Tone::Secondary,
            p
        ),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center)
    .into()
}

#[test]
fn the_toolbar_looks_the_same() {
    let p = palette();
    let content = column![
        kit::dim("hinter origin: Pull ist primär".to_owned(), &p),
        bar(false, true, &p),
        kit::dim(
            "Git-Aktionen gibt es in der Homebrew-Version (gesperrt)".to_owned(),
            &p
        ),
        bar(true, false, &p),
    ]
    .spacing(spacing::M)
    .padding(spacing::XXL);
    matches_picture("toolbar", Size::new(900.0, 170.0), content.into());
}

fn entry(label: &str, key: Option<&str>, selected: bool) -> PopupEntry<()> {
    PopupEntry {
        label: label.to_owned(),
        key: key.map(str::to_owned),
        is_selected: selected,
        on_press: Some(()),
        on_hover: None,
    }
}

fn branch_menu(delete_open: bool) -> Vec<MenuRow<()>> {
    vec![
        MenuRow::Entry(entry("Auschecken", Some("⇧⌘O"), !delete_open)),
        MenuRow::Entry(entry("Neuer Branch von hier…", Some("⇧⌘T"), false)),
        MenuRow::Entry(entry("Umbenennen…", Some("⇧F6"), false)),
        MenuRow::Separator,
        MenuRow::Submenu(
            entry("Löschen…", None, delete_open),
            vec![
                MenuRow::Entry(entry("Nur lokal", None, true)),
                MenuRow::Entry(entry("Lokal und origin", None, false)),
                MenuRow::Separator,
                MenuRow::Danger(entry("Hart (Änderungen verwerfen…)", None, false)),
            ],
        ),
        MenuRow::Danger(entry("Alle Änderungen verwerfen…", None, false)),
    ]
}

#[test]
fn the_menu_with_a_submenu_looks_the_same() {
    let p = palette();
    let window = Size::new(1700.0, 220.0);
    let at = |x: f32, open: bool| {
        popup_menu_rows_in(
            branch_menu(open),
            MENU_WIDTH,
            Point::new(x, 16.0),
            window,
            &p,
        )
    };
    // The last menu is at the right edge: its submenu folds to the left.
    let content: Cell<'_> = stack![at(16.0, false), at(340.0, true), at(1380.0, true)].into();
    matches_picture("submenu", window, content);
}
