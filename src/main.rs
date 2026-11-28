//! ncrs – a fast, keyboard-first dual-panel file manager.
//!
//! The async runtime is tokio: iced's `tokio` feature makes the iced executor
//! run every `Task` on a tokio runtime, so no `#[tokio::main]` is needed here.

// Windows release builds are GUI programs, so a double click opens no console
// window. A command line run attaches to its parent's console in `main`.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(test)]
mod e2e_tests;

#[cfg(test)]
mod hermes_protocol;
// The generators of the README, store and trailer pictures: they assert
// nothing, so the normal test build leaves them out.
#[cfg(all(test, feature = "media-tools"))]
mod repos_scene;
#[cfg(all(test, feature = "media-tools"))]
mod screenshot_scenes;
#[cfg(all(test, feature = "media-tools"))]
mod screenshot_tests;
#[cfg(test)]
mod shot_file;
#[cfg(all(test, feature = "media-tools"))]
mod trailer;
#[cfg(all(test, feature = "media-tools"))]
mod trailer_data;
#[cfg(test)]
mod ui_tests;

mod app;
mod backend;
pub use ncrs_ops::clipboard_paste;
#[cfg(test)]
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod clipboard_system_tests;
pub use ncrs_config::commands;
pub use ncrs_config::config;
pub use ncrs_palette::command_line;
mod context_action_message;
pub use ncrs_config::file_list_tags;
pub use ncrs_files::context_menu;
pub use ncrs_fs as fs;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use ncrs_ops::drag_out;
mod input_state;
pub use ncrs_filing::{execute, filing, query_work, rules_work, tags_work};
pub use ncrs_hermes_ui::{
    condition_editor, filing_setup, filing_status, hermes_column, hermes_overview, hermes_status,
    reading_again, rule_form, rules_page, saved_queries, tags_page,
};
pub use ncrs_i18n as i18n;
pub use ncrs_jobs as jobs;
mod git_keys;
mod keymap;
mod repos_keys;
mod space_keys;
pub use ncrs_ui_kit::list_window;
#[cfg(any(target_os = "macos", test))]
mod menu;
mod messages;
#[cfg(target_os = "macos")]
mod native_menu;
pub use ncrs_config::palette;
pub use ncrs_core::selection;
pub use ncrs_files::panel_drag;
pub use ncrs_palette::quick_open;
pub use ncrs_remote::dialog as remote_dialog;
pub use ncrs_search::view_state as search_view;
#[cfg(test)]
#[cfg(target_os = "macos")]
mod sandbox_probe_tests;
mod session_state;
mod system_dialogs;
pub use ncrs_ops::prompt as dialog;
pub use ncrs_ops::transfer as transfer_dialog;
pub use ncrs_ops::trash as trash_view;
pub use ncrs_settings::{ai_model, key_bindings, ocr_models, open_vsx, theme_fetch, vsix};
mod ui;
pub use ncrs_config::vscode_theme;

use app::App;
use std::env;
use std::path::Path;

/// The application as iced runs it, window settings and all.
///
/// One definition, shared with the end-to-end tests in `e2e_tests/mod.rs`. A test
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
    let application = iced::application(boot, App::update, App::view)
        .subscription(App::subscription)
        .theme(App::theme)
        .scale_factor(App::scale_factor)
        .title(App::title)
        .window(iced::window::Settings {
            size: window_size,
            icon: window_icon(),
            ..window_frame()
        })
        .default_font(ncrs_ui_kit::APP_FONT)
        .antialiasing(true)
        // The close request becomes Quit, which saves the session state first.
        .exit_on_close_request(false);
    ncrs_ui_kit::BUNDLED_FONTS
        .iter()
        .fold(application, |app, font| app.font(*font))
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
ncrs - a fast, keyboard-first dual-panel file manager

USAGE:
    ncrs
    ncrs <path>...
        Opens a folder in the active panel, or a file's folder with the cursor
        on the file. A file:// URI works too. If ncrs is already running, that
        window shows it and comes to the front.
    ncrs --check-ai <models-dir>
        Loads the Hermes model from <models-dir> without opening a window,
        prints the compute devices, GPU or CPU and the time of three
        sentences, and exits 0 if all went well, 1 if not, 2 on a usage
        error.
        On Windows ncrs is a window program, so cmd.exe and PowerShell do not
        wait for it: run it as start /wait ncrs --check-ai <models-dir>
        (PowerShell: Start-Process -Wait -PassThru) to get the exit code in
        %ERRORLEVEL% (.ExitCode).

KEYS (the defaults; [keys] in the config file changes them):
    up/down    move selection      Tab       switch panel
    PgUp/PgDn  page                Backspace go up
    Home/End   first/last          Enter    open
    Ins/Space  tag a row           *        tag all
    F1         command list        F2       Hermes chat (Beta)
    Shift+F2   command line        F12      Monitor
    F3 / F4    view / edit         F5 / F6  copy / move
    F7         create directory    Shift+F4 new file (opens in the editor)
    F8         delete (to trash)   Shift+F8 delete permanently
    Shift+F6   rename in place
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
    active panel, the window size and your favourites. ncrs writes it; delete it
    to start fresh, which also clears the favourites. A file that does not parse
    is kept as state.toml.broken.

    --data-dir DIR keeps config, index, state and journal in DIR and the index
    key as a file there instead of the system keychain. For tests with
    throwaway data, not for real data.

    journal.toml next to state.toml records what ncrs moved to the trash on
    macOS, so the trash view can list it and put it back.

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
files to open.

OPEN FROM OTHER PROGRAMS:
    macOS: ncrs is listed under Open With in the Finder; open -a ncrs <path>
    and dropping a folder on the Dock icon work. Linux: ncrs is listed for
    folders, and while it runs it answers \"Show in folder\" (the
    org.freedesktop.FileManager1 service). Windows and Linux: the program
    settings can make ncrs the default for folders; Windows can add it to
    the Explorer context menu.";

/// `ncrs --check-ai <models-dir>`: developer and CI output in English, not UI
/// text, so it is not in strings.json. Returns the exit code.
#[cfg(feature = "local-ai")]
fn check_ai(models_dir_argument: Option<std::ffi::OsString>) -> i32 {
    let Some(models_dir) = models_dir_argument else {
        eprintln!("ncrs: --check-ai needs the folder with the model");
        return 2;
    };
    match ncrs_ai::check_local_model(std::path::Path::new(&models_dir)) {
        Ok(check) => {
            for device in &check.devices {
                println!("ai device: {device}");
            }
            match &check.compute {
                ncrs_ai::Compute::Cpu => println!("ai compute: CPU"),
                ncrs_ai::Compute::Gpu(device) => println!("ai compute: GPU {device}"),
            }
            for (number, time) in check.sentence_times.iter().enumerate() {
                println!("ai sentence {} {} ms", number + 1, time.as_millis());
            }
            0
        }
        Err(error) => {
            eprintln!("ncrs: the AI check failed: {error}");
            1
        }
    }
}

#[cfg(not(feature = "local-ai"))]
fn check_ai(_models_dir: Option<std::ffi::OsString>) -> i32 {
    eprintln!("ncrs: this build has no built-in AI model (feature local-ai)");
    2
}

/// `--data-dir DIR`: the folder is created when missing and made absolute;
/// `None` when that is not possible.
fn data_dir_of(value: Option<std::ffi::OsString>) -> Option<std::path::PathBuf> {
    let given = std::path::PathBuf::from(value?);
    std::fs::create_dir_all(&given).ok()?;
    std::fs::canonicalize(&given)
        .ok()
        .filter(|absolute| absolute.is_dir())
}

fn main() -> iced::Result {
    // Git and ssh start this binary as their askpass helper: that run answers
    // the prompt and exits, before anything else (console, instance lock, GUI).
    // The program name stays in front: the prompt is the argument after it.
    let helper_arguments: Vec<String> = env::args_os()
        .map(|given| given.to_string_lossy().into_owned())
        .collect();
    if let Some(code) = ncrs_git::askpass::run_if_asked(&helper_arguments) {
        std::process::exit(code);
    }
    let mut arguments: Vec<std::ffi::OsString> = env::args_os().skip(1).collect();
    if !arguments.is_empty() {
        // The Windows build has no console of its own; every command line run
        // prints to the one it was started from.
        ncrs_console::attach_to_parent_console();
    }
    if let Some(at) = arguments.iter().position(|given| given == "--data-dir") {
        let Some(dir) = data_dir_of(arguments.get(at + 1).cloned()) else {
            eprintln!("ncrs: --data-dir needs a folder");
            eprintln!("try 'ncrs --help'");
            std::process::exit(2);
        };
        config::use_data_dir(dir);
        arguments.drain(at..(at + 2).min(arguments.len()));
    }
    let argument = arguments
        .first()
        .map(|given| given.to_string_lossy().into_owned());
    match argument.as_deref() {
        Some("-V" | "--version") => {
            println!(
                "ncrs {} ({})",
                env!("NCRS_DISPLAY_VERSION"),
                backend::Backend::CURRENT.name()
            );
            return Ok(());
        }
        Some("-h" | "--help") => {
            println!("{HELP}");
            return Ok(());
        }
        Some("--check-ai") => std::process::exit(check_ai(arguments.get(1).cloned())),
        Some(path) if !path.starts_with('-') => {}
        Some(other) => {
            eprintln!("ncrs: unknown option: {other}");
            eprintln!("try 'ncrs --help'");
            std::process::exit(2);
        }
        None => {}
    }

    let opening = match requests_of(&arguments) {
        Ok(requests) => requests,
        Err(missing) => {
            eprintln!("ncrs: no such file or folder: {missing}");
            std::process::exit(2);
        }
    };

    backend::log_backend();

    let mut environment = app::Environment::of_the_user();
    fs::restore_grants(environment.index_path.as_deref());
    fs::warm_up_terminal_detection();
    let data_dir = environment
        .index_path
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    let instance = data_dir.as_deref().map(config::instance_lock::acquire);
    match (&instance, data_dir.as_deref()) {
        // The running ncrs opens it; a start that cannot reach it goes on as
        // a window of its own.
        (Some(config::instance_lock::Instance::Second), Some(dir))
            if !opening.is_empty() && ncrs_handoff::send(dir, &opening).is_ok() =>
        {
            std::process::exit(0);
        }
        (Some(config::instance_lock::Instance::First(_)), Some(dir)) => {
            ncrs_handoff::discard_stale(dir);
            // Our own inbox: the app reads it right after the window is up.
            if !opening.is_empty() {
                drop(ncrs_handoff::send(dir, &opening));
            }
            #[cfg(target_os = "linux")]
            ncrs_handoff::serve_file_manager(dir.to_path_buf());
        }
        _ => {}
    }
    let saved = environment
        .state_path
        .as_deref()
        .map(session_state::load)
        .unwrap_or_default();
    let window_size = saved
        .window_size()
        .unwrap_or(ui::layout::INITIAL_WINDOW_SIZE);
    if matches!(instance, Some(config::instance_lock::Instance::Second)) {
        environment = environment.for_second_instance();
    }
    // `instance` holds the lock until the process ends.
    program(window_size, move || {
        let (app, started) = App::new(environment.clone(), saved.clone());
        // Paths from the command line wait in the inbox, and on macOS the
        // system may have opened files already.
        let handed_over = app.poll_handoffs();
        (app, iced::Task::batch([started, handed_over]))
    })
    .run()
}

/// The paths of the command line as requests: every argument is a folder or
/// file, or a `file://` URI of one. `Err` names the first that does not exist.
fn requests_of(arguments: &[std::ffi::OsString]) -> Result<Vec<ncrs_handoff::Request>, String> {
    if arguments
        .first()
        .is_none_or(|first| first.to_string_lossy().starts_with('-'))
    {
        return Ok(Vec::new());
    }
    let cwd = env::current_dir().unwrap_or_default();
    arguments
        .iter()
        .map(|given| {
            let path = ncrs_handoff::path_from_argument(given, &cwd);
            let request = ncrs_handoff::Request::Open(path);
            match request.target() {
                Some(_) => Ok(request),
                None => Err(given.to_string_lossy().into_owned()),
            }
        })
        .collect()
}
