//! One picture per theme preset with every kit widget in its states: the
//! style guide made visible. A widget that changes its look changes the
//! picture, and the diff is the review.

use crate::snapshot_support::ReferenceSnapshot as _;
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
    // The list scrolls and fills its sidebar; here it gets the room of one group.
    const ONE_GROUP_HEIGHT: f32 = 64.0;
    let list = nav_list::nav_list(
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
    );
    container(list).height(ONE_GROUP_HEIGHT).into()
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
                Some(kit::focused(
                    hermes_kit::dropdown(150.0, &items, 0, p),
                    true,
                    p,
                )),
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
                Some(kit::focusable_segmented(&options, 0, true, true, p)),
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
        .matches_reference_image(format!("tests/snapshots/{name}.png"))
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
            number: Some(number),
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
            None,
            Some("Cmd+1 … 5".to_owned()),
            &palette,
        );
        let few = favourites_bar::view(
            vec![slot(1, "~", true), slot(2, "Documents", false)],
            None,
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
        .matches_reference_image(format!("tests/snapshots/{name}.png"))
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

/// The ticked rows apart from the grid, for the same reason.
mod check_rows_ui_tests {
    use super::*;
    use crate::check_rows::{check_group_head, check_row, CheckGroupHead, CheckRow};

    fn picture(preset: ThemePreset) {
        let palette = preset.palette();
        let plain = |name: &str, check: Check, cursor: bool| {
            check_row(
                CheckRow {
                    check,
                    name: name.to_owned(),
                    detail: None,
                    reason: "in main".to_owned(),
                    worktree: false,
                    kept: false,
                    cursor,
                    on_toggle: Some(()),
                    outcome: None,
                },
                &palette,
            )
        };
        let content: Element<'_, ()> = iced::widget::column![
            check_group_head(
                CheckGroupHead {
                    title: "infra".to_owned(),
                    subtitle: "~/code/infra".to_owned(),
                    count: 5,
                    open: true,
                    check: Check::Mixed,
                    cursor: false,
                    on_toggle_open: (),
                    on_toggle_check: (),
                },
                &palette,
            ),
            plain("chore/renovate-oct", Check::On, false),
            plain("fix/etcd-snapshot-s3", Check::Off, true),
            check_row(
                CheckRow {
                    check: Check::On,
                    name: "fix/redis-probe-timeout".to_owned(),
                    detail: Some("~/code/infra-wt-redis".to_owned()),
                    reason: "worktree only, branch merged".to_owned(),
                    worktree: true,
                    kept: false,
                    cursor: false,
                    on_toggle: Some(()),
                    outcome: None,
                },
                &palette,
            ),
            check_row(
                CheckRow {
                    check: Check::Off,
                    name: "feat/connector-oauth".to_owned(),
                    detail: None,
                    reason: "3 local changes".to_owned(),
                    worktree: false,
                    kept: true,
                    cursor: false,
                    on_toggle: None,
                    outcome: None,
                },
                &palette,
            ),
        ]
        .into();
        let name = format!("check-rows-{}", format!("{preset:?}").to_lowercase());
        let matches = Simulator::with_size(
            iced::Settings {
                default_font: crate::APP_FONT,
                ..iced::Settings::default()
            },
            Size::new(560.0, 180.0),
            container(content).style(theme::panel(&palette)),
        )
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image(format!("tests/snapshots/{name}.png"))
        .unwrap();
        assert!(matches, "{name} differs from its image");
    }

    #[test]
    fn the_ticked_rows_look_the_same_in_every_preset() {
        for preset in ThemePreset::ALL {
            picture(preset);
        }
    }
}

/// The blocks of the filing pages apart from the grid, so the grid stays as it is.
mod filing_blocks_ui_tests {
    use super::*;
    use crate::hermes_kit::TargetCheck;
    use crate::kit::PatternField;
    use crate::plan_rows::{plan_action_row, PlanActionRow, PlanFileRow, PlanLink};
    use crate::setting_rows::RowButton;
    use iced::widget::column;

    fn chips<'a>(p: &Palette) -> Cell<'a> {
        let chips =
            (0..7).map(|index| hermes_kit::target_chip(index, &format!("T{}", index + 1), p));
        Row::with_children(chips).spacing(spacing::M).into()
    }

    fn target_rows<'a>(p: &Palette) -> Cell<'a> {
        let row = |state: Check, index: usize, number: &'a str, name: &'a str, marked: bool| {
            hermes_kit::target_check_row(
                TargetCheck {
                    state,
                    on_toggle: Some(()),
                    index,
                    number,
                    name,
                    path: "/Volumes/Vault/Documents",
                    marked,
                },
                p,
            )
        };
        column![
            row(Check::On, 0, "T1", "Documents", false),
            row(Check::On, 1, "T2", "Company", true),
            row(Check::Off, 2, "T3", "Family", false),
        ]
        .into()
    }

    fn pattern<'a>(p: &Palette) -> Cell<'a> {
        const TOKENS: [(&str, ()); 4] = [
            ("{sender}", ()),
            ("{kind}", ()),
            ("{year}", ()),
            ("{day}", ()),
        ];
        let field = |value: &'a str, preview: Option<&str>, problem: Option<&str>| {
            kit::pattern_field(
                PatternField {
                    value,
                    id: "gallery-pattern",
                    on_input: Box::new(|_| ()),
                    on_submit: (),
                    tokens: &TOKENS,
                    preview: preview.map(str::to_owned),
                    problem: problem.map(str::to_owned),
                },
                p,
            )
        };
        column![
            kit::token_row("Add", &TOKENS, p),
            field(
                "Bank/{sender}/{year}/{kind}.pdf",
                Some("Result: Bank/Example Bank/2026/Statement.pdf"),
                None,
            ),
            field(
                "Bank/{sender}/{unknown}",
                None,
                Some("{unknown} is not a block")
            ),
        ]
        .spacing(spacing::M)
        .into()
    }

    fn plan_rows<'a>(p: &Palette) -> Cell<'a> {
        let line_width = 850.0;
        let file = |chosen: bool| PlanFileRow {
            name: "scan-006.pdf".to_owned(),
            what: "no rule matches · stays where it is".to_owned(),
            what_color: p.dim_text,
            chip: None,
            chosen,
            on_press: (),
        };
        let links = || {
            vec![
                PlanLink {
                    label: "Make a rule…".to_owned(),
                    on_press: (),
                },
                PlanLink {
                    label: "Hermes: suggest a rule".to_owned(),
                    on_press: (),
                },
            ]
        };
        column![
            plan_action_row(
                PlanActionRow {
                    file: file(false),
                    links: Vec::new(),
                    marked_link: None,
                },
                line_width,
                p,
            ),
            plan_action_row(
                PlanActionRow {
                    file: file(false),
                    links: links(),
                    marked_link: None,
                },
                line_width,
                p,
            ),
            plan_action_row(
                PlanActionRow {
                    file: file(true),
                    links: links(),
                    marked_link: Some(1),
                },
                line_width,
                p,
            ),
        ]
        .into()
    }

    fn folding_heads<'a>(p: &Palette) -> Cell<'a> {
        let head = |open: bool, index: usize| {
            SettingRow::folding_head(
                open,
                hermes_kit::target_chip(index, "T1", p),
                "Documents".to_owned(),
                Some("/Volumes/Vault/Documents · from Downloads".to_owned()),
                (),
                vec![
                    RowButton::new("+ Rule…", Some(()), Tone::Secondary),
                    RowButton::new("Remove", Some(()), Tone::Secondary),
                ],
                p,
            )
        };
        let group = Group {
            intro: None,
            title: String::new(),
            rows: vec![head(true, 0), head(false, 1)],
        };
        let focus = RowFocus {
            row: 1,
            open: None,
            button: 0,
        };
        let rows = setting_rows::sections_with_focus(
            vec![group],
            |_| true,
            None,
            Some(focus),
            |_, _| None,
            p,
        );
        column(rows).width(Length::Fill).into()
    }

    fn picture(preset: ThemePreset) {
        let palette = preset.palette();
        let p = &palette;
        let content: Cell<'_> = column![
            chips(p),
            target_rows(p),
            pattern(p),
            plan_rows(p),
            folding_heads(p),
        ]
        .spacing(spacing::L)
        .into();
        let name = format!("filing-blocks-{}", format!("{preset:?}").to_lowercase());
        let matches = Simulator::with_size(
            iced::Settings {
                default_font: crate::APP_FONT,
                ..iced::Settings::default()
            },
            Size::new(900.0, 640.0),
            container(content)
                .padding(spacing::XXL)
                .style(theme::panel(p)),
        )
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image(format!("tests/snapshots/{name}.png"))
        .unwrap();
        assert!(matches, "{name} differs from its image");
    }

    #[test]
    fn the_filing_blocks_look_the_same_in_every_preset() {
        for preset in ThemePreset::ALL {
            picture(preset);
        }
    }
}
