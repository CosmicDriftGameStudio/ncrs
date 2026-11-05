//! The System page, and the menu and the dialog that stand over it.

use iced::widget::{column, container, mouse_area, row, Space, Stack};
use iced::{alignment, Element, Length};

use ncrs_config::system_monitor::{DriveColumn, NetworkUnit};
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_ui_kit::data_table::{data_table, SortMark, TableCell, TableColumn, TableRow};
use ncrs_ui_kit::frame::{DialogSize, Frame, KeyHints};
use ncrs_ui_kit::kit::{self, Tone};
use ncrs_ui_kit::meter::{level_text_color, meter, meter_level, Meter};
use ncrs_ui_kit::page_status_bar::{page_status_bar, PageStatusBar, PageStatusLine};
use ncrs_ui_kit::page_text::{page_band, page_heading, page_line};
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::popup_menu::{popup_menu, PopupEntry};
use ncrs_ui_kit::theme::{self, font_size, spacing};

use crate::ctx::ViewCtx;
use crate::kill::{KillFailure, KillRefusal};
use crate::layout::{
    visible_rows, PAGE_PADDING, TASKS_HEAD_GAP, TASKS_HEAD_HEIGHT, TOP_BAND_HEIGHT,
};
use crate::message::MonitorMessage;
use crate::snapshot::{HostInfo, Snapshot};
use crate::state::{Failure, MenuEntry, State};
use crate::tasks::TaskColumn;
use crate::units::{
    clock_time, one_decimal_or_dash, percent_or_dash, rate_or_dash, short_size, uptime_text, DASH,
};

pub const FILTER_ID: &str = "monitor-filter";

const SIDE_WIDTH: f32 = 420.0;
const NUMBER_WIDTH: f32 = 64.0;
const NAME_WIDTH: f32 = 100.0;

type Page<'a> = Element<'a, MonitorMessage>;

fn label<'a>(content: impl Into<String>, color: iced::Color) -> Page<'a> {
    page_line(content, font_size::BODY, color)
}

fn heading<'a>(content: &str, palette: &Palette) -> Page<'a> {
    page_heading(content, palette)
}

fn plain_column<M>(title: &str, width: Length, right: bool) -> TableColumn<M> {
    TableColumn {
        title: title.to_owned(),
        width,
        align_right: right,
        sort: SortMark::None,
        on_press: None,
    }
}

fn host_line<'a>(host: &HostInfo, lang: Language, palette: &Palette) -> Page<'a> {
    let uptime = host.uptime_seconds.map(|seconds| {
        fill_template(
            lang.text(Msg::MonitorUptime),
            &[("{time}", &uptime_text(seconds))],
        )
    });
    let mut line = row![
        page_line(host.name.clone(), font_size::TITLE, palette.text),
        label(
            format!("{} \u{b7} {} \u{b7} {}", host.os, host.kernel, host.arch),
            palette.dim_text
        ),
        Space::new().width(Length::Fill),
    ]
    .spacing(spacing::L)
    .align_y(alignment::Vertical::Center);
    if let Some(since_boot) = uptime {
        line = line.push(label(since_boot, palette.dim_text));
    }
    line.into()
}

fn meters<'a>(snapshot: &Snapshot, lang: Language, palette: &Palette) -> Page<'a> {
    let mut blocks: Vec<Page<'a>> = Vec::new();
    let mut add = |title: Msg, percent: Option<f32>, detail: String| {
        let level = meter_level(percent.unwrap_or(0.0));
        let value = percent_or_dash(percent);
        blocks.push(meter(
            Meter {
                title: lang.text(title),
                value: &value,
                fraction: percent.unwrap_or(0.0) / 100.0,
                level,
                detail: &detail,
            },
            palette,
        ));
    };
    if let Some(cpu) = &snapshot.cpu {
        add(
            Msg::MonitorMeterCpu,
            cpu.percent,
            fill_template(
                lang.text(Msg::MonitorDetailCpu),
                &[("{brand}", &cpu.brand), ("{cores}", &cpu.cores.to_string())],
            ),
        );
    }
    let usage_detail = |msg: Msg, usage: crate::snapshot::Usage| {
        fill_template(
            lang.text(msg),
            &[
                ("{used}", &short_size(usage.used)),
                ("{total}", &short_size(usage.total)),
                ("{free}", &short_size(usage.free)),
            ],
        )
    };
    if let Some(memory) = snapshot.memory {
        add(
            Msg::MonitorMeterMemory,
            Some(memory.percent()),
            usage_detail(Msg::MonitorDetailMemory, memory),
        );
    }
    if let Some(swap) = snapshot.swap {
        add(
            Msg::MonitorMeterSwap,
            Some(swap.percent()),
            usage_detail(Msg::MonitorDetailSwap, swap),
        );
    }
    if let (Some(load), Some(cpu)) = (snapshot.load, &snapshot.cpu) {
        let percent = (load.one * 100.0 / cpu.cores.max(1) as f64) as f32;
        let detail = fill_template(
            lang.text(Msg::MonitorDetailLoad),
            &[
                ("{one}", &format!("{:.2}", load.one)),
                ("{five}", &format!("{:.2}", load.five)),
                ("{fifteen}", &format!("{:.2}", load.fifteen)),
            ],
        );
        let level = meter_level(percent);
        let value = format!("{:.2}", load.one);
        blocks.push(meter(
            Meter {
                title: lang.text(Msg::MonitorMeterLoad),
                value: &value,
                fraction: percent / 100.0,
                level,
                detail: &detail,
            },
            palette,
        ));
    }
    row(blocks).spacing(spacing::XL).into()
}

fn network_block<'a>(state: &State, lang: Language, palette: &Palette) -> Option<Page<'a>> {
    let networks = state.networks_to_show();
    if networks.is_empty() {
        return None;
    }
    let bits = state.settings.network_unit == NetworkUnit::Bits;
    let columns = [
        plain_column(lang.text(Msg::MonitorNetworkInterface), Length::Fill, false),
        plain_column(
            lang.text(Msg::MonitorNetworkReceived),
            Length::Fixed(90.0),
            true,
        ),
        plain_column(
            lang.text(Msg::MonitorNetworkSent),
            Length::Fixed(90.0),
            true,
        ),
    ];
    let rows = networks
        .into_iter()
        .map(|network| TableRow {
            cells: vec![
                TableCell::plain(network.name.clone()),
                TableCell::plain(rate_or_dash(network.received_per_second, bits)),
                TableCell::plain(rate_or_dash(network.transmitted_per_second, bits)),
            ],
            is_cursor: false,
            on_press: None,
            on_right_press: None,
        })
        .collect();
    Some(
        column![
            heading(lang.text(Msg::MonitorNetwork), palette),
            data_table(&columns, rows, palette)
        ]
        .into(),
    )
}

fn drives_block<'a>(state: &State, lang: Language, palette: &Palette) -> Option<Page<'a>> {
    let drives = state.drives_to_show();
    if drives.is_empty() {
        return None;
    }
    let bits = state.settings.network_unit == NetworkUnit::Bits;
    let free = state.settings.drive_column == DriveColumn::Free;
    let right = |title: Msg| plain_column(lang.text(title), Length::Fixed(66.0), true);
    let columns = [
        plain_column("", Length::Fill, false),
        right(if free {
            Msg::MonitorDriveFree
        } else {
            Msg::MonitorDriveUsed
        }),
        right(Msg::MonitorDriveTotal),
        right(Msg::MonitorDriveRead),
        right(Msg::MonitorDriveWritten),
    ];
    let rows = drives
        .into_iter()
        .map(|drive| {
            let shown = if free { drive.available } else { drive.used() };
            let color = level_text_color(meter_level(drive.used_percent()), palette);
            TableRow {
                cells: vec![
                    TableCell::plain(if drive.name.is_empty() {
                        drive.mount.display().to_string()
                    } else {
                        drive.name.clone()
                    }),
                    TableCell {
                        text: short_size(shown),
                        color: Some(color),
                    },
                    TableCell::plain(short_size(drive.total)),
                    TableCell::plain(rate_or_dash(drive.read_per_second, bits)),
                    TableCell::plain(rate_or_dash(drive.written_per_second, bits)),
                ],
                is_cursor: false,
                on_press: Some(MonitorMessage::OpenDrive(drive.mount.clone())),
                on_right_press: None,
            }
        })
        .collect();
    Some(
        column![
            heading(lang.text(Msg::MonitorDrives), palette),
            data_table(&columns, rows, palette)
        ]
        .into(),
    )
}

fn sensors_block<'a>(snapshot: &Snapshot, lang: Language, palette: &Palette) -> Option<Page<'a>> {
    if snapshot.sensors.is_empty() {
        return None;
    }
    let columns = [
        plain_column("", Length::Fill, false),
        plain_column("\u{b0}C", Length::Fixed(66.0), true),
    ];
    let rows = snapshot
        .sensors
        .iter()
        .map(|sensor| TableRow {
            cells: vec![
                TableCell::plain(sensor.label.clone()),
                TableCell::plain(format!("{:.0}", sensor.celsius)),
            ],
            is_cursor: false,
            on_press: None,
            on_right_press: None,
        })
        .collect();
    Some(
        column![
            heading(lang.text(Msg::MonitorSensors), palette),
            data_table(&columns, rows, palette)
        ]
        .into(),
    )
}

/// `count` is the number of tasks in a grouped row, shown in the pid column.
fn task_cells(
    state: &State,
    task: &crate::snapshot::TaskRow,
    count: Option<usize>,
) -> Vec<TableCell> {
    let memory_total = state
        .snapshot()
        .and_then(|s| s.memory)
        .map_or(0, |m| m.total);
    let memory_percent =
        (memory_total > 0).then(|| (task.resident as f64 * 100.0 / memory_total as f64) as f32);
    let command: String = task.command.chars().skip(state.command_offset).collect();
    vec![
        TableCell::plain(one_decimal_or_dash(task.cpu)),
        TableCell::plain(one_decimal_or_dash(memory_percent)),
        TableCell::plain(short_size(task.resident)),
        TableCell::plain(count.map_or_else(
            || task.pid.to_string(),
            |members| format!("\u{d7}{members}"),
        )),
        TableCell::plain(task.user.clone()),
        TableCell::plain(clock_time(task.cpu_seconds)),
        TableCell::plain(
            task.status
                .map_or_else(|| DASH.to_owned(), |status| status.letter().to_string()),
        ),
        TableCell::plain(command),
    ]
}

fn task_column_width(column: TaskColumn) -> Length {
    match column {
        TaskColumn::Cpu
        | TaskColumn::Memory
        | TaskColumn::Resident
        | TaskColumn::Pid
        | TaskColumn::Time => Length::Fixed(NUMBER_WIDTH),
        TaskColumn::User => Length::Fixed(NAME_WIDTH),
        TaskColumn::Status => Length::Fixed(28.0),
        TaskColumn::Command => Length::Fill,
    }
}

fn tasks_block<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Page<'a> {
    let ViewCtx {
        lang,
        palette,
        window_size,
    } = ctx;
    let visible = visible_rows(window_size.height);
    let first = state.window.offset(state.order.len(), visible);
    let columns: Vec<TableColumn<MonitorMessage>> = TaskColumn::ALL
        .iter()
        .map(|&column_kind| TableColumn {
            title: lang.text(column_kind.title()).to_owned(),
            width: task_column_width(column_kind),
            align_right: column_kind.is_right_aligned(),
            sort: if column_kind == state.column {
                if column_kind.sorts_descending() {
                    SortMark::Descending
                } else {
                    SortMark::Ascending
                }
            } else {
                SortMark::None
            },
            on_press: Some(MonitorMessage::SortBy(column_kind)),
        })
        .collect();
    let rows = (first..(first + visible).min(state.order.len()))
        .filter_map(|row_index| {
            let task = state.row_at(row_index)?;
            let count = state.group_at(row_index).map(crate::tasks::Group::count);
            Some(TableRow {
                cells: task_cells(state, task, count),
                is_cursor: state.cursor_index == row_index,
                on_press: Some(MonitorMessage::SelectRow(task.pid)),
                on_right_press: Some(MonitorMessage::OpenMenu(task.pid)),
            })
        })
        .collect();

    let summary = state.snapshot().and_then(|s| s.counts).map(|counts| {
        fill_template(
            lang.text(Msg::MonitorTasksSummary),
            &[
                ("{total}", &counts.total.to_string()),
                ("{running}", &counts.running.to_string()),
                ("{sleeping}", &counts.sleeping.to_string()),
                ("{other}", &counts.other.to_string()),
                ("{sort}", lang.text(state.column.title())),
            ],
        )
    });
    let find_key = if cfg!(target_os = "macos") {
        "Cmd+F"
    } else {
        "Ctrl+F"
    };
    let placeholder = fill_template(
        lang.text(Msg::MonitorFilterPlaceholder),
        &[("{key}", find_key)],
    );
    let filter = container(kit::text_field(
        &placeholder,
        &state.filter,
        FILTER_ID,
        Some(MonitorMessage::FilterChanged),
        MonitorMessage::FilterDone,
        palette,
    ))
    .width(240.0);
    let mut head = row![heading(lang.text(Msg::MonitorTasks), palette)]
        .spacing(spacing::L)
        .align_y(alignment::Vertical::Center);
    if let Some(counts) = summary {
        head = head.push(label(counts, palette.dim_text));
    }
    head = head.push(Space::new().width(Length::Fill)).push(filter);
    column![
        container(head)
            .height(TASKS_HEAD_HEIGHT)
            .align_y(alignment::Vertical::Center),
        mouse_area(data_table(&columns, rows, palette)).on_scroll(MonitorMessage::Scroll),
    ]
    .spacing(TASKS_HEAD_GAP)
    .width(Length::Fill)
    .into()
}

fn status_bar<'a>(state: &'a State, lang: Language, palette: &Palette) -> Page<'a> {
    let failure = state
        .failure
        .as_ref()
        .map(|failure| failure_text(failure, lang));
    let notice = state
        .notice
        .as_ref()
        .map(|name| fill_template(lang.text(Msg::MonitorNoticeKilled), &[("{name}", name)]));
    let every = fill_template(
        lang.text(Msg::MonitorHintRefresh),
        &[("{seconds}", &state.refresh_every().as_secs().to_string())],
    );
    let line = match (&failure, &notice) {
        (Some(text), _) => PageStatusLine::Failure(text),
        (None, Some(text)) => PageStatusLine::Notice(text),
        (None, None) => PageStatusLine::Hint(lang.text(if state.is_grouped() {
            Msg::MonitorHintUngroup
        } else {
            Msg::MonitorHintGroup
        })),
    };
    page_status_bar(
        PageStatusBar {
            mode: lang.text(Msg::MonitorMode),
            line,
            right: Some(&every),
        },
        palette,
    )
}

fn failure_text(failure: &Failure, lang: Language) -> String {
    match failure {
        Failure::Measure(reason) => fill_template(
            lang.text(Msg::MonitorFailureMeasure),
            &[("{reason}", reason)],
        ),
        Failure::Kill {
            name,
            failure: refusal,
        } => {
            let msg = match refusal {
                KillFailure::Refused(KillRefusal::OwnProcess) => Msg::MonitorFailureKillOwn,
                KillFailure::Refused(KillRefusal::Changed) => Msg::MonitorFailureKillChanged,
                KillFailure::NotKilled => Msg::MonitorFailureKillNotKilled,
            };
            fill_template(lang.text(msg), &[("{name}", name)])
        }
    }
}

pub fn view<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, MonitorMessage> {
    let (lang, palette) = (ctx.lang, ctx.palette);
    let body: Page<'a> = match state.snapshot() {
        None => container(label(lang.text(Msg::MonitorMeasuring), palette.dim_text))
            .padding(PAGE_PADDING)
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        Some(snapshot) => {
            let rule = || theme::horizontal_rule(palette.border_inactive);
            let mut side = column![];
            for (index, block) in [
                network_block(state, lang, palette),
                drives_block(state, lang, palette),
                sensors_block(snapshot, lang, palette),
            ]
            .into_iter()
            .flatten()
            .enumerate()
            {
                if index > 0 {
                    side = side.push(rule());
                }
                side = side.push(container(block).padding(PAGE_PADDING).width(Length::Fill));
            }
            let mut top = column![].spacing(spacing::XL);
            if let Some(host) = &snapshot.host {
                top = top.push(host_line(host, lang, palette));
            }
            top = top.push(meters(snapshot, lang, palette));
            let top_band = page_band(top, PAGE_PADDING, palette)
                .width(Length::Fill)
                .height(TOP_BAND_HEIGHT);
            let side_band = page_band(side, 0.0, palette)
                .width(SIDE_WIDTH + 2.0 * PAGE_PADDING)
                .height(Length::Fill);
            let tasks = container(tasks_block(state, ctx))
                .padding(PAGE_PADDING)
                .width(Length::Fill)
                .height(Length::Fill)
                .clip(true);
            column![
                top_band,
                rule(),
                row![
                    side_band,
                    theme::vertical_rule(palette.border_inactive),
                    tasks
                ]
                .height(Length::Fill),
            ]
            .into()
        }
    };
    column![
        container(body).width(Length::Fill).height(Length::Fill),
        status_bar(state, lang, palette),
    ]
    .into()
}

fn entry_label(entry: MenuEntry, lang: Language) -> (&'static str, Option<String>) {
    match entry {
        MenuEntry::Kill => (lang.text(Msg::MonitorMenuKill), Some("K".to_owned())),
        MenuEntry::CopyCommandLine => (lang.text(Msg::MonitorMenuCopyCommand), None),
        MenuEntry::GoToFolder => (lang.text(Msg::MonitorMenuGoToFolder), None),
    }
}

/// The context menu with the click catcher under it, over the whole window.
pub fn menu_view<'a>(state: &State, ctx: ViewCtx<'a>) -> Option<Element<'a, MonitorMessage>> {
    let menu = state.menu.as_ref()?;
    let entries = state
        .menu_entries()
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let (label, key) = entry_label(entry, ctx.lang);
            let enabled = state.entry_is_enabled(entry, &menu.task);
            PopupEntry {
                label: label.to_owned(),
                key,
                is_selected: menu.selected == index,
                on_press: enabled.then_some(MonitorMessage::MenuChoose(index)),
                on_hover: Some(MonitorMessage::MenuHover(index)),
            }
        })
        .collect();
    let catcher = mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
        .on_press(MonitorMessage::MenuClose)
        .on_right_press(MonitorMessage::MenuClose);
    Some(
        Stack::new()
            .push(catcher)
            .push(popup_menu(entries, menu.origin, ctx.palette))
            .into(),
    )
}

/// The kill confirmation; the app puts it over a scrim.
pub fn dialog_view<'a>(state: &State, ctx: ViewCtx<'a>) -> Option<Element<'a, MonitorMessage>> {
    let dialog = state.kill_dialog.as_ref()?;
    let ViewCtx {
        lang,
        palette,
        window_size,
    } = ctx;
    let subject = fill_template(
        lang.text(Msg::MonitorKillSubject),
        &[
            ("{name}", &dialog.target.name),
            ("{pid}", &dialog.target.pid.to_string()),
        ],
    );
    let content = column![
        label(subject, palette.text),
        kit::error_line(lang.text(Msg::MonitorKillWarning), palette),
    ]
    .spacing(spacing::LINE_GAP);
    let cancel_focused = dialog.focus == 0;
    let cancel = if cancel_focused {
        kit::enter_button(
            lang.text(Msg::DialogCancel),
            Some(MonitorMessage::CancelKill),
            Tone::Primary,
            palette,
        )
    } else {
        kit::button(
            lang.text(Msg::DialogCancel),
            Some(MonitorMessage::CancelKill),
            Tone::filled_if(false),
            palette,
        )
    };
    let confirm = if cancel_focused {
        kit::button(
            lang.text(Msg::MonitorKillConfirm),
            Some(MonitorMessage::ConfirmKill),
            Tone::Danger,
            palette,
        )
    } else {
        kit::enter_button(
            lang.text(Msg::MonitorKillConfirm),
            Some(MonitorMessage::ConfirmKill),
            Tone::Danger,
            palette,
        )
    };
    Some(
        Frame::new(
            lang.text(Msg::MonitorKillTitle),
            content,
            DialogSize::Medium,
            window_size,
            lang,
            palette,
        )
        .buttons(vec![cancel, confirm])
        .on_close(MonitorMessage::CancelKill)
        .hints(KeyHints {
            enter: true,
            tab: true,
        })
        .into_element(),
    )
}
