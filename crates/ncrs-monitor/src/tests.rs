#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::path::PathBuf;
use std::time::Duration;

use iced::{Point, Size};
use iced_test::Simulator;
use ncrs_config::system_monitor::{StatusStyle, StatusValue, SystemSettings, Visibility};
use ncrs_i18n::Language;
use ncrs_ui_kit::palette::ThemePreset;
use ncrs_ui_kit::theme;

use crate::state::MenuEntry;
use crate::*;

const WINDOW: Size = Size::new(1200.0, 760.0);

fn ctx() -> Ctx {
    Ctx {
        window_size: WINDOW,
        pointer: Point::new(300.0, 300.0),
    }
}

/// Only a path with a drive letter is absolute on Windows.
fn bin_folder() -> PathBuf {
    PathBuf::from(if cfg!(windows) {
        r"C:\usr\bin"
    } else {
        "/usr/bin"
    })
}

fn task(pid: u32, name: &str, cpu: f32, resident: u64) -> TaskRow {
    TaskRow {
        pid,
        start_time: 1_000 + u64::from(pid),
        name: name.to_owned(),
        user: "marc".to_owned(),
        cpu: Some(cpu),
        resident,
        cpu_seconds: 61 * u64::from(pid),
        status: Some(TaskStatus::Sleeping),
        command: format!("/usr/bin/{name} --flag"),
        program: Some(bin_folder().join(name)),
    }
}

fn network(name: &str, received: u64, sent: u64) -> NetworkRow {
    NetworkRow {
        name: name.to_owned(),
        received_per_second: Some(received),
        transmitted_per_second: Some(sent),
        has_traffic: received + sent > 0,
    }
}

fn snapshot() -> Snapshot {
    Snapshot {
        host: Some(HostInfo {
            name: "studio".to_owned(),
            os: "Linux 24.04".to_owned(),
            kernel: "6.8.0".to_owned(),
            arch: "x86_64".to_owned(),
            uptime_seconds: Some(3 * 86_400 + 4 * 3_600),
        }),
        cpu: Some(CpuInfo {
            brand: "Test CPU".to_owned(),
            cores: 8,
            percent: Some(69.0),
        }),
        memory: Some(Usage {
            total: 16 << 30,
            used: 11 << 30,
            free: 5 << 30,
        }),
        swap: Some(Usage {
            total: 4 << 30,
            used: 1 << 30,
            free: 3 << 30,
        }),
        load: Some(LoadInfo {
            one: 1.5,
            five: 1.2,
            fifteen: 0.9,
        }),
        networks: vec![
            network("lo", 5_000, 5_000),
            network("en0", 3_994, 30_618),
            network("idle0", 0, 0),
        ],
        sensors: vec![SensorRow {
            label: "CPU die".to_owned(),
            celsius: 54.0,
        }],
        tasks: vec![
            task(10, "alpha", 5.0, 100 << 20),
            task(20, "beta", 50.0, 300 << 20),
            task(30, "gamma", 20.0, 200 << 20),
        ],
        counts: Some(TaskCounts {
            total: 3,
            running: 1,
            sleeping: 2,
            other: 0,
        }),
    }
}

fn open_state(snapshot: Snapshot) -> State {
    let mut state = State::default();
    state.set_settings(SystemSettings::default());
    let _ = update(&mut state, MonitorMessage::Toggle, &ctx());
    state.test_put_snapshot(snapshot);
    state
}

fn badge_settings(values: &[StatusValue], style: StatusStyle) -> SystemSettings {
    SystemSettings {
        status_values: values.to_vec(),
        status_style: style,
        ..SystemSettings::default()
    }
}

#[test]
fn a_full_reading_without_cpu_use_asks_for_a_second_one_only_while_open() {
    use crate::update::lacks_cpu_reading;
    let mut first = snapshot();
    if let Some(cpu) = first.cpu.as_mut() {
        cpu.percent = None;
    }
    let open = open_state(snapshot());
    assert!(lacks_cpu_reading(&open, sampler::Depth::Full, &first));
    assert!(!lacks_cpu_reading(&open, sampler::Depth::Full, &snapshot()));
    let light = sampler::Depth::Light(sampler::Wanted::default());
    assert!(!lacks_cpu_reading(&open, light, &first));
    assert!(!lacks_cpu_reading(
        &State::default(),
        sampler::Depth::Full,
        &first
    ));
}

#[test]
fn the_tick_follows_what_there_is_to_show() {
    let mut state = State::default();
    assert_eq!(tick_every(&state), None, "closed, nothing selected");

    state.set_settings(badge_settings(&[StatusValue::Cpu], StatusStyle::Number));
    assert_eq!(
        tick_every(&state),
        Some(Duration::from_secs(2)),
        "closed, badge"
    );

    let _ = update(&mut state, MonitorMessage::Toggle, &ctx());
    assert!(state.is_open());
    assert_eq!(tick_every(&state), Some(Duration::from_secs(2)), "open");
}

#[test]
fn nothing_selected_frees_the_sampler_and_the_snapshot() {
    let mut state = State::default();
    state.set_settings(badge_settings(&[StatusValue::Cpu], StatusStyle::Number));
    state.test_put_snapshot(snapshot());
    assert!(state.snapshot().is_some());
    state.set_settings(badge_settings(&[], StatusStyle::Number));
    assert!(state.snapshot().is_none());
    assert!(!state.has_sampler());
    assert_eq!(tick_every(&state), None);
}

#[test]
fn the_badge_text_in_both_modes() {
    let values = [StatusValue::Cpu, StatusValue::Memory, StatusValue::Network];
    let mut state = State::default();
    state.set_settings(badge_settings(&values, StatusStyle::Number));
    state.test_put_snapshot(snapshot());
    assert_eq!(
        status_text(&state).as_deref(),
        Some("CPU 69%  MEM 69%  \u{2193}3.9K \u{2191}29.9K")
    );
    state.set_settings(badge_settings(&values, StatusStyle::Symbol));
    assert_eq!(
        status_text(&state).as_deref(),
        Some("CPU \u{2586}  MEM \u{2586}  \u{2193}3.9K \u{2191}29.9K")
    );
}

#[test]
fn a_value_not_measured_yet_shows_a_dash_and_none_selected_shows_nothing() {
    let mut state = State::default();
    state.set_settings(badge_settings(
        &[StatusValue::Cpu, StatusValue::Network],
        StatusStyle::Number,
    ));
    assert_eq!(
        status_text(&state).as_deref(),
        Some("CPU \u{2013}  \u{2193}\u{2013} \u{2191}\u{2013}")
    );
    state.set_settings(badge_settings(&[], StatusStyle::Number));
    assert_eq!(status_text(&state), None);
}

#[test]
fn the_cursor_stays_on_its_row_when_the_order_changes() {
    let mut state = open_state(snapshot());
    // Sorted by CPU: beta (50), gamma (20), alpha (5).
    let _ = update(&mut state, MonitorMessage::SelectRow(30), &ctx());
    assert_eq!(state.test_cursor_row(), 1);

    let mut next = snapshot();
    next.tasks[0].cpu = Some(90.0);
    next.tasks[1].cpu = Some(1.0);
    state.test_put_snapshot(next);
    assert_eq!(state.test_cursor_row(), 1, "alpha 90, gamma 20, beta 1");
    assert_eq!(state.pid_under_cursor(), Some(30), "gamma is still second");

    let mut other = snapshot();
    other.tasks[2].cpu = Some(60.0);
    state.test_put_snapshot(other);
    assert_eq!(state.test_cursor_row(), 1);
    assert_eq!(state.pid_under_cursor(), Some(20), "beta moved under it");
}

/// Two measurements with the order turned around: row and window stay.
#[test]
fn a_measurement_moves_neither_the_cursor_row_nor_the_window() {
    let visible = visible_rows(WINDOW.height);
    let reversed = || {
        let mut reversed = many_tasks(100);
        for (index, task) in reversed.tasks.iter_mut().enumerate() {
            task.cpu = Some(index as f32);
        }
        reversed
    };
    let mut state = open_state(many_tasks(100));
    assert_eq!(state.test_cursor_row(), 0);
    assert_eq!(state.pid_under_cursor(), Some(100), "the highest CPU");
    state.test_put_snapshot(reversed());
    assert_eq!(state.test_cursor_row(), 0);
    assert_eq!(state.window.offset(state.test_row_count(), visible), 0);
    assert_eq!(state.pid_under_cursor(), Some(199), "now the highest CPU");

    let _ = update(&mut state, MonitorMessage::Move(5), &ctx());
    state.test_put_snapshot(many_tasks(100));
    assert_eq!(state.test_cursor_row(), 5);
    assert_eq!(state.window.offset(state.test_row_count(), visible), 0);

    let _ = update(&mut state, MonitorMessage::Last, &ctx());
    let offset = state.window.offset(state.test_row_count(), visible);
    assert!(offset > 0);
    state.test_put_snapshot(reversed());
    assert_eq!(state.test_cursor_row(), 99);
    assert_eq!(state.window.offset(state.test_row_count(), visible), offset);
}

#[test]
fn a_list_that_gets_shorter_holds_the_cursor_inside() {
    let mut state = open_state(many_tasks(50));
    let _ = update(&mut state, MonitorMessage::Last, &ctx());
    assert_eq!(state.test_cursor_row(), 49);
    state.test_put_snapshot(many_tasks(10));
    assert_eq!(state.test_cursor_row(), 9);
}

#[test]
fn sorting_filtering_and_grouping_start_the_list_at_the_top() {
    let mut state = open_state(many_tasks(100));
    for message in [
        MonitorMessage::SortBy(TaskColumn::Memory),
        MonitorMessage::FilterChanged("t0".to_owned()),
        MonitorMessage::ToggleGroups,
    ] {
        let _ = update(&mut state, MonitorMessage::Last, &ctx());
        assert!(state.test_cursor_row() > 0);
        let _ = update(&mut state, message, &ctx());
        assert_eq!(state.test_cursor_row(), 0);
        assert_eq!(
            state
                .window
                .offset(state.test_row_count(), visible_rows(WINDOW.height)),
            0
        );
    }
}

/// The target is fixed when `k` is pressed: a later order does not change it.
#[test]
fn the_kill_dialog_keeps_the_task_it_was_asked_for() {
    let mut state = open_state(snapshot());
    let _ = update(&mut state, MonitorMessage::AskKill, &ctx());
    let before = state.kill_dialog.as_ref().unwrap().target.clone();
    assert_eq!(before.pid, 20, "beta is on top");

    let mut next = snapshot();
    next.tasks[0].cpu = Some(90.0);
    state.test_put_snapshot(next);
    assert_eq!(state.pid_under_cursor(), Some(10));
    assert_eq!(state.kill_dialog.as_ref().unwrap().target, before);
}

#[test]
fn the_menu_keeps_the_task_it_was_opened_on() {
    let mut state = open_state(snapshot());
    let _ = update(&mut state, MonitorMessage::MenuKey, &ctx());
    let mut next = snapshot();
    next.tasks[0].cpu = Some(90.0);
    state.test_put_snapshot(next);
    let copy = state
        .menu_entries()
        .iter()
        .position(|entry| *entry == MenuEntry::CopyCommandLine)
        .unwrap();
    let (_, action) = update(&mut state, MonitorMessage::MenuChoose(copy), &ctx());
    assert_eq!(
        action,
        Some(Action::CopyToClipboard("/usr/bin/beta --flag".to_owned()))
    );
}

#[test]
fn the_menu_goes_to_the_folder_of_the_task_it_was_opened_on() {
    let mut state = open_state(snapshot());
    let _ = update(&mut state, MonitorMessage::MenuKey, &ctx());
    let mut next = snapshot();
    next.tasks[0].cpu = Some(90.0);
    state.test_put_snapshot(next);
    let go = state.test_menu_index_of_go_to_folder();
    let (_, action) = update(&mut state, MonitorMessage::MenuChoose(go), &ctx());
    assert_eq!(
        action,
        Some(Action::GoToFolder {
            folder: bin_folder(),
            file: Some("beta".to_owned()),
        })
    );
}

#[test]
fn the_filter_narrows_the_rows_and_escape_takes_it_back() {
    let mut state = open_state(snapshot());
    let _ = update(
        &mut state,
        MonitorMessage::FilterChanged("amm".to_owned()),
        &ctx(),
    );
    assert_eq!(state.test_row_count(), 1);
    let (_, action) = update(&mut state, MonitorMessage::Escape, &ctx());
    assert_eq!(action, None);
    assert!(state.is_open(), "the first Esc leaves the filter field");
    assert_eq!(state.test_row_count(), 3);
}

#[test]
fn escape_closes_the_dialog_then_the_menu_then_the_page() {
    let mut state = open_state(snapshot());
    let _ = update(&mut state, MonitorMessage::SelectRow(10), &ctx());
    let _ = update(&mut state, MonitorMessage::OpenMenu(10), &ctx());
    assert!(menu_view(&state, view_ctx()).is_some());
    let _ = update(&mut state, MonitorMessage::Escape, &ctx());
    assert!(menu_view(&state, view_ctx()).is_none());
    assert!(state.is_open());

    if KILL_OFFERED {
        let _ = update(&mut state, MonitorMessage::AskKill, &ctx());
        assert!(dialog_view(&state, view_ctx()).is_some());
        let _ = update(&mut state, MonitorMessage::Escape, &ctx());
        assert!(dialog_view(&state, view_ctx()).is_none());
        assert!(state.is_open());
    }
    let (_, action) = update(&mut state, MonitorMessage::Escape, &ctx());
    assert_eq!(action, Some(Action::Closed));
    assert!(!state.is_open());
}

#[test]
fn go_to_folder_names_the_folder_and_the_file_and_closes_the_page() {
    let mut state = open_state(snapshot());
    let _ = update(&mut state, MonitorMessage::OpenMenu(10), &ctx());
    let go_to_folder = state.test_menu_index_of_go_to_folder();
    let (_, action) = update(&mut state, MonitorMessage::MenuChoose(go_to_folder), &ctx());
    assert_eq!(
        action,
        Some(Action::GoToFolder {
            folder: bin_folder(),
            file: Some("alpha".to_owned())
        })
    );
    assert!(!state.is_open());
}

#[test]
fn a_drive_click_opens_its_mount_point() {
    let mut state = open_state(snapshot());
    let (_, action) = update(
        &mut state,
        MonitorMessage::OpenDrive(PathBuf::from("/Volumes/x")),
        &ctx(),
    );
    assert_eq!(
        action,
        Some(Action::GoToFolder {
            folder: PathBuf::from("/Volumes/x"),
            file: None
        })
    );
    assert!(!state.is_open());
}

#[test]
fn the_own_process_cannot_be_killed_from_the_menu() {
    let mut own = snapshot();
    own.tasks[0].pid = std::process::id();
    let mut state = open_state(own);
    let _ = update(
        &mut state,
        MonitorMessage::SelectRow(std::process::id()),
        &ctx(),
    );
    let _ = update(&mut state, MonitorMessage::AskKill, &ctx());
    assert!(dialog_view(&state, view_ctx()).is_none());
}

#[test]
fn idle_interfaces_and_system_volumes_are_hidden_until_shown() {
    let mut state = open_state(snapshot());
    let names = |shown: &State| shown.test_network_names();
    assert_eq!(names(&state), ["lo", "en0"]);
    state.set_settings(SystemSettings {
        idle_interfaces: Visibility::Show,
        ..SystemSettings::default()
    });
    assert_eq!(names(&state), ["lo", "en0", "idle0"]);

    let volume = |name: &str, system: bool| DriveRow {
        name: name.to_owned(),
        mount: PathBuf::from(format!("/{name}")),
        file_system: "apfs".to_owned(),
        total: 100,
        available: 50,
        read_per_second: None,
        written_per_second: None,
        is_system_volume: system,
    };
    state.test_put_drives(vec![volume("data", false), volume("vm", true)]);
    assert_eq!(state.test_drive_names(), ["data"]);
    state.set_settings(SystemSettings {
        system_volumes: Visibility::Show,
        ..SystemSettings::default()
    });
    assert_eq!(state.test_drive_names(), ["data", "vm"]);
}

fn view_ctx() -> ViewCtx<'static> {
    static PALETTE: std::sync::OnceLock<ncrs_ui_kit::palette::Palette> = std::sync::OnceLock::new();
    ViewCtx {
        lang: Language::English,
        palette: PALETTE.get_or_init(|| ThemePreset::DEFAULT.palette()),
        window_size: WINDOW,
    }
}

fn matches_image(state: &State, name: &str) {
    let palette = ThemePreset::DEFAULT.palette();
    let content = iced::widget::container(view(state, view_ctx()))
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .style(theme::root(&palette));
    let mut simulator = Simulator::with_size(
        iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        },
        WINDOW,
        content,
    );
    let matches = simulator
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_image(format!("tests/snapshots/{name}.png"))
        .unwrap();
    assert!(matches, "{name} differs from its image");
}

fn drive(name: &str, mount: &str, file_system: &str) -> DriveRow {
    DriveRow {
        name: name.to_owned(),
        mount: PathBuf::from(mount),
        file_system: file_system.to_owned(),
        total: 500 << 30,
        available: 120 << 30,
        read_per_second: Some(1 << 20),
        written_per_second: Some(2 << 20),
        is_system_volume: false,
    }
}

fn sensor(label: &str, celsius: f32) -> SensorRow {
    SensorRow {
        label: label.to_owned(),
        celsius,
    }
}

fn host(os: &str, kernel: &str, arch: &str) -> HostInfo {
    HostInfo {
        name: "studio".to_owned(),
        os: os.to_owned(),
        kernel: kernel.to_owned(),
        arch: arch.to_owned(),
        uptime_seconds: Some(3 * 86_400 + 4 * 3_600),
    }
}

fn many_tasks(count: u32) -> Snapshot {
    let mut many = snapshot();
    many.tasks = (0..count)
        .map(|index| {
            task(
                100 + index,
                &format!("t{index:03}"),
                100.0 - index as f32,
                1 << 20,
            )
        })
        .collect();
    many
}

/// The first row starts where the layout constants say, and the last row in
/// view ends inside the page without room for one more.
#[test]
fn the_rows_fill_the_page_without_a_cut_off_row() {
    use crate::layout::{visible_rows, ROWS_BOTTOM, ROWS_TOP};
    let palette = ThemePreset::DEFAULT.palette();
    let row_height = ncrs_ui_kit::layout::ROW_HEIGHT;
    for height in [480.0, 600.0, 760.0, 901.0] {
        let window = Size::new(1200.0, height);
        let mut state = open_state(many_tasks(80));
        let _ = update(
            &mut state,
            MonitorMessage::Measured(sampler::Depth::Full, Box::new(many_tasks(80))),
            &Ctx {
                window_size: window,
                ..ctx()
            },
        );
        let page = ViewCtx {
            window_size: window,
            ..view_ctx()
        };
        let content = iced::widget::column![
            iced::widget::Space::new().height(theme::size::TITLE_BAR),
            view(&state, page),
        ];
        let mut simulator = Simulator::with_size(
            iced::Settings {
                default_font: ncrs_ui_kit::APP_FONT,
                ..iced::Settings::default()
            },
            window,
            iced::widget::container(content).style(theme::root(&palette)),
        );
        let rows = visible_rows(height);
        let row_top = |shown: &mut Simulator<'_, MonitorMessage>, index: usize| {
            let line = format!("/usr/bin/t{index:03} --flag");
            let found = shown.find(line.as_str()).unwrap();
            found.bounds().center_y() - row_height / 2.0
        };
        let first = row_top(&mut simulator, 0);
        assert!(
            (first - ROWS_TOP).abs() < 3.0,
            "{height}: {first} vs {ROWS_TOP}"
        );
        let last_bottom = row_top(&mut simulator, rows - 1) + row_height;
        let room_end = height - ROWS_BOTTOM;
        assert!(last_bottom <= room_end + 3.0, "cut off at {height}");
        assert!(last_bottom + row_height > room_end - 3.0, "gap at {height}");
    }
}

fn grouped_snapshot() -> Snapshot {
    let mut grouped = snapshot();
    grouped.tasks = vec![
        task(30, "Helper (Renderer)", 10.0, 100 << 20),
        task(10, "Helper", 5.0, 50 << 20),
        task(20, "solo", 12.0, 10 << 20),
    ];
    grouped
}

#[test]
fn the_cursor_stays_on_its_row_while_the_programs_change() {
    let mut state = open_state(grouped_snapshot());
    let _ = update(&mut state, MonitorMessage::ToggleGroups, &ctx());
    assert_eq!(state.test_row_count(), 2);
    let _ = update(&mut state, MonitorMessage::Move(1), &ctx());
    assert_eq!(state.pid_under_cursor(), Some(20), "solo is second");

    let mut next = grouped_snapshot();
    next.tasks[2].cpu = Some(99.0);
    state.test_put_snapshot(next);
    assert_eq!(state.test_cursor_row(), 1);
    assert_eq!(state.pid_under_cursor(), Some(10), "Helper moved under it");
}

#[test]
fn a_program_path_that_is_not_absolute_has_no_folder_to_go_to() {
    let state = open_state(snapshot());
    for program in ["sftp://marc@host/x/prog", "bin/prog"] {
        let mut task = state.task_by_pid(20).unwrap().clone();
        task.program = Some(PathBuf::from(program));
        assert!(!state.entry_is_enabled(MenuEntry::GoToFolder, &task));
        assert!(crate::state::program_folder(&task).is_none());
    }
}

#[test]
fn pid_zero_offers_no_kill() {
    let state = open_state(snapshot());
    let mut task = state.task_by_pid(20).unwrap().clone();
    task.pid = 0;
    assert!(!state.entry_is_enabled(MenuEntry::Kill, &task));
}

#[test]
fn nothing_is_killed_while_grouped_not_even_a_group_of_one() {
    let mut state = open_state(grouped_snapshot());
    let _ = update(&mut state, MonitorMessage::ToggleGroups, &ctx());
    for pid in [10, 20] {
        let _ = update(&mut state, MonitorMessage::SelectRow(pid), &ctx());
        let _ = update(&mut state, MonitorMessage::AskKill, &ctx());
        assert!(dialog_view(&state, view_ctx()).is_none());
        let _ = update(&mut state, MonitorMessage::OpenMenu(pid), &ctx());
        assert!(!state.menu_entries().contains(&MenuEntry::Kill));
        assert!(!state.entry_is_enabled(MenuEntry::Kill, &state.task_by_pid(pid).unwrap().clone()));
    }
    let _ = update(&mut state, MonitorMessage::ToggleGroups, &ctx());
    let _ = update(&mut state, MonitorMessage::OpenMenu(10), &ctx());
    assert_eq!(
        state.menu_entries().contains(&MenuEntry::Kill),
        KILL_OFFERED
    );
}

#[test]
fn a_group_copies_and_opens_the_task_with_the_smallest_pid() {
    let mut state = open_state(grouped_snapshot());
    let _ = update(&mut state, MonitorMessage::ToggleGroups, &ctx());
    let _ = update(&mut state, MonitorMessage::SelectRow(10), &ctx());
    let _ = update(&mut state, MonitorMessage::OpenMenuAtCursor, &ctx());
    let copy = state
        .menu_entries()
        .iter()
        .position(|entry| *entry == MenuEntry::CopyCommandLine)
        .unwrap();
    let (_, action) = update(&mut state, MonitorMessage::MenuChoose(copy), &ctx());
    assert_eq!(
        action,
        Some(Action::CopyToClipboard("/usr/bin/Helper --flag".to_owned()))
    );
}

#[test]
fn the_option_tap_opens_the_menu_on_the_first_row_and_the_next_one_closes_it() {
    let mut state = open_state(snapshot());
    let _ = update(&mut state, MonitorMessage::MenuKey, &ctx());
    assert!(state.menu_is_open());
    let origin = state.menu.as_ref().unwrap().origin;
    assert!(
        (origin.y - (crate::layout::ROWS_TOP + ncrs_ui_kit::layout::ROW_HEIGHT)).abs() < 0.5,
        "right under the first row"
    );
    let _ = update(&mut state, MonitorMessage::MenuKey, &ctx());
    assert!(!state.menu_is_open());
}

#[test]
fn grouping_is_off_again_when_the_page_opens() {
    let mut state = open_state(grouped_snapshot());
    let _ = update(&mut state, MonitorMessage::ToggleGroups, &ctx());
    assert!(state.is_grouped());
    let _ = update(&mut state, MonitorMessage::Close, &ctx());
    let _ = update(&mut state, MonitorMessage::Toggle, &ctx());
    assert!(!state.is_grouped());
}

/// Named for CI, which compares pixels only on macOS.
mod ui_tests {
    use super::*;

    /// macOS: load average, process states and sensors; the volume is APFS.
    #[test]
    fn page_with_the_blocks_of_macos() {
        let mut mac = snapshot();
        mac.host = Some(host("macOS 15.4", "Darwin 24.4.0", "arm64"));
        mac.sensors = vec![
            sensor("CPU efficiency cores", 41.0),
            sensor("CPU performance cores", 54.0),
            sensor("GPU", 46.0),
        ];
        let mut state = open_state(mac);
        state.test_put_drives(vec![drive("Macintosh HD", "/", "apfs")]);
        matches_image(&state, "system-page-macos");
    }

    /// Linux: load average, process states and sensors; volumes are named by device.
    #[test]
    fn page_with_the_blocks_of_linux() {
        let mut linux = snapshot();
        linux.host = Some(host("Ubuntu 24.04", "6.8.0-45-generic", "x86_64"));
        linux.sensors = vec![
            sensor("coretemp Package id 0", 62.0),
            sensor("nvme Composite", 38.0),
        ];
        let mut state = open_state(linux);
        state.test_put_drives(vec![
            drive("/dev/nvme0n1p2", "/", "ext4"),
            drive("/dev/sda1", "/mnt/data", "ext4"),
        ]);
        matches_image(&state, "system-page-linux");
    }

    /// Windows: no load average, no process states, so no counts and a dash in the
    /// state column, and no sensors.
    #[test]
    fn page_with_the_blocks_of_windows() {
        let mut windows = snapshot();
        windows.host = Some(host("Windows 11 (26100)", "26100", "x86_64"));
        windows.load = None;
        windows.counts = None;
        windows.sensors.clear();
        for task in &mut windows.tasks {
            task.status = None;
        }
        let mut state = open_state(windows);
        state.test_put_drives(vec![drive("Windows", "C:\\", "NTFS")]);
        matches_image(&state, "system-page-windows");
    }

    /// Grouped by program: the helpers of one browser are one row with a count.
    #[test]
    fn page_grouped_by_program() {
        let mut mac = grouped_snapshot();
        mac.host = Some(host("macOS 15.4", "Darwin 24.4.0", "arm64"));
        mac.tasks.push(task(40, "Helper (GPU)", 8.0, 80 << 20));
        let mut state = open_state(mac);
        state.test_put_drives(vec![drive("Macintosh HD", "/", "apfs")]);
        let _ = update(&mut state, MonitorMessage::ToggleGroups, &ctx());
        matches_image(&state, "system-page-grouped");
    }
}

#[tokio::test]
async fn a_full_reading_without_cpu_use_makes_the_page_measure_again() {
    use iced_test::futures::futures::StreamExt as _;
    use iced_test::runtime::{task, Action};

    let mut first = snapshot();
    if let Some(cpu) = first.cpu.as_mut() {
        cpu.percent = None;
    }
    let mut state = open_state(snapshot());

    let (without_cpu, _) = update(
        &mut state,
        MonitorMessage::Measured(sampler::Depth::Full, Box::new(first)),
        &ctx(),
    );
    let mut stream = task::into_stream(without_cpu).expect("a follow-up measurement is scheduled");
    let mut follow_up = None;
    while let Some(action) = stream.next().await {
        if let Action::Output(message) = action {
            follow_up = Some(message);
        }
    }
    assert!(matches!(follow_up, Some(MonitorMessage::MeasureAgain)));

    let (with_cpu, _) = update(
        &mut state,
        MonitorMessage::Measured(sampler::Depth::Full, Box::new(snapshot())),
        &ctx(),
    );
    assert!(task::into_stream(with_cpu).is_none());
}
