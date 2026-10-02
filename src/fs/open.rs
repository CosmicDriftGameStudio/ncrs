//! Handing a file to an external program, for F3 (view) and F4 (edit).
//!
//! Opening must never run anything. The system opener performs the default
//! action, and for a script, an app bundle or a shortcut that is "run it".
//! Such a file (`could_execute`) goes to a text editor instead, or is refused
//! where there is no safe editor. An extension list is never complete: on
//! Unix the exec bit catches the rest, Windows depends on the list alone.
//!
//! Starting is not blocking: the program is spawned and reaped on a thread of
//! its own, so the window stays usable while a viewer is open.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// What the user wants done with the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenKind {
    View,
    Edit,
}

/// Where F3 and F4 start a program. A trait so the end-to-end tests can hand
/// the app a launcher of their own: a test must never open a real window.
pub trait Launcher: Send + Sync {
    /// Starts the program for `path` and returns without waiting for it. The
    /// error is already text, for the status line.
    ///
    /// `executable` is `could_execute(path)`, passed in so the caller has
    /// already decided about refusing and the launcher stays free of the
    /// filesystem.
    fn launch(&self, kind: OpenKind, path: &Path, executable: bool) -> Result<(), String>;
}

/// The operating system's opener, through `std::process::Command`.
pub struct SystemLauncher;

impl Launcher for SystemLauncher {
    fn launch(&self, kind: OpenKind, path: &Path, executable: bool) -> Result<(), String> {
        let (program, args) = open_command(kind, path, executable)
            .ok_or_else(|| "refusing to open an executable".to_string())?;
        let mut child = Command::new(&program)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| format!("{}: {err}", program.to_string_lossy()))?;
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

/// The program and arguments that open `path`, or `None` when nothing safe
/// exists. Never a shell: the path is one argument of its own, so `&` or a
/// space in a name cannot become a command.
///
/// With `executable` set, macOS and Windows show the content in a text editor
/// for both kinds; Linux has no generic editor, so it opens nothing.
///
/// A relative path is anchored with `./`, so a name that starts with `-` is
/// not read as an option.
// ponytail: no $EDITOR/$VISUAL, the app is a GUI without a terminal and vim
// would start into nothing. T10 makes the programs configurable.
pub fn open_command(
    kind: OpenKind,
    path: &Path,
    executable: bool,
) -> Option<(OsString, Vec<OsString>)> {
    let anchored: PathBuf = if path.is_absolute() {
        path.to_path_buf()
    } else {
        Path::new(".").join(path)
    };
    let argument = anchored.into_os_string();

    #[cfg(target_os = "macos")]
    {
        let as_text = executable || kind == OpenKind::Edit;
        Some(if as_text {
            ("open".into(), vec!["-t".into(), argument])
        } else {
            ("open".into(), vec![argument])
        })
    }
    #[cfg(windows)]
    {
        Some(if executable || kind == OpenKind::Edit {
            ("notepad".into(), vec![argument])
        } else {
            ("explorer".into(), vec![argument])
        })
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = kind;
        (!executable).then(|| ("xdg-open".into(), vec![argument]))
    }
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

    fn command(kind: OpenKind, path: &Path, executable: bool) -> Option<(OsString, Vec<OsString>)> {
        open_command(kind, path, executable)
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_views_with_open_and_edits_in_the_text_editor() {
        let path = Path::new("/tmp/a & b.txt");
        assert_eq!(
            command(OpenKind::View, path, false),
            Some(("open".into(), vec![path.as_os_str().to_owned()]))
        );
        assert_eq!(
            command(OpenKind::Edit, path, false),
            Some((
                "open".into(),
                vec!["-t".into(), path.as_os_str().to_owned()]
            ))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_shows_an_executable_as_text_for_both_kinds() {
        let path = Path::new("/tmp/run.command");
        for kind in [OpenKind::View, OpenKind::Edit] {
            assert_eq!(
                command(kind, path, true),
                Some((
                    "open".into(),
                    vec!["-t".into(), path.as_os_str().to_owned()]
                ))
            );
        }
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    #[test]
    fn linux_opens_both_kinds_with_xdg_open_and_refuses_an_executable() {
        let path = Path::new("/tmp/a & b.txt");
        for kind in [OpenKind::View, OpenKind::Edit] {
            assert_eq!(
                command(kind, path, false),
                Some(("xdg-open".into(), vec![path.as_os_str().to_owned()]))
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
            Some(("explorer".into(), vec![path.as_os_str().to_owned()]))
        );
        for kind in [OpenKind::View, OpenKind::Edit] {
            assert_eq!(
                command(kind, path, true),
                Some(("notepad".into(), vec![path.as_os_str().to_owned()]))
            );
        }
        assert_eq!(
            command(OpenKind::Edit, path, false),
            Some(("notepad".into(), vec![path.as_os_str().to_owned()]))
        );
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
            let (_, args) = command(kind, Path::new("-rf evil"), false).unwrap();
            let last = args.last().unwrap().to_string_lossy().into_owned();
            assert!(last.starts_with("./-rf evil"), "got {last:?}");
            let (_, absolute) = command(kind, Path::new("/tmp/-x"), false).unwrap();
            assert_eq!(absolute.last().unwrap(), "/tmp/-x");
        }
    }
}
