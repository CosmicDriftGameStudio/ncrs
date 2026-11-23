use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use crate::action_error::{classify_sync_failure, ActionError};

/// The macOS App Store build does not run write actions for now.
// ponytail: sandbox may not start git; revisit after a sandbox test run
pub const WRITE_ACTIONS_ENABLED: bool = !cfg!(feature = "sandbox");

const PATH_COMMANDS: &[&str] = &["add", "reset", "checkout", "rm"];

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Whether a usable `git` was found on this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitCli {
    Available(PathBuf),
    NotFound,
}

static LOCATED: OnceLock<GitCli> = OnceLock::new();

impl GitCli {
    /// Looks `git` up once (PATH, then the usual install folders, which a
    /// Finder-launched app does not have on its PATH) and remembers the answer.
    pub fn locate() -> &'static Self {
        LOCATED.get_or_init(|| {
            let tools_installed = std::cell::OnceCell::new();
            candidates()
                .into_iter()
                .filter(|candidate| {
                    may_run(candidate, cfg!(target_os = "macos"), || {
                        *tools_installed.get_or_init(command_line_tools_installed)
                    })
                })
                .find(|candidate| runs(candidate))
                .map_or(Self::NotFound, Self::Available)
        })
    }

    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available(_))
    }
}

/// True when the view may offer write actions at all: not the sandbox build, and git is installed.
pub fn write_actions_available() -> bool {
    WRITE_ACTIONS_ENABLED && GitCli::locate().is_available()
}

fn executable_name() -> &'static str {
    if cfg!(windows) {
        "git.exe"
    } else {
        "git"
    }
}

fn candidates() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .map(|folder| folder.join(executable_name()))
                .collect()
        })
        .unwrap_or_default();
    let fixed: &[&str] = if cfg!(windows) {
        &[
            r"C:\Program Files\Git\cmd\git.exe",
            r"C:\Program Files (x86)\Git\cmd\git.exe",
        ]
    } else {
        &[
            "/opt/homebrew/bin/git",
            "/usr/local/bin/git",
            "/usr/bin/git",
        ]
    };
    found.extend(fixed.iter().map(PathBuf::from));
    found.retain(|candidate| candidate.is_absolute() && candidate.is_file());
    found
}

const MACOS_SHIM: &str = "/usr/bin/git";

/// Without the Xcode command line tools `/usr/bin/git` on a Mac is a stub that
/// opens an install dialog when it is run, even for `--version`.
fn is_macos_shim(candidate: &Path, on_macos: bool) -> bool {
    on_macos && candidate == Path::new(MACOS_SHIM)
}

/// Whether to start `candidate` to see if it is a git. The stub is never
/// started unless the command line tools stand behind it.
fn may_run(candidate: &Path, on_macos: bool, tools_installed: impl FnOnce() -> bool) -> bool {
    !is_macos_shim(candidate, on_macos) || tools_installed()
}

fn command_line_tools_installed() -> bool {
    Command::new("/usr/bin/xcode-select")
        .arg("-p")
        .stdin(Stdio::null())
        .output()
        .is_ok_and(|output| output.status.success())
}

fn runs(exe: &Path) -> bool {
    base_command(exe)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .is_ok_and(|output| output.status.success())
}

fn base_command(exe: &Path) -> Command {
    let mut command = Command::new(exe);
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .env("GIT_SEQUENCE_EDITOR", "true")
        .env("LC_ALL", "C");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(CREATE_NO_WINDOW)
    };
    command
}

/// What a finished `git` call left behind.
pub struct Output {
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl Output {
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).trim_end().to_string()
    }

    /// What a person (or a parser of git's messages) wants to read after a failure.
    pub fn combined(&self) -> String {
        let out = String::from_utf8_lossy(&self.stdout);
        let both = format!("{}\n{}", self.stderr.trim_end(), out.trim_end());
        redact_credentials(both.trim())
    }
}

/// `https://user:pass@host` becomes `https://***@host`: git echoes remote URLs
/// in its messages, and those end up on screen and in logs.
fn redact_credentials(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(position) = rest.find("://") {
        let (before, after) = rest.split_at(position + 3);
        result.push_str(before);
        let authority_end = after
            .find(|letter: char| letter == '/' || letter.is_whitespace())
            .unwrap_or(after.len());
        match after[..authority_end].rfind('@') {
            Some(at) => {
                result.push_str("***");
                rest = &after[at..];
            }
            None => rest = after,
        }
    }
    result.push_str(rest);
    result
}

/// One repository and the git that writes to it. Every call blocks.
#[derive(Clone, Debug)]
pub struct Git {
    exe: PathBuf,
    root: PathBuf,
    /// Filter drivers of the repository's own config to switch off; set only
    /// for a `Git` that reads (see `open_reading`).
    reading_without: Option<Vec<String>>,
}

#[cfg(windows)]
const NULL_DEVICE: &str = "NUL";
#[cfg(not(windows))]
const NULL_DEVICE: &str = "/dev/null";

impl Git {
    pub fn open(root: &Path) -> Result<Self, ActionError> {
        if !WRITE_ACTIONS_ENABLED {
            return Err(ActionError::DisabledInSandbox);
        }
        match GitCli::locate() {
            GitCli::Available(exe) => Ok(Self {
                exe: exe.clone(),
                root: root.to_path_buf(),
                reading_without: None,
            }),
            GitCli::NotFound => Err(ActionError::GitNotFound),
        }
    }

    /// Like `open`, for calls that only read: no hooks, no optional index
    /// refresh and none of the filter commands the repository's own config
    /// names, so showing a folder never runs what a downloaded `.git/config` says.
    pub fn open_reading(root: &Path) -> Result<Self, ActionError> {
        let mut git = Self::open(root)?;
        git.reading_without = Some(crate::repo_access::repository_filter_names(root));
        Ok(git)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn run(&self, args: &[&str], stdin: Option<&[u8]>) -> Result<Output, ActionError> {
        let mut command = base_command(&self.exe);
        command
            .arg("-C")
            .arg(&self.root)
            // A repository from a download must not make every call start a watcher it names.
            .args(["-c", "core.fsmonitor=false"]);
        if let Some(filters) = &self.reading_without {
            command
                .env("GIT_OPTIONAL_LOCKS", "0")
                .args(["-c", &format!("core.hooksPath={NULL_DEVICE}")]);
            for name in filters {
                for key in ["clean", "smudge", "process"] {
                    command.args(["-c", &format!("filter.{name}.{key}=")]);
                }
            }
        }
        // File names from the view are literal, never patterns. Not for every
        // command: `stash --include-untracked` builds pathspecs itself and
        // silently leaves the untracked files in place under this flag.
        if args
            .first()
            .is_some_and(|name| PATH_COMMANDS.contains(name))
        {
            command.arg("--literal-pathspecs");
        }
        command
            .args(args)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|cause| ActionError::Io(cause.to_string()))?;
        if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
            pipe.write_all(bytes)
                .map_err(|cause| ActionError::Io(cause.to_string()))?;
        }
        let output = child
            .wait_with_output()
            .map_err(|cause| ActionError::Io(cause.to_string()))?;
        Ok(Output {
            success: output.status.success(),
            stdout: output.stdout,
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }

    /// Runs and turns a failure into `ActionError::Other`.
    pub(crate) fn ok(&self, args: &[&str]) -> Result<Output, ActionError> {
        self.ok_with_input(args, None)
    }

    pub(crate) fn ok_with_input(
        &self,
        args: &[&str],
        stdin: Option<&[u8]>,
    ) -> Result<Output, ActionError> {
        let output = self.run(args, stdin)?;
        if output.success {
            Ok(output)
        } else {
            Err(ActionError::Other {
                git_output: output.combined(),
            })
        }
    }

    pub(crate) fn text(&self, args: &[&str]) -> Result<String, ActionError> {
        Ok(self.ok(args)?.stdout_text())
    }

    /// Runs a call that talks to a remote and sorts its failure by cause.
    pub(crate) fn talk(&self, args: &[&str], remote: &str) -> Result<Output, ActionError> {
        let output = self.run(args, None)?;
        if output.success {
            Ok(output)
        } else {
            Err(classify_sync_failure(&output.combined(), remote))
        }
    }

    /// `None` when the command fails (a missing ref, say).
    pub(crate) fn try_text(&self, args: &[&str]) -> Option<String> {
        self.run(args, None)
            .ok()
            .filter(|output| output.success)
            .map(|output| output.stdout_text())
    }

    pub(crate) fn git_dir(&self) -> Result<PathBuf, ActionError> {
        Ok(PathBuf::from(
            self.text(&["rev-parse", "--absolute-git-dir"])?,
        ))
    }

    pub fn current_branch(&self) -> Option<String> {
        self.try_text(&["symbolic-ref", "--short", "-q", "HEAD"])
            .filter(|name| !name.is_empty())
    }

    pub(crate) fn head_sha(&self) -> Option<String> {
        self.try_text(&["rev-parse", "--verify", "-q", "HEAD"])
            .filter(|sha| !sha.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_in_urls_are_redacted_from_git_messages() {
        assert_eq!(
            redact_credentials("fatal: unable to access 'https://user:pw@host.example/r.git/': x"),
            "fatal: unable to access 'https://***@host.example/r.git/': x"
        );
        assert_eq!(
            redact_credentials("ssh://git@host/r and https://a:b@c/d"),
            "ssh://***@host/r and https://***@c/d"
        );
        assert_eq!(
            redact_credentials("see https://host/path@x"),
            "see https://host/path@x"
        );
    }

    #[test]
    fn the_mac_stub_is_not_started_without_the_command_line_tools() {
        let stub = Path::new("/usr/bin/git");
        assert!(!may_run(stub, true, || false));
        assert!(may_run(stub, true, || true));
    }

    #[test]
    fn other_gits_and_other_systems_are_started_without_asking_for_the_tools() {
        let asked = std::cell::Cell::new(false);
        let ask = || {
            asked.set(true);
            false
        };
        assert!(may_run(Path::new("/opt/homebrew/bin/git"), true, ask));
        assert!(may_run(Path::new("/usr/bin/git"), false, ask));
        assert!(!asked.get());
    }
}
