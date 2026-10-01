//! NC-rs – a Norton Commander style dual-panel file manager.
//!
//! The async runtime is tokio: iced's `tokio` feature makes the iced executor
//! run every `Task` on a tokio runtime, so no `#[tokio::main]` is needed here.

#[cfg(test)]
mod e2e_tests;

#[cfg(test)]
mod ui_tests;

mod app;
mod backend;
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
        .window_size(ui::layout::INITIAL_WINDOW_SIZE)
        .default_font(iced::Font::MONOSPACE)
        .antialiasing(true)
}

/// `-V` / `--version` / `--help` print and exit without opening a window.
///
/// A bug report that cannot name the version is one that has to be followed
/// up. `--help` lives here too because this is the only place that knows the
/// binary's name and the graphics backend it was built with.
const HELP: &str = "\
ncrs - a Norton Commander style dual-panel file manager

USAGE:
    ncrs

KEYS:
    up/down    move selection      Tab       switch panel
    PgUp/PgDn  page                Backspace go up
    Home/End   first/last          Enter    open
    Insert     tag a row           *        tag all
    F7         create directory    F9       switch language (en/de)
    F10 / Q    quit

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
