//! NC-rs – a dual-panel file manager inspired by Norton Commander.
//!
//! The async runtime is tokio: iced's `tokio` feature makes the iced executor
//! run every `Task` on a tokio runtime, so no `#[tokio::main]` is needed here.

#[cfg(test)]
mod e2e_tests;

#[cfg(test)]
mod screenshot_tests;
#[cfg(test)]
mod ui_tests;

mod ai_model;
mod app;
mod backend;
mod command_line;
mod command_palette;
mod commands;
mod config;
mod context_menu;
mod dialog;
mod execute;
mod filing;
mod filing_setup;
mod filing_status;
mod filing_work;
mod fs;
mod hermes_column;
mod hermes_status;
mod i18n;
mod jobs;
mod key_bindings;
mod keymap;
mod list_window;
#[cfg(any(target_os = "macos", test))]
mod menu;
mod messages;
#[cfg(target_os = "macos")]
mod native_menu;
mod ocr_models;
mod open_vsx;
mod palette;
mod query_work;
mod quick_fix;
mod quick_open;
mod remote_dialog;
mod reorganisation_plan;
mod reorganisation_work;
mod review_list;
mod review_work;
mod rule_suggestions;
mod saved_queries;
mod search_view;
mod selection;
mod session_state;
mod suggestion_work;
mod theme_fetch;
mod transfer_dialog;
mod ui;
mod vscode_theme;
mod vsix;

use app::App;
use std::env;

/// The application as iced runs it, window settings and all.
///
/// One definition, shared with the end-to-end tests in `e2e_tests.rs`. A test
/// that wires `update` differently from `main` proves nothing about the app the
/// user runs; the subscription is the part that matters most, because it is
/// what routes keys.
///
/// Returned as `Application`, not as `impl Program`: `Application::run` is an
/// inherent method, and an opaque return type would hide it from `main`.
///
/// `boot` builds the initial state, so tests can start the app in a scratch
/// directory while `main` passes `App::new`.
fn program(
    window_size: iced::Size,
    boot: impl Fn() -> (App, iced::Task<crate::messages::Message>) + 'static,
) -> iced::Application<impl iced::Program<State = App, Message = crate::messages::Message>> {
    iced::application(boot, App::update, App::view)
        .subscription(App::subscription)
        .theme(App::theme)
        .title(App::title)
        .window(iced::window::Settings {
            size: window_size,
            icon: window_icon(),
            ..window_frame()
        })
        .default_font(iced::Font::MONOSPACE)
        .antialiasing(true)
        // The close request becomes Quit, which saves the session state first.
        .exit_on_close_request(false)
}

/// The title bar row in the app draws the title and, off macOS, the window
/// buttons; the system frame gives way to it.
#[cfg(target_os = "macos")]
fn window_frame() -> iced::window::Settings {
    iced::window::Settings {
        platform_specific: iced::window::settings::PlatformSpecific {
            title_hidden: true,
            titlebar_transparent: true,
            fullsize_content_view: true,
        },
        ..iced::window::Settings::default()
    }
}

#[cfg(not(target_os = "macos"))]
fn window_frame() -> iced::window::Settings {
    iced::window::Settings {
        decorations: false,
        // Wayland docks and task switchers match a window to its launcher by
        // this id, so it has to be the basename of ncrs.desktop.
        #[cfg(target_os = "linux")]
        platform_specific: iced::window::settings::PlatformSpecific {
            application_id: "ncrs".to_owned(),
            ..iced::window::settings::PlatformSpecific::default()
        },
        ..iced::window::Settings::default()
    }
}

const WINDOW_ICON_SIZE: u32 = 128;
const WINDOW_ICON_RGBA: &[u8] = include_bytes!("../assets/icon-128.rgba");

/// Used by Linux and Windows; macOS takes the icon from the app bundle.
fn window_icon() -> Option<iced::window::Icon> {
    iced::window::icon::from_rgba(
        WINDOW_ICON_RGBA.to_vec(),
        WINDOW_ICON_SIZE,
        WINDOW_ICON_SIZE,
    )
    .ok()
}

/// `-V` / `--version` / `--help` print and exit without opening a window.
///
/// A bug report that cannot name the version is one that has to be followed
/// up. `--help` lives here too because this is the only place that knows the
/// binary's name and the graphics backend it was built with.
const HELP: &str = "\
ncrs - a dual-panel file manager inspired by Norton Commander

USAGE:
    ncrs

KEYS (the defaults; [keys] in the config file changes them):
    up/down    move selection      Tab       switch panel
    PgUp/PgDn  page                Backspace go up
    Home/End   first/last          Enter    open
    Ins/Space  tag a row           *        tag all
    F3 / F4    view / edit         F5 / F6  copy / move
    F7         create directory    Shift+F4 new file (opens in the editor)
    F8         delete (to trash)   Shift+F8 delete permanently
    F9         switch language (en/de)
    F10 / Cmd+Q  quit
    Alt+Left/Right  folder history back / forward (macOS also Cmd+[ / ])
    Ctrl+D / Cmd+D  add folder to favourites (listed in Alt+F1 / Alt+F2)

VIEW / EDIT:
    F3 and F4 open the file under the cursor (tags are ignored). The programs
    come from [open] view / edit in the config file; without them the system
    viewer and editor: open / open -t on macOS, xdg-open on Linux, explorer /
    notepad on Windows. $EDITOR is not used. edit must be a text editor: files
    that could run (scripts, apps) go there instead of being run.

CONFIG:
    ~/.config/ncrs/config.toml on macOS and Linux ($XDG_CONFIG_HOME/ncrs/
    config.toml on Linux if set), %APPDATA%\\ncrs\\config.toml on Windows. A
    folder named data next to the ncrs executable makes it portable: config.toml
    and index.db live there instead (not inside a macOS .app). A
    missing file means the defaults; an invalid one means the defaults plus the
    file and line in the status bar.

    state.toml next to index.db remembers the panels' folders and sort, the
    active panel and the window size. ncrs writes it; delete it to start fresh.

    [keys] gives an action one key or a list of keys; actions not listed keep
    their defaults, [] leaves an action without a key:
        [keys]
        copy = \"F2\"
        quit = [\"F10\", \"primary+q\"]
    primary is Cmd on macOS and Ctrl elsewhere; also ctrl, alt, shift, cmd.
    One key on two actions, Escape, and letters or digits without ctrl, alt
    or cmd (they jump to a file name) are errors. The full list of actions
    is in the README.

    [theme] picks a palette (preset \"cosmic\", the default, \"default\",
    \"classic\" or \"dracula\") and sets single colors on top of it as
    \"#rrggbb\" or \"#rgb\":
        [theme]
        preset = \"classic\"
        directory = \"#ffff55\"
    Roles: background, panel, text, dim_text, directory, cursor, cursor_text,
    tagged, border, bar, bar_inactive, status_bar, dialog, menu, accent,
    success, error. Read at start; text below 3:1 contrast is reported.
    vscode = \"themes/my-theme.json\" takes the colors of a VS Code theme file
    (path relative to the config folder), under the roles written here.

DELETE:
    F8 asks, then moves the tagged rows (or the row under the cursor) to the
    trash. If the trash cannot take them, nothing is deleted. Shift+F8 deletes
    for good: Enter cancels there, Shift+F8 again confirms.

Run ncrs without arguments; a GUI application with no arguments and no
files to open.";

fn main() -> iced::Result {
    match env::args().nth(1).as_deref() {
        Some("-V" | "--version") => {
            println!(
                "ncrs {} ({})",
                env!("CARGO_PKG_VERSION"),
                backend::Backend::CURRENT.name()
            );
            return Ok(());
        }
        Some("-h" | "--help") => {
            println!("{HELP}");
            return Ok(());
        }
        Some(other) => {
            eprintln!("ncrs: unknown option: {other}");
            eprintln!("try 'ncrs --help'");
            std::process::exit(2);
        }
        None => {}
    }

    backend::log_backend();

    let environment = app::Environment::of_the_user();
    fs::restore_grants(environment.index_path.as_deref());
    let saved = environment
        .state_path
        .as_deref()
        .map(session_state::load)
        .unwrap_or_default();
    let window_size = saved
        .window_size()
        .unwrap_or(ui::layout::INITIAL_WINDOW_SIZE);
    program(window_size, move || {
        App::new(environment.clone(), saved.clone())
    })
    .run()
}
