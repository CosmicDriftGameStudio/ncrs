//! "Open terminal here": the terminals ncrs knows, how to find them, and how
//! each is told the folder to start in.
//!
//! Every terminal is a [`ProgramLine`] with `{path}` where the folder goes, so
//! the one the user picks in the settings is simply saved as that line. The
//! folder is always one argument of its own and the working directory of the
//! process; nothing goes through a shell.
//!
//! On macOS everything starts through `/usr/bin/open`, the one way the App
//! Sandbox lets ncrs start another app.

use std::ffi::{OsStr, OsString};
use std::path::Path;

use crate::open::OpenCommand;
use crate::open_programs::ProgramLine;

const PATH_PLACEHOLDER: &str = "{path}";
const OPEN_PROGRAM: &str = "/usr/bin/open";

/// A terminal found on this machine, with the line that starts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownTerminal {
    pub name: String,
    pub line: ProgramLine,
}

/// There is no terminal to start: none is set and none was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoTerminal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Os {
    Mac,
    Windows,
    Linux,
}

impl Os {
    const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// What tells that a terminal is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Probe {
    /// macOS: the app with this bundle id, found through Launch Services.
    Bundle(&'static str),
    /// A program on `PATH`.
    Program(&'static str),
    /// The program an environment variable names.
    EnvProgram(&'static str),
}

/// How the folder reaches the terminal. What the probe found is the app path
/// (`Bundle`) or the program (`Program`, `EnvProgram`).
#[derive(Debug, Clone, Copy)]
enum Launch {
    /// The found program, then these arguments.
    Program(&'static [&'static str]),
    /// `open -a <app>` then these arguments: the running app opens a window.
    App(&'static [&'static str]),
    /// `open -na <app> --args` then these arguments: the app takes them as its
    /// own command line, which needs a new instance.
    NewInstance(&'static [&'static str]),
}

struct Spec {
    name: &'static str,
    probe: Probe,
    launch: Launch,
}

const fn spec(name: &'static str, probe: Probe, launch: Launch) -> Spec {
    Spec {
        name,
        probe,
        launch,
    }
}

/// The one table of known terminals, in the order "Automatic" prefers them.
fn known_terminals(os: Os) -> &'static [Spec] {
    use Launch::{App, NewInstance, Program};
    use Probe::{Bundle, EnvProgram, Program as OnPath};
    const MAC: &[Spec] = &[
        spec(
            "iTerm2",
            Bundle("com.googlecode.iterm2"),
            App(&[PATH_PLACEHOLDER]),
        ),
        spec(
            "Ghostty",
            Bundle("com.mitchellh.ghostty"),
            App(&[PATH_PLACEHOLDER]),
        ),
        spec(
            "WezTerm",
            Bundle("com.github.wez.wezterm"),
            NewInstance(&["start", "--cwd", PATH_PLACEHOLDER]),
        ),
        spec(
            "kitty",
            Bundle("net.kovidgoyal.kitty"),
            NewInstance(&["--directory", PATH_PLACEHOLDER]),
        ),
        spec(
            "Alacritty",
            Bundle("org.alacritty"),
            NewInstance(&["--working-directory", PATH_PLACEHOLDER]),
        ),
        spec(
            "Warp",
            Bundle("dev.warp.Warp-Stable"),
            App(&[PATH_PLACEHOLDER]),
        ),
        spec(
            "Terminal",
            Bundle("com.apple.Terminal"),
            App(&[PATH_PLACEHOLDER]),
        ),
    ];
    const WINDOWS: &[Spec] = &[
        spec(
            "Windows Terminal",
            OnPath("wt.exe"),
            Program(&["-d", PATH_PLACEHOLDER]),
        ),
        spec(
            "PowerShell 7",
            OnPath("pwsh.exe"),
            Program(&["-NoExit", "-WorkingDirectory", PATH_PLACEHOLDER]),
        ),
        spec(
            "Windows PowerShell",
            OnPath("powershell.exe"),
            Program(&["-NoExit"]),
        ),
    ];
    const LINUX: &[Spec] = &[
        spec("$TERMINAL", EnvProgram("TERMINAL"), Program(&[])),
        spec(
            "x-terminal-emulator",
            OnPath("x-terminal-emulator"),
            Program(&[]),
        ),
        spec(
            "GNOME Terminal",
            OnPath("gnome-terminal"),
            Program(&["--working-directory={path}"]),
        ),
        spec(
            "Konsole",
            OnPath("konsole"),
            Program(&["--workdir", PATH_PLACEHOLDER]),
        ),
        spec(
            "Xfce Terminal",
            OnPath("xfce4-terminal"),
            Program(&["--working-directory={path}"]),
        ),
        spec(
            "kitty",
            OnPath("kitty"),
            Program(&["--directory", PATH_PLACEHOLDER]),
        ),
        spec(
            "Alacritty",
            OnPath("alacritty"),
            Program(&["--working-directory", PATH_PLACEHOLDER]),
        ),
        spec(
            "WezTerm",
            OnPath("wezterm"),
            Program(&["start", "--cwd", PATH_PLACEHOLDER]),
        ),
        spec(
            "foot",
            OnPath("foot"),
            Program(&["--working-directory={path}"]),
        ),
    ];
    match os {
        Os::Mac => MAC,
        Os::Windows => WINDOWS,
        Os::Linux => LINUX,
    }
}

impl Spec {
    /// The line that starts this terminal, given what the probe found.
    fn line(&self, found: &str) -> ProgramLine {
        match self.launch {
            Launch::Program(args) => ProgramLine::new(found, args),
            Launch::App(args) => {
                let words: Vec<&str> = ["-a", found]
                    .into_iter()
                    .chain(args.iter().copied())
                    .collect();
                ProgramLine::new(OPEN_PROGRAM, &words)
            }
            Launch::NewInstance(args) => {
                let words: Vec<&str> = ["-na", found, "--args"]
                    .into_iter()
                    .chain(args.iter().copied())
                    .collect();
                ProgramLine::new(OPEN_PROGRAM, &words)
            }
        }
    }
}

/// The terminals installed on this machine, in the order "Automatic" prefers
/// them.
pub fn detect_terminals() -> Vec<KnownTerminal> {
    detect_with(Os::current(), &find_on_this_machine)
}

fn detect_with(os: Os, find: &dyn Fn(Probe) -> Option<String>) -> Vec<KnownTerminal> {
    known_terminals(os)
        .iter()
        .filter_map(|terminal| {
            let found = find(terminal.probe)?;
            let name = match terminal.probe {
                Probe::EnvProgram(_) => file_name_of(&found),
                Probe::Bundle(_) | Probe::Program(_) => terminal.name.to_owned(),
            };
            Some(KnownTerminal {
                name,
                line: terminal.line(&found),
            })
        })
        .collect()
}

fn file_name_of(program: &str) -> String {
    Path::new(program).file_name().map_or_else(
        || program.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The command that opens a terminal in `folder`: the saved `chosen` line, else
/// the first of `detected`. `{path}` in an argument is replaced by the folder;
/// without one the folder is not appended, the program starts in it.
pub fn terminal_command(
    chosen: Option<&ProgramLine>,
    detected: &[KnownTerminal],
    folder: &Path,
) -> Result<OpenCommand, NoTerminal> {
    let line = chosen
        .or_else(|| detected.first().map(|terminal| &terminal.line))
        .ok_or(NoTerminal)?;
    Ok(OpenCommand {
        program: line.program().into(),
        args: line
            .args()
            .iter()
            .map(|template| with_folder(template, folder.as_os_str()))
            .collect(),
        current_dir: Some(folder.to_owned()),
        new_console: cfg!(windows),
    })
}

/// Built piece by piece as an `OsString`: a folder need not be valid UTF-8.
fn with_folder(template: &str, folder: &OsStr) -> OsString {
    let mut parts = template.split(PATH_PLACEHOLDER);
    let mut filled = OsString::from(parts.next().unwrap_or_default());
    for part in parts {
        filled.push(folder);
        filled.push(part);
    }
    filled
}

fn find_on_this_machine(probe: Probe) -> Option<String> {
    match probe {
        Probe::Bundle(bundle_id) => app_with_bundle_id(bundle_id),
        Probe::Program(name) => is_on_path(name).then(|| name.to_owned()),
        Probe::EnvProgram(variable) => {
            let program = std::env::var(variable)
                .ok()
                .filter(|value| !value.is_empty())?;
            let exists = if program.contains(crate::open_programs::is_separator) {
                is_program(Path::new(&program))
            } else {
                is_on_path(&program)
            };
            exists.then_some(program)
        }
    }
}

#[cfg(target_os = "macos")]
fn app_with_bundle_id(bundle_id: &str) -> Option<String> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::NSString;
    let url = NSWorkspace::sharedWorkspace()
        .URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))?;
    url.path().map(|path| path.to_string())
}

#[cfg(not(target_os = "macos"))]
fn app_with_bundle_id(_bundle_id: &str) -> Option<String> {
    None
}

fn is_on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(name))
            .any(|candidate| is_program(&candidate))
    })
}

#[cfg(unix)]
fn is_program(candidate: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(candidate)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// Not `metadata`: the `wt.exe` of the Store is an alias that cannot be
/// followed, only listed.
#[cfg(not(unix))]
fn is_program(candidate: &Path) -> bool {
    std::fs::symlink_metadata(candidate).is_ok()
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

    fn installed(present: &'static [&'static str]) -> impl Fn(Probe) -> Option<String> {
        move |probe| match probe {
            Probe::Bundle(id) => present
                .contains(&id)
                .then(|| format!("/Applications/{id}.app")),
            Probe::Program(name) => present.contains(&name).then(|| name.to_owned()),
            Probe::EnvProgram(variable) => present
                .contains(&variable)
                .then(|| "/usr/local/bin/mine".to_owned()),
        }
    }

    fn words(line: &ProgramLine) -> Vec<&str> {
        line.words().collect()
    }

    fn started(chosen: Option<&ProgramLine>, folder: &str) -> OpenCommand {
        terminal_command(chosen, &[], Path::new(folder)).unwrap()
    }

    fn line_of(os: Os, name: &str) -> ProgramLine {
        let terminal = known_terminals(os)
            .iter()
            .find(|terminal| terminal.name == name)
            .unwrap();
        let found = match terminal.probe {
            Probe::Bundle(id) => format!("/Applications/{id}.app"),
            Probe::Program(program) => program.to_owned(),
            Probe::EnvProgram(_) => "mine".to_owned(),
        };
        terminal.line(&found)
    }

    fn args_of(command: &OpenCommand) -> Vec<&str> {
        command
            .args
            .iter()
            .map(|arg| arg.to_str().unwrap())
            .collect()
    }

    #[test]
    fn macos_starts_every_terminal_through_open_with_the_folder_as_its_own_argument() {
        let folder = "/Users/me/My Projects";
        let expected: [(&str, &str, &[&str]); 7] = [
            ("iTerm2", "/Applications/com.googlecode.iterm2.app", &[]),
            ("Ghostty", "/Applications/com.mitchellh.ghostty.app", &[]),
            ("Warp", "/Applications/dev.warp.Warp-Stable.app", &[]),
            ("Terminal", "/Applications/com.apple.Terminal.app", &[]),
            (
                "kitty",
                "/Applications/net.kovidgoyal.kitty.app",
                &["--directory"],
            ),
            (
                "Alacritty",
                "/Applications/org.alacritty.app",
                &["--working-directory"],
            ),
            (
                "WezTerm",
                "/Applications/com.github.wez.wezterm.app",
                &["start", "--cwd"],
            ),
        ];
        for (name, app, flags) in expected {
            let command = started(Some(&line_of(Os::Mac, name)), folder);
            let mut want = if flags.is_empty() {
                vec!["-a", app]
            } else {
                vec!["-na", app, "--args"]
            };
            want.extend(flags);
            want.push(folder);
            assert_eq!(command.program, OsString::from("/usr/bin/open"), "{name}");
            assert_eq!(args_of(&command), want, "{name}");
            assert_eq!(command.current_dir.as_deref(), Some(Path::new(folder)));
        }
    }

    #[test]
    fn windows_terminal_gets_d_and_powershell_stays_open_in_the_folder() {
        let folder = r"C:\Users\me\My Projects";
        let wt = started(Some(&line_of(Os::Windows, "Windows Terminal")), folder);
        assert_eq!(wt.program, OsString::from("wt.exe"));
        assert_eq!(args_of(&wt), ["-d", folder]);
        let pwsh = started(Some(&line_of(Os::Windows, "PowerShell 7")), folder);
        assert_eq!(args_of(&pwsh), ["-NoExit", "-WorkingDirectory", folder]);
        let legacy = started(Some(&line_of(Os::Windows, "Windows PowerShell")), folder);
        assert_eq!(args_of(&legacy), ["-NoExit"]);
        assert_eq!(legacy.current_dir.as_deref(), Some(Path::new(folder)));
    }

    #[test]
    fn linux_terminals_get_their_own_working_directory_argument() {
        let folder = "/home/me/My Projects";
        let expected: [(&str, Vec<String>); 9] = [
            (
                "GNOME Terminal",
                vec![format!("--working-directory={folder}")],
            ),
            ("Konsole", vec!["--workdir".into(), folder.into()]),
            (
                "Xfce Terminal",
                vec![format!("--working-directory={folder}")],
            ),
            ("kitty", vec!["--directory".into(), folder.into()]),
            (
                "Alacritty",
                vec!["--working-directory".into(), folder.into()],
            ),
            (
                "WezTerm",
                vec!["start".into(), "--cwd".into(), folder.into()],
            ),
            ("foot", vec![format!("--working-directory={folder}")]),
            ("x-terminal-emulator", vec![]),
            ("$TERMINAL", vec![]),
        ];
        for (name, args) in expected {
            let command = started(Some(&line_of(Os::Linux, name)), folder);
            assert_eq!(args_of(&command), args, "{name}");
            assert_eq!(command.current_dir.as_deref(), Some(Path::new(folder)));
        }
    }

    #[test]
    fn detection_lists_the_installed_terminals_in_the_order_of_the_table() {
        let found = detect_with(
            Os::Mac,
            &installed(&[
                "com.apple.Terminal",
                "org.alacritty",
                "com.googlecode.iterm2",
            ]),
        );
        let names: Vec<&str> = found
            .iter()
            .map(|terminal| terminal.name.as_str())
            .collect();
        assert_eq!(names, ["iTerm2", "Alacritty", "Terminal"]);

        let found = detect_with(Os::Windows, &installed(&["powershell.exe", "pwsh.exe"]));
        let names: Vec<&str> = found
            .iter()
            .map(|terminal| terminal.name.as_str())
            .collect();
        assert_eq!(names, ["PowerShell 7", "Windows PowerShell"]);

        let found = detect_with(Os::Linux, &installed(&["foot", "konsole", "TERMINAL"]));
        let names: Vec<&str> = found
            .iter()
            .map(|terminal| terminal.name.as_str())
            .collect();
        assert_eq!(names, ["mine", "Konsole", "foot"]);
        assert_eq!(words(&found[0].line), ["/usr/local/bin/mine"]);
        assert!(detect_with(Os::Linux, &installed(&[])).is_empty());
    }

    #[test]
    fn automatic_takes_the_first_detected_and_a_saved_line_wins() {
        let folder = Path::new("/tmp/work");
        let detected = detect_with(Os::Linux, &installed(&["konsole", "foot"]));
        let automatic = terminal_command(None, &detected, folder).unwrap();
        assert_eq!(automatic.program, OsString::from("konsole"));

        let saved = ProgramLine::new("foot", &["--working-directory={path}"]);
        let chosen = terminal_command(Some(&saved), &detected, folder).unwrap();
        assert_eq!(chosen.program, OsString::from("foot"));
        assert_eq!(args_of(&chosen), ["--working-directory=/tmp/work"]);

        assert_eq!(terminal_command(None, &[], folder), Err(NoTerminal));
    }

    #[test]
    fn a_custom_line_gets_the_folder_where_path_stands_and_never_appended() {
        let folder = Path::new("/tmp/a b");
        let with_placeholder = ProgramLine::new("myterm", &["--cd", "{path}", "--tab"]);
        let command = terminal_command(Some(&with_placeholder), &[], folder).unwrap();
        assert_eq!(args_of(&command), ["--cd", "/tmp/a b", "--tab"]);
        assert_eq!(command.current_dir.as_deref(), Some(folder));

        let without = ProgramLine::new("myterm", &["--tab"]);
        let command = terminal_command(Some(&without), &[], folder).unwrap();
        assert_eq!(args_of(&command), ["--tab"]);
        assert_eq!(command.current_dir.as_deref(), Some(folder));
    }

    #[test]
    fn a_program_found_on_the_path_is_detected_and_a_missing_one_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let name = if cfg!(windows) {
            "ncrs-fake-term.exe"
        } else {
            "ncrs-fake-term"
        };
        let file = dir.path().join(name);
        std::fs::write(&file, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert!(is_program(&file));
        assert!(!is_program(&dir.path().join("absent")));
    }

    #[test]
    fn the_known_terminals_have_a_folder_argument_where_the_terminal_takes_one() {
        for os in [Os::Mac, Os::Windows, Os::Linux] {
            for terminal in known_terminals(os) {
                let line = terminal.line("found");
                let in_args = line.args().iter().any(|arg| arg.contains(PATH_PLACEHOLDER));
                let by_working_directory_only = matches!(
                    terminal.name,
                    "$TERMINAL" | "x-terminal-emulator" | "Windows PowerShell"
                );
                assert_eq!(in_args, !by_working_directory_only, "{}", terminal.name);
            }
        }
    }

    #[test]
    fn the_folder_is_filled_in_inside_an_argument_and_in_every_place() {
        assert_eq!(
            with_folder("--cd={path}:{path}", OsStr::new("/tmp/x")),
            OsString::from("--cd=/tmp/x:/tmp/x")
        );
    }
}
