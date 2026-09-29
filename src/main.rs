//! NC-rs – a Norton Commander style dual-panel file manager.
//!
//! The async runtime is tokio: iced's `tokio` feature makes the iced executor
//! run every `Task` on a tokio runtime, so no `#[tokio::main]` is needed here.

mod app;
mod backend;
mod fs;
mod i18n;
mod keymap;
mod messages;
mod ui;

use app::App;

fn main() -> iced::Result {
    backend::log_backend();
    iced::application(App::title, App::update, App::view)
        .subscription(App::subscription)
        .theme(App::theme)
        .window_size(ui::layout::INITIAL_WINDOW_SIZE)
        .default_font(iced::Font::MONOSPACE)
        .antialiasing(true)
        .run_with(App::new)
}
