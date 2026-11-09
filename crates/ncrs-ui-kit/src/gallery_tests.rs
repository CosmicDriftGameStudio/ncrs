//! One picture per theme preset with every kit widget in its states: the
//! style guide made visible. A widget that changes its look changes the
//! picture, and the diff is the review.

// reason: a failing assertion is the signal in a test
#![allow(clippy::unwrap_used)]

use iced::widget::{button, column, container, text, Row, Space};
use iced::{alignment, Element, Length, Size};
use iced_test::Simulator;

use crate::hermes_kit::{self, ChipKind, MenuItem};
use crate::kit::{self, Check, Tone};
use crate::meter::{self, Meter, MeterLevel};
use crate::nav_list::{self, NavGroup, NavItem};
use crate::palette::{Palette, ThemePreset};
use crate::setting_rows::{self, Group, RowFocus, RowKey, SettingRow};
use crate::theme::{self, font_size, size, spacing};

type Cell<'a> = Element<'a, ()>;

const GALLERY: Size = Size::new(1360.0, 1000.0);
const LABEL_WIDTH: f32 = 140.0;
const COLUMN_WIDTH: f32 = 280.0;
const FOCUS_FIELD: &str = "gallery-focus-field";

/// What one row shows under rest, on or selected, disabled and focus.
struct Slots<'a> {
    rest: Option<Cell<'a>>,
    on: Option<Cell<'a>>,
    disabled: Option<Cell<'a>>,
    focus: Option<Cell<'a>>,
}

fn slots<'a>(
    rest: Option<Cell<'a>>,
    on: Option<Cell<'a>>,
    disabled: Option<Cell<'a>>,
    focus: Option<Cell<'a>>,
) -> Slots<'a> {
    Slots {
        rest,
        on,
        disabled,
        focus,
    }
}

fn grid_row<'a>(label: &str, cells: Slots<'a>, palette: &Palette) -> Cell<'a> {
    let mut line = Row::new()
        .push(
            container(kit::dim(label.to_owned(), palette))
                .width(LABEL_WIDTH)
                .align_y(alignment::Vertical::Center),
        )
        .spacing(spacing::M)
        .align_y(alignment::Vertical::Center);
    for cell in [cells.rest, cells.on, cells.disabled, cells.focus] {
        line =
            line.push(container(cell.unwrap_or_else(|| Space::new().into())).width(COLUMN_WIDTH));
    }
    line.into()
}

fn header<'a>(palette: &Palette) -> Cell<'a> {
    let title = |title: &str| Some(kit::dim(title.to_owned(), palette));
    grid_row(
        "",
        slots(
            title("rest"),
            title("on / selected"),
            title("disabled"),
            title("focus"),
        ),
        palette,
    )
}

fn tab<'a>(title: &str, active: bool, palette: &Palette) -> Cell<'a> {
    container(column![
        theme::tab_top_line(palette, active),
        button(text(title.to_owned()).size(font_size::FILE))
            .padding([0.0, spacing::L])
            .height(size::TITLE_BAR)
            .style(theme::title_tab(palette, active)),
    ])
    .width(150)
    .into()
}

fn plain_row<'a>(
    label: &str,
    tagged: bool,
    cursor: bool,
    active: bool,
    palette: &Palette,
) -> Cell<'a> {
    button(text(label.to_owned()).size(font_size::ROW))
        .width(Length::Fill)
        .height(size::ROW)
        .padding([0.0, spacing::CELL_PADDING_X])
        .style(theme::row(palette, tagged, cursor, active))
        .into()
}

fn menu<'a>(palette: &Palette) -> Cell<'a> {
    let entry = |label: &str, selected: bool| -> Cell<'a> {
        button(text(label.to_owned()).size(font_size::ROW))
            .width(Length::Fill)
            .height(size::ROW)
            .padding([0.0, spacing::CELL_PADDING_X])
            .on_press(())
            .style(theme::menu_item(palette, selected))
            .into()
    };
    container(column![entry("Open", true), entry("Rename", false)])
        .padding(spacing::XS)
        .width(150)
        .style(theme::menu(palette))
        .into()
}

fn setting_row<'a>(focused: bool, palette: &Palette) -> Cell<'a> {
    let mut setting = SettingRow::text(
        "Language".to_owned(),
        Some("The language of the app.".to_owned()),
        kit::button("Change", Some(()), Tone::Secondary, palette),
    );
    setting.key = Some(RowKey::Press(()));
    setting.control_width = Some(90.0);
    let group = Group {
        intro: None,
        title: String::new(),
        rows: vec![setting],
    };
    let focus = focused.then_some(RowFocus {
        row: 0,
        open: None,
        button: 0,
    });
    let rows =
        setting_rows::sections_with_focus(vec![group], |_| true, None, focus, |_, _| None, palette);
    column(rows).width(Length::Fill).into()
}

fn gauge<'a>(level: MeterLevel, fraction: f32, palette: &Palette) -> Cell<'a> {
    meter::meter(
        Meter {
            title: "Disk",
            value: "42 %",
            fraction,
            level,
            detail: "120 GB of 500 GB",
        },
        palette,
    )
}

/// One page of the settings sidebar; `dimmed` is the page having the keys.
fn sidebar_item<'a>(chosen: bool, dimmed: bool, palette: &Palette) -> Cell<'a> {
    nav_list::nav_list(
        vec![NavGroup {
            head: "Pages".to_owned(),
            mark: None,
            items: vec![NavItem {
                label: "General".to_owned(),
                suffix: None,
                chosen,
                dimmed,
                on_press: (),
            }],
        }],
        palette,
    )
}

fn focus_ring<'a>(content: Cell<'a>, palette: &Palette) -> Cell<'a> {
    let ring = theme::focus_border(palette);
    container(content)
        .style(move |_: &iced::Theme| container::Style {
            border: ring,
            ..container::Style::default()
        })
        .into()
}

fn gallery<'a>(palette: &Palette) -> Cell<'a> {
    let p = palette;
    let button_of =
        |tone: Tone, label: &str, on: bool| kit::button(label, on.then_some(()), tone, p);
    let options = [("One", ()), ("Two", ())];
    let items = [MenuItem::new("Alpha", ()), MenuItem::new("Beta", ())];
    let field = |value: &str, id: &'static str, typing: bool| {
        if typing {
            kit::text_field("Name", value, id, Some(|_: String| ()), (), p)
        } else {
            kit::text_field("Name", value, id, None::<fn(String) -> ()>, (), p)
        }
    };
    let list_row = |label: &str, selected: bool, focused: bool| {
        kit::list_row(
            text(label.to_owned()).size(font_size::ROW).into(),
            (),
            selected,
            focused,
            p,
        )
    };
    let rows: Vec<Cell<'a>> = vec![
        header(p),
        grid_row(
            "button",
            slots(
                Some(button_of(Tone::Secondary, "Cancel", true)),
                None,
                Some(button_of(Tone::Secondary, "Cancel", false)),
                None,
            ),
            p,
        ),
        grid_row(
            "primary",
            slots(
                Some(button_of(Tone::Primary, "Save", true)),
                Some(kit::enter_button("Save", Some(()), Tone::Primary, p)),
                Some(button_of(Tone::Primary, "Save", false)),
                None,
            ),
            p,
        ),
        grid_row(
            "danger",
            slots(
                Some(button_of(Tone::Danger, "Delete", true)),
                None,
                Some(button_of(Tone::Danger, "Delete", false)),
                None,
            ),
            p,
        ),
        grid_row(
            "text field",
            slots(
                Some(field("", "gallery-empty", true)),
                Some(field("Invoices", "gallery-filled", true)),
                Some(field("Invoices", "gallery-off", false)),
                Some(field("Invoices", FOCUS_FIELD, true)),
            ),
            p,
        ),
        grid_row(
            "dropdown",
            slots(
                Some(hermes_kit::dropdown(150.0, &items, 0, p)),
                Some(hermes_kit::dropdown(150.0, &items, 1, p)),
                None,
                Some(focus_ring(hermes_kit::dropdown(150.0, &items, 0, p), p)),
            ),
            p,
        ),
        grid_row(
            "checkbox",
            slots(
                Some(kit::checkbox(Check::Off, Some(()), p)),
                Some(kit::checkbox(Check::On, Some(()), p)),
                Some(kit::checkbox(Check::Off, None, p)),
                None,
            ),
            p,
        ),
        grid_row(
            "segment",
            slots(
                Some(kit::segmented(&options, 0, true, p)),
                Some(kit::segmented(&options, 1, true, p)),
                Some(kit::segmented(&options, 0, false, p)),
                None,
            ),
            p,
        ),
        grid_row(
            "chip",
            slots(
                Some(hermes_kit::chip(
                    "invoice",
                    ChipKind::Automatic,
                    None::<()>,
                    p,
                )),
                None,
                None,
                None,
            ),
            p,
        ),
        grid_row(
            "own chip",
            slots(
                Some(hermes_kit::chip("invoice", ChipKind::Own, Some(()), p)),
                Some(kit::removable_chip("invoice", (), p)),
                None,
                None,
            ),
            p,
        ),
        grid_row(
            "key cap",
            slots(
                Some(kit::key_cap("F5", p)),
                Some(kit::key_cap_focused("F5", p)),
                None,
                None,
            ),
            p,
        ),
        grid_row(
            "tab",
            slots(
                Some(tab("Inactive", false, p)),
                Some(tab("Active", true, p)),
                None,
                None,
            ),
            p,
        ),
        grid_row(
            "list row",
            slots(
                Some(list_row("invoice.pdf", false, true)),
                Some(list_row("invoice.pdf", true, true)),
                None,
                None,
            ),
            p,
        ),
        grid_row(
            "list, no keys",
            slots(
                Some(list_row("invoice.pdf", true, false)),
                Some(plain_row("invoice.pdf", true, false, true, p)),
                None,
                None,
            ),
            p,
        ),
        grid_row(
            "sidebar item",
            slots(
                Some(sidebar_item(false, false, p)),
                Some(sidebar_item(true, false, p)),
                Some(sidebar_item(true, true, p)),
                None,
            ),
            p,
        ),
        grid_row("menu", slots(None, Some(menu(p)), None, None), p),
        grid_row(
            "setting row",
            slots(
                Some(setting_row(false, p)),
                None,
                None,
                Some(setting_row(true, p)),
            ),
            p,
        ),
        grid_row(
            "meter",
            slots(
                Some(gauge(MeterLevel::Normal, 0.3, p)),
                Some(gauge(MeterLevel::Warning, 0.6, p)),
                Some(gauge(MeterLevel::Critical, 0.95, p)),
                None,
            ),
            p,
        ),
        grid_row(
            "mode field",
            slots(
                Some(
                    container(text("LEFT").size(font_size::STATUS))
                        .padding([0.0, spacing::L])
                        .height(size::STATUS_BAR)
                        .style(theme::mode_field(p))
                        .into(),
                ),
                None,
                None,
                None,
            ),
            p,
        ),
    ];
    container(column(rows).spacing(spacing::L))
        .padding(spacing::XXL)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::panel(p))
        .into()
}

fn matches_image(preset: ThemePreset) {
    let palette = preset.palette();
    let name = format!("gallery-{}", format!("{preset:?}").to_lowercase());
    let mut simulator = Simulator::with_size(
        iced::Settings {
            default_font: crate::APP_FONT,
            ..iced::Settings::default()
        },
        GALLERY,
        gallery(&palette),
    );
    simulator.click(iced::widget::Id::new(FOCUS_FIELD)).unwrap();
    let matches = simulator
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_image(format!("tests/snapshots/{name}.png"))
        .unwrap();
    assert!(matches, "{name} differs from its image");
}

/// Named for CI, which compares pixels only on macOS.
mod ui_tests {
    use super::*;

    #[test]
    fn the_gallery_looks_the_same_in_every_preset() {
        for preset in ThemePreset::ALL {
            matches_image(preset);
        }
    }
}

/// The favourites bar apart from the grid: so adding it left the grid alone.
mod favourites_bar_ui_tests {
    use super::*;
    use crate::favourites_bar::{self, Slot};

    fn slot(number: usize, label: &str, is_current: bool) -> Slot<()> {
        Slot {
            number,
            label: label.to_owned(),
            is_current,
            tooltip: String::new(),
            on_press: (),
            on_right_press: (),
        }
    }

    fn bar_picture(preset: ThemePreset) {
        let palette = preset.palette();
        let full = favourites_bar::view(
            vec![
                slot(1, "~", false),
                slot(2, "Documents", true),
                slot(3, "work/src", false),
                slot(4, "Downloads", false),
                slot(5, "projects/ncrs", false),
            ],
            Some("Cmd+1 … 5".to_owned()),
            &palette,
        );
        let few = favourites_bar::view(
            vec![slot(1, "~", true), slot(2, "Documents", false)],
            None,
            &palette,
        );
        let content: Element<'_, ()> = iced::widget::column![full, few].spacing(spacing::L).into();
        let name = format!("favourites-bar-{}", format!("{preset:?}").to_lowercase());
        let matches = Simulator::with_size(
            iced::Settings {
                default_font: crate::APP_FONT,
                ..iced::Settings::default()
            },
            Size::new(700.0, 80.0),
            container(content).style(theme::panel(&palette)),
        )
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_image(format!("tests/snapshots/{name}.png"))
        .unwrap();
        assert!(matches, "{name} differs from its image");
    }

    #[test]
    fn the_favourites_bar_looks_the_same_in_every_preset() {
        for preset in ThemePreset::ALL {
            bar_picture(preset);
        }
    }
}
