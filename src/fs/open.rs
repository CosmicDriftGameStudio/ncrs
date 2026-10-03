//! Handing a file to an external program, for F3 (view) and F4 (edit).
//!
//! The programs are configurable (`[open]` in the config file); the system's
//! own are the defaults.
//!
//! Opening must never run anything. The system opener performs the default
//! action, and for a script, an app bundle or a shortcut that is "run it".
//! Such a file (`could_execute`) goes to the edit program instead, which is
//! therefore expected to be a text editor, or is refused where there is none.
//! An extension list is never complete: on Unix the exec bit catches the
//! rest, Windows depends on the list alone.
//!
//! Starting is not blocking: the program is spawned and reaped on a thread of
//! its own, so the window stays usable while a viewer is open.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::{OpenPrograms, ProgramLine};

/// What the user wants done with the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenKind {
    View,
    Edit,
}

/// A program and its arguments, ready to start. Built by `open_command`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCommand {
    pub program: OsString,
    pub args: Vec<OsString>,
}

/// Where F3 and F4 start a program. A trait so the end-to-end tests can hand
/// the app a launcher of their own: a test must never open a real window.
pub trait Launcher: Send + Sync {
    /// Starts `command` and returns without waiting for it. The error is
    /// already text, for the status line.
    ///
    /// Refusing a file is decided before, in `open_command`: the launcher
    /// starts what it is given.
    fn launch(&self, command: &OpenCommand) -> Result<(), String>;
}

/// The operating system's way to start a program, through
/// `std::process::Command`.
#[derive(Debug)]
pub struct SystemLauncher;

impl Launcher for SystemLauncher {
    fn launch(&self, command: &OpenCommand) -> Result<(), String> {
        let mut child = Command::new(&command.program)
            .args(&command.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| format!("{}: {err}", command.program.to_string_lossy()))?;
        // Reaped on a thread, or every opened file would leave a zombie.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}

/// Extensions of files the system opener would run or install rather than
/// show, compared case-insensitively.
const EXECUTABLE_EXTENSIONS: &[&str] = &[
    "app",
    "command",
    "tool",
    "terminal",
    "workflow",
    "action",
    "scpt",
    "applescript",
    "sh",
    "bash",
    "zsh",
    "csh",
    "py",
    "pl",
    "rb",
    "desktop",
    "appimage",
    "run",
    "bin",
    "exe",
    "com",
    "bat",
    "cmd",
    "msi",
    "msc",
    "lnk",
    "url",
    "scr",
    "pif",
    "ps1",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "hta",
    "cpl",
    "jar",
    "reg",
];

/// Whether the system opener might run `path` instead of showing it: an
/// executable file (the link is followed, as the opener follows it) or an
/// extension from `EXECUTABLE_EXTENSIONS`.
pub fn could_execute(path: &Path) -> bool {
    let listed = path.extension().is_some_and(|raw| {
        let lowered = raw.to_string_lossy().to_ascii_lowercase();
        EXECUTABLE_EXTENSIONS.contains(&lowered.as_str())
    });
    listed || has_exec_bit(path)
}

#[cfg(unix)]
fn has_exec_bit(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn has_exec_bit(_path: &Path) -> bool {
    false
}

/// The system's own view and edit programs, used where the config names none.
/// Linux has no generic editor, so there it is `None`.
fn default_programs() -> (ProgramLine, Option<ProgramLine>) {
    #[cfg(target_os = "macos")]
    {
        (
            ProgramLine::new("open", &[]),
            Some(ProgramLine::new("open", &["-t"])),
        )
    }
    #[cfg(windows)]
    {
        (
            ProgramLine::new("explorer", &[]),
            Some(ProgramLine::new("notepad", &[])),
        )
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        (ProgramLine::new("xdg-open", &[]), None)
    }
}

/// The command that opens `path`, or `None` when nothing safe exists. Never a
/// shell: the path is one argument of its own, so `&` or a space in a name
/// cannot become a command.
///
/// The programs come from the config, the system's own fill in what it leaves
/// out. With `executable` set the file goes to the edit program for both
/// kinds, never to the view program: the system opener would run it, so the
/// edit program has to be a text editor. Without one (Linux default) nothing
/// is opened. Edit without an edit program falls back to the view program.
///
/// A relative path is anchored with `./`, so a name that starts with `-` is
/// not read as an option.
// ponytail: no $EDITOR/$VISUAL, the app is a GUI without a terminal and vim
// would start into nothing.
pub fn open_command(
    programs: &OpenPrograms,
    kind: OpenKind,
    path: &Path,
    executable: bool,
) -> Option<OpenCommand> {
    let (default_view, default_edit) = default_programs();
    let view = programs.view.clone().unwrap_or(default_view);
    let edit = programs.edit.clone().or(default_edit);
    let chosen = if executable {
        edit?
    } else {
        match kind {
            OpenKind::View => view,
            OpenKind::Edit => edit.unwrap_or(view),
        }
    };
    let anchored: PathBuf = if path.is_absolute() {
        path.to_path_buf()
    } else {
        Path::new(".").join(path)
    };
    let mut args: Vec<OsString> = chosen.args().iter().map(OsString::from).collect();
    args.push(anchored.into_os_string());
    Some(OpenCommand {
        program: chosen.program().into(),
        args,
    })
}

// reason: a failing assertion is the signal in a test
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    fn command(kind: OpenKind, path: &Path, executable: bool) -> Option<OpenCommand> {
        open_command(&OpenPrograms::default(), kind, path, executable)
    }

    fn with(program: &str, args: &[&str], path: &Path) -> OpenCommand {
        let mut all: Vec<OsString> = args.iter().map(OsString::from).collect();
        all.push(path.as_os_str().to_owned());
        OpenCommand {
            program: program.into(),
            args: all,
        }
    }

    fn configured(view: Option<ProgramLine>, edit: Option<ProgramLine>) -> OpenPrograms {
        OpenPrograms { view, edit }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_views_with_open_and_edits_in_the_text_editor() {
        let path = Path::new("/tmp/a & b.txt");
        assert_eq!(
            command(OpenKind::View, path, false),
            Some(with("open", &[], path))
        );
        assert_eq!(
            command(OpenKind::Edit, path, false),
            Some(with("open", &["-t"], path))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_shows_an_executable_as_text_for_both_kinds() {
        let path = Path::new("/tmp/run.command");
        for kind in [OpenKind::View, OpenKind::Edit] {
            assert_eq!(command(kind, path, true), Some(with("open", &["-t"], path)));
        }
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    #[test]
    fn linux_opens_both_kinds_with_xdg_open_and_refuses_an_executable() {
        let path = Path::new("/tmp/a & b.txt");
        for kind in [OpenKind::View, OpenKind::Edit] {
            assert_eq!(
                command(kind, path, false),
                Some(with("xdg-open", &[], path))
            );
            assert_eq!(command(kind, path, true), None);
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_views_with_explorer_and_edits_in_notepad() {
        let path = Path::new(r"C:\tmp\a & b.txt");
        assert_eq!(
            command(OpenKind::View, path, false),
            Some(with("explorer", &[], path))
        );
        for kind in [OpenKind::View, OpenKind::Edit] {
            assert_eq!(command(kind, path, true), Some(with("notepad", &[], path)));
        }
        assert_eq!(
            command(OpenKind::Edit, path, false),
            Some(with("notepad", &[], path))
        );
    }

    #[test]
    fn configured_programs_get_their_arguments_then_the_path() {
        let programs = configured(
            Some(ProgramLine::new("viewer", &["--x", "-y"])),
            Some(ProgramLine::new("editor", &["-n"])),
        );
        let path = Path::new("/tmp/a.txt");
        assert_eq!(
            open_command(&programs, OpenKind::View, path, false),
            Some(with("viewer", &["--x", "-y"], path))
        );
        assert_eq!(
            open_command(&programs, OpenKind::Edit, path, false),
            Some(with("editor", &["-n"], path))
        );
    }

    #[test]
    fn an_executable_goes_to_the_configured_editor_for_both_kinds() {
        let programs = configured(
            Some(ProgramLine::new("viewer", &[])),
            Some(ProgramLine::new("editor", &["-n"])),
        );
        let path = Path::new("/tmp/run.sh");
        for kind in [OpenKind::View, OpenKind::Edit] {
            assert_eq!(
                open_command(&programs, kind, path, true),
                Some(with("editor", &["-n"], path))
            );
        }
    }

    /// A configured viewer must never receive a file that could run.
    #[test]
    fn an_executable_never_goes_to_the_configured_viewer() {
        let programs = configured(Some(ProgramLine::new("viewer", &[])), None);
        let path = Path::new("/tmp/run.sh");
        let found = open_command(&programs, OpenKind::View, path, true);
        #[cfg(target_os = "macos")]
        assert_eq!(found, Some(with("open", &["-t"], path)));
        #[cfg(windows)]
        assert_eq!(found, Some(with("notepad", &[], path)));
        #[cfg(not(any(target_os = "macos", windows)))]
        assert_eq!(found, None);
    }

    #[test]
    fn edit_without_an_edit_program_falls_back_to_the_configured_viewer() {
        // Linux has no default editor; elsewhere the default editor applies.
        let programs = configured(Some(ProgramLine::new("viewer", &["--x"])), None);
        let path = Path::new("/tmp/a.txt");
        let found = open_command(&programs, OpenKind::Edit, path, false);
        #[cfg(not(any(target_os = "macos", windows)))]
        assert_eq!(found, Some(with("viewer", &["--x"], path)));
        #[cfg(target_os = "macos")]
        assert_eq!(found, Some(with("open", &["-t"], path)));
        #[cfg(windows)]
        assert_eq!(found, Some(with("notepad", &[], path)));
    }

    #[test]
    fn a_relative_path_is_anchored_for_a_configured_program() {
        let programs = configured(Some(ProgramLine::new("viewer", &["--x"])), None);
        let found = open_command(&programs, OpenKind::View, Path::new("-rf"), false).unwrap();
        assert_eq!(found.args, ["--x", "./-rf"]);
    }

    #[test]
    fn extensions_make_a_file_executable_whatever_the_case() {
        for name in ["/x/a.COMMAND", "/x/b.exe", "/x/c.Desktop", "/x/d.lnk"] {
            assert!(could_execute(Path::new(name)), "{name}");
        }
        assert!(!could_execute(Path::new("/x/nonexistent.txt")));
        assert!(!could_execute(Path::new("/x/noextension")));
    }

    #[cfg(unix)]
    #[test]
    fn the_exec_bit_makes_a_file_executable_even_without_an_extension() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("script");
        std::fs::write(&script, "#!/bin/sh\n").unwrap();
        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&script, &link).unwrap();
        assert!(!could_execute(&script));

        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert!(could_execute(&script));
        assert!(could_execute(&link), "the opener follows the link");
    }

    /// The path is the last argument and stays one piece, and a name that
    /// starts with `-` cannot be taken for an option.
    #[cfg(unix)]
    #[test]
    fn the_path_is_one_argument_and_never_an_option() {
        for kind in [OpenKind::View, OpenKind::Edit] {
            let args = command(kind, Path::new("-rf evil"), false).unwrap().args;
            let last = args.last().unwrap().to_string_lossy().into_owned();
            assert!(last.starts_with("./-rf evil"), "got {last:?}");
            let absolute = command(kind, Path::new("/tmp/-x"), false).unwrap().args;
            assert_eq!(absolute.last().unwrap(), "/tmp/-x");
        }
    }
}
