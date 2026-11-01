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

use crate::open_programs::{OpenPrograms, ProgramLine};

/// What the user wants done with the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OpenKind {
    View,
    Edit,
}

/// Whether this is the macOS App Store build, which runs in the App Sandbox
/// and may start another program only as an app (`open -a`). Only whether
/// the variable is set is looked at.
pub fn runs_in_app_sandbox() -> bool {
    cfg!(target_os = "macos") && std::env::var_os("APP_SANDBOX_CONTAINER_ID").is_some()
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

/// The Full Disk Access page of System Settings, which is what lifts the
/// privacy block on network volumes, iCloud Drive and cloud storage folders.
/// A fixed program and URL: nothing the user typed reaches the command.
#[cfg(target_os = "macos")]
pub fn privacy_settings_command() -> OpenCommand {
    OpenCommand {
        program: "/usr/bin/open".into(),
        args: vec![
            "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles".into(),
        ],
    }
}

/// Shows the trash folder in the Finder. The sandbox may only start apps
/// through `open -a`, and `home_dir` is the real home there. A fixed program:
/// nothing the user typed reaches the command.
#[cfg(target_os = "macos")]
pub fn trash_in_finder_command() -> OpenCommand {
    OpenCommand {
        program: "/usr/bin/open".into(),
        args: vec![
            "-a".into(),
            "Finder".into(),
            super::home_dir().join(".Trash").into(),
        ],
    }
}

/// The system's default opener on `target`, a file or an address, as one
/// argument of its own. Never a shell.
pub fn system_open_command(target: impl Into<OsString>) -> OpenCommand {
    let (opener, _) = default_programs();
    let mut args: Vec<OsString> = opener.args().iter().map(OsString::from).collect();
    args.push(target.into());
    OpenCommand {
        program: opener.program().into(),
        args,
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
    // Windows: shortcuts, help and installer files, packages
    "appref-ms",
    "application",
    "settingcontent-ms",
    "library-ms",
    "search-ms",
    "scf",
    "chm",
    "hlp",
    "inf",
    "msp",
    "mst",
    "psm1",
    "wsc",
    "sct",
    "ws",
    "gadget",
    "diagcab",
    "appx",
    "msix",
    "appxbundle",
    "msixbundle",
    "appinstaller",
    // macOS: location files, profiles, installers, plug-ins
    "fileloc",
    "inetloc",
    "webloc",
    "mobileconfig",
    "pkg",
    "mpkg",
    "prefpane",
    "saver",
    "shortcut",
    // Linux: packages
    "deb",
    "rpm",
    "flatpakref",
    "snap",
];

/// Whether the system opener might run `path` instead of showing it: an
/// executable file (the link is followed, as the opener follows it) or an
/// extension from `EXECUTABLE_EXTENSIONS`.
pub fn could_execute(path: &Path) -> bool {
    listed_extension(path) || has_exec_bit(path)
}

/// The extension is read after trailing dots and spaces are cut off, as
/// Windows does when it opens the file: `evil.exe.` is `evil.exe`.
fn listed_extension(path: &Path) -> bool {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let trimmed = name.trim_end_matches(['.', ' ']);
    Path::new(trimmed).extension().is_some_and(|raw| {
        let lowered = raw.to_string_lossy().to_ascii_lowercase();
        EXECUTABLE_EXTENSIONS.contains(&lowered.as_str())
    })
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
///
/// An argument of the chosen program may contain `{path}` and `{line}`, which
/// become the (anchored) path and the line to jump to, `1` without one. A
/// program with no `{path}` in any argument gets the path appended, as before;
/// one without `{line}` simply opens the file.
// ponytail: no $EDITOR/$VISUAL, the app is a GUI without a terminal and vim
// would start into nothing.
pub fn open_command(
    programs: &OpenPrograms,
    kind: OpenKind,
    path: &Path,
    executable: bool,
    line: Option<u64>,
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
    let line_number = line.unwrap_or(1);
    let mut args: Vec<OsString> = chosen
        .args()
        .iter()
        .map(|template| substitute(template, anchored.as_os_str(), line_number))
        .collect();
    if !chosen
        .args()
        .iter()
        .any(|arg| arg.contains(PATH_PLACEHOLDER))
    {
        args.push(anchored.into_os_string());
    }
    Some(OpenCommand {
        program: chosen.program().into(),
        args,
    })
}

const PATH_PLACEHOLDER: &str = "{path}";

/// `template` with `{path}` and `{line}` filled in one pass, so a path that
/// itself contains `{line}` stays as it is. Built piece by piece as an
/// `OsString`: a path need not be valid UTF-8.
fn substitute(template: &str, path: &std::ffi::OsStr, line: u64) -> OsString {
    let mut filled = OsString::new();
    let mut rest = template;
    while let Some((before, after_brace)) = rest.split_once('{') {
        filled.push(before);
        if let Some(tail) = after_brace.strip_prefix("path}") {
            filled.push(path);
            rest = tail;
        } else if let Some(tail) = after_brace.strip_prefix("line}") {
            filled.push(line.to_string());
            rest = tail;
        } else {
            filled.push("{");
            rest = after_brace;
        }
    }
    filled.push(rest);
    filled
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
        open_command(&OpenPrograms::default(), kind, path, executable, None)
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
    fn the_trash_opens_in_the_finder_through_open_dash_a() {
        let trash = crate::home_dir().join(".Trash");
        assert_eq!(
            trash_in_finder_command(),
            with("/usr/bin/open", &["-a", "Finder"], &trash)
        );
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
            open_command(&programs, OpenKind::View, path, false, None),
            Some(with("viewer", &["--x", "-y"], path))
        );
        assert_eq!(
            open_command(&programs, OpenKind::Edit, path, false, None),
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
                open_command(&programs, kind, path, true, None),
                Some(with("editor", &["-n"], path))
            );
        }
    }

    /// A configured viewer must never receive a file that could run.
    #[test]
    fn an_executable_never_goes_to_the_configured_viewer() {
        let programs = configured(Some(ProgramLine::new("viewer", &[])), None);
        let path = Path::new("/tmp/run.sh");
        let found = open_command(&programs, OpenKind::View, path, true, None);
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
        let found = open_command(&programs, OpenKind::Edit, path, false, None);
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
        let found = open_command(&programs, OpenKind::View, Path::new("-rf"), false, None).unwrap();
        let anchored = Path::new(".").join("-rf").into_os_string();
        assert_eq!(found.args, [OsString::from("--x"), anchored]);
    }

    fn code_goto() -> OpenPrograms {
        configured(
            Some(ProgramLine::new("code", &["--goto", "{path}:{line}"])),
            Some(ProgramLine::new("code", &["--goto", "{path}:{line}"])),
        )
    }

    #[test]
    fn the_line_and_path_placeholders_are_filled_in() {
        let found = open_command(
            &code_goto(),
            OpenKind::Edit,
            Path::new("/a/b.rs"),
            false,
            Some(26),
        )
        .unwrap();
        assert_eq!(found.program, OsString::from("code"));
        assert_eq!(
            found.args,
            [OsString::from("--goto"), OsString::from("/a/b.rs:26")]
        );
    }

    #[test]
    fn without_a_line_the_placeholder_is_one() {
        let found = open_command(
            &code_goto(),
            OpenKind::View,
            Path::new("/a/b.rs"),
            false,
            None,
        )
        .unwrap();
        assert_eq!(
            found.args,
            [OsString::from("--goto"), OsString::from("/a/b.rs:1")]
        );
    }

    #[test]
    fn a_program_without_the_path_placeholder_gets_the_path_appended() {
        let programs = configured(Some(ProgramLine::new("viewer", &["+{line}"])), None);
        let path = Path::new("/a/b.rs");
        let found = open_command(&programs, OpenKind::View, path, false, Some(7)).unwrap();
        assert_eq!(
            found.args,
            [OsString::from("+7"), path.as_os_str().to_owned()]
        );
    }

    #[test]
    fn a_program_without_the_line_placeholder_just_opens_the_file() {
        let programs = configured(Some(ProgramLine::new("viewer", &["{path}"])), None);
        let found = open_command(
            &programs,
            OpenKind::View,
            Path::new("/a/b.rs"),
            false,
            Some(7),
        )
        .unwrap();
        assert_eq!(found.args, [OsString::from("/a/b.rs")]);
    }

    /// Substitution is a single pass: a file named like a placeholder is not
    /// filled in again.
    #[test]
    fn a_placeholder_in_the_file_name_stays_literal() {
        let found = open_command(
            &code_goto(),
            OpenKind::Edit,
            Path::new("/a/{line}{path}.txt"),
            false,
            Some(3),
        )
        .unwrap();
        assert_eq!(
            found.args,
            [
                OsString::from("--goto"),
                OsString::from("/a/{line}{path}.txt:3")
            ]
        );
    }

    #[test]
    fn a_relative_path_is_anchored_inside_the_placeholder() {
        let found = open_command(
            &code_goto(),
            OpenKind::Edit,
            Path::new("-rf"),
            false,
            Some(2),
        )
        .unwrap();
        let anchored = Path::new(".").join("-rf");
        assert_eq!(
            found.args,
            [
                OsString::from("--goto"),
                OsString::from(format!("{}:2", anchored.display()))
            ]
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

    #[test]
    fn trailing_dots_and_spaces_do_not_hide_the_extension() {
        for name in ["evil.exe.", "evil.exe ", "run.SH..", "x.bat . ."] {
            assert!(could_execute(Path::new(name)), "{name:?}");
        }
        for name in ["a.txt", "a.txt.", "readme", "...", " "] {
            assert!(!could_execute(Path::new(name)), "{name:?}");
        }
    }

    #[test]
    fn launcher_file_types_of_every_system_count_as_executable() {
        for name in [
            "a.appref-ms",
            "b.settingcontent-ms",
            "c.chm",
            "d.msix",
            "e.webloc",
            "f.INETLOC",
            "g.pkg",
            "h.mobileconfig",
            "i.deb",
            "j.rpm",
            "k.flatpakref",
            "l.snap",
        ] {
            assert!(could_execute(Path::new(name)), "{name}");
        }
        for name in ["a.iso", "b.img", "c.vhd", "d.dmg"] {
            assert!(!could_execute(Path::new(name)), "{name}");
        }
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
