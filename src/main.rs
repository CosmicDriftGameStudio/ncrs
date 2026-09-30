//! NC-rs – a Norton Commander style dual-panel file manager.
//!
//! The async runtime is tokio: iced's `tokio` feature makes the iced executor
//! run every `Task` on a tokio runtime, so no `#[tokio::main]` is needed here.

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

    iced::application(App::new, App::update, App::view)
        .subscription(App::subscription)
        .theme(App::theme)
        .title(App::title)
        .window_size(ui::layout::INITIAL_WINDOW_SIZE)
        .default_font(iced::Font::MONOSPACE)
        .antialiasing(true)
        .run()
}
