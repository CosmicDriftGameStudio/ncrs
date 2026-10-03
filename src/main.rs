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

mod app;
mod backend;
mod config;
mod context_menu;
mod dialog;
mod fs;
mod i18n;
mod jobs;
mod keymap;
mod messages;
mod selection;
mod ui;

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
    boot: impl Fn() -> (App, iced::Task<crate::messages::Message>) + 'static,
) -> iced::Application<impl iced::Program<State = App, Message = crate::messages::Message>> {
    iced::application(boot, App::update, App::view)
        .subscription(App::subscription)
        .theme(App::theme)
        .title(App::title)
        .window(iced::window::Settings {
            size: ui::layout::INITIAL_WINDOW_SIZE,
            icon: window_icon(),
            ..iced::window::Settings::default()
        })
        .default_font(iced::Font::MONOSPACE)
        .antialiasing(true)
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

KEYS:
    up/down    move selection      Tab       switch panel
    PgUp/PgDn  page                Backspace go up
    Home/End   first/last          Enter    open
    Ins/Space  tag a row           *        tag all
    F3 / F4    view / edit         F5 / F6  copy / move
    F7         create directory
    F8         delete (to trash)   Shift+F8 delete permanently
    F9         switch language (en/de)
    F10 / Cmd+Q  quit

VIEW / EDIT:
    F3 and F4 open the file under the cursor (tags are ignored). The programs
    come from [open] view / edit in the config file; without them the system
    viewer and editor: open / open -t on macOS, xdg-open on Linux, explorer /
    notepad on Windows. $EDITOR is not used. edit must be a text editor: files
    that could run (scripts, apps) go there instead of being run.

CONFIG:
    ~/.config/ncrs/config.toml on macOS and Linux ($XDG_CONFIG_HOME/ncrs/
    config.toml on Linux if set), %APPDATA%\\ncrs\\config.toml on Windows. A
    missing file means the defaults; an invalid one means the defaults plus the
    file and line in the status bar.

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

    program(App::new).run()
}
