use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

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

fn finish(mut child: Child, timeout: Option<Duration>) -> Result<Output, ActionError> {
    let io_error = |cause: std::io::Error| ActionError::Io(cause.to_string());
    let Some(limit) = timeout else {
        let output = child.wait_with_output().map_err(io_error)?;
        return Ok(Output {
            success: output.status.success(),
            stdout: output.stdout,
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    };
    let stdout = reader_of(child.stdout.take());
    let stderr = reader_of(child.stderr.take());
    let deadline = Instant::now() + limit;
    let mut timed_out = false;
    let success = loop {
        match child.try_wait().map_err(io_error)? {
            Some(status) => break status.success(),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                timed_out = true;
                break false;
            }
            None => std::thread::sleep(POLL_INTERVAL),
        }
    };
    let mut stderr_text =
        String::from_utf8_lossy(&stderr.recv_timeout(PIPE_GRACE).unwrap_or_default()).to_string();
    if timed_out {
        stderr_text.push_str(&format!(
            "\nOperation timed out after {} s",
            limit.as_secs()
        ));
    }
    Ok(Output {
        success,
        stdout: stdout.recv_timeout(PIPE_GRACE).unwrap_or_default(),
        stderr: stderr_text,
    })
}

fn reader_of(pipe: Option<impl std::io::Read + Send + 'static>) -> mpsc::Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut open) = pipe {
            let _ = open.read_to_end(&mut bytes);
        }
        let _ = sender.send(bytes);
    });
    receiver
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

/// How a call that needs a login (or a host-key answer) gets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prompts {
    /// Never asks: git fails with an authentication error.
    Plain,
    /// Asks through the askpass broker; the request carries this job number.
    Ask { job: u64 },
    /// Runs without a person: no prompt of any kind, ssh in batch mode, no
    /// hooks. For fetching in the background and in bulk runs.
    Unattended,
}

pub(crate) fn repository_runs_commands() -> ActionError {
    ActionError::Other {
        git_output: "the repository config runs commands".to_string(),
    }
}

/// The only transports an unattended call may use: no `ext::` command transport.
const UNATTENDED_PROTOCOLS: &str = "https:http:ssh:git:file";
/// A push into a local folder runs the hooks found there, whatever this call sets.
const UNATTENDED_PUSH_PROTOCOLS: &str = "https:http:ssh:git";
/// `-c` settings of an unattended call: no command transport, no background
/// maintenance that a fetch would start (it runs the repository's own gc hooks),
/// and nothing in submodules: their config is not checked for commands.
const UNATTENDED_SETTINGS: [&str; 6] = [
    "protocol.ext.allow=never",
    "maintenance.auto=false",
    "gc.auto=0",
    "submodule.recurse=false",
    "fetch.recurseSubmodules=false",
    "push.recurseSubmodules=no",
];
/// How long an unattended call may take before its git is killed.
const UNATTENDED_TIMEOUT: Duration = Duration::from_secs(300);
/// What a killed git's pipes may still deliver; a child of git (ssh) can hold them open.
const PIPE_GRACE: Duration = Duration::from_secs(1);
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// One repository and the git that writes to it. Every call blocks.
#[derive(Clone, Debug)]
pub struct Git {
    exe: PathBuf,
    root: PathBuf,
    /// Filter drivers of the repository's own config to switch off; set only
    /// for a `Git` that reads (see `open_reading`) or runs unattended.
    reading_without: Option<Vec<String>>,
    prompts: Prompts,
    timeout: Option<Duration>,
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
                prompts: Prompts::Plain,
                timeout: None,
            }),
            GitCli::NotFound => Err(ActionError::GitNotFound),
        }
    }

    /// Like `open`, for calls that only read: no hooks, no optional index
    /// refresh and none of the filter commands the repository's own config
    /// names, so showing a folder never runs what a downloaded `.git/config` says.
    pub fn open_reading(root: &Path) -> Result<Self, ActionError> {
        let mut git = Self::open(root)?;
        let filters = git.repository_filter_names()?;
        // `-c` cuts a name at its first `=`, so such a filter would stay on.
        let cannot_be_emptied =
            |name: &String| name.is_empty() || name.contains(['=', char::REPLACEMENT_CHARACTER]);
        if filters.iter().any(cannot_be_emptied) {
            return Err(repository_runs_commands());
        }
        git.reading_without = Some(filters);
        Ok(git)
    }

    /// Like `open`, for a call that may ask the user for a login: the answers
    /// come through `askpass`, requests carry `job`.
    pub fn open_interactive(root: &Path, job: u64) -> Result<Self, ActionError> {
        let mut git = Self::open(root)?;
        git.prompts = Prompts::Ask { job };
        Ok(git)
    }

    /// Like `open_reading` for calls that fetch without anyone looking: never
    /// asks, never opens a window, runs no hook and kills a hanging git.
    pub fn open_unattended(root: &Path) -> Result<Self, ActionError> {
        let mut git = Self::open_reading(root)?;
        git.prompts = Prompts::Unattended;
        git.timeout = Some(UNATTENDED_TIMEOUT);
        Ok(git)
    }

    /// Kills a call that runs longer than `limit`; it then fails like a network timeout.
    #[must_use]
    pub fn with_timeout(mut self, limit: Duration) -> Self {
        self.timeout = Some(limit);
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The `-c` settings every call of this git gets besides the common ones.
    fn extra_settings(&self) -> &'static [&'static str] {
        match self.prompts {
            Prompts::Unattended => &UNATTENDED_SETTINGS,
            Prompts::Plain | Prompts::Ask { .. } => &[],
        }
    }

    /// An unattended call does nothing but the one thing it was asked for.
    pub(crate) fn is_unattended(&self) -> bool {
        self.prompts == Prompts::Unattended
    }

    fn apply_prompts(&self, command: &mut Command) {
        match self.prompts {
            Prompts::Plain => {}
            Prompts::Ask { job } => {
                if let Some(setup) = crate::askpass::setup_for(job) {
                    command
                        .env("GIT_ASKPASS", &setup.askpass)
                        .env("SSH_ASKPASS", &setup.askpass)
                        .env("SSH_ASKPASS_REQUIRE", "force")
                        .env(crate::askpass::ENDPOINT_VARIABLE, &setup.endpoint)
                        .env(crate::askpass::TOKEN_VARIABLE, &setup.token);
                    // Old OpenSSH only uses an askpass when it believes there is a display.
                    if cfg!(all(unix, not(target_os = "macos")))
                        && std::env::var_os("DISPLAY").is_none()
                    {
                        command.env("DISPLAY", "ncrs:0");
                    }
                }
            }
            Prompts::Unattended => {
                command
                    .env("GIT_ALLOW_PROTOCOL", UNATTENDED_PROTOCOLS)
                    .env("GIT_ASKPASS", "")
                    .env("SSH_ASKPASS_REQUIRE", "never")
                    .env("GCM_INTERACTIVE", "never")
                    .env(
                        "GIT_SSH_COMMAND",
                        "ssh -o BatchMode=yes -o ConnectTimeout=15",
                    );
            }
        }
    }

    /// Only reads, or runs unattended: nothing the repository names may start.
    pub(crate) fn is_guarded(&self) -> bool {
        self.reading_without.is_some()
    }

    pub(crate) fn run(&self, args: &[&str], stdin: Option<&[u8]>) -> Result<Output, ActionError> {
        let mut command = base_command(&self.exe);
        command
            .arg("-C")
            .arg(&self.root)
            // A repository from a download must not make every call start a watcher it names.
            .args(["-c", "core.fsmonitor=false"]);
        for setting in self.extra_settings() {
            command.args(["-c", setting]);
        }
        self.apply_prompts(&mut command);
        if self.is_unattended() && args.first() == Some(&"push") {
            command.env("GIT_ALLOW_PROTOCOL", UNATTENDED_PUSH_PROTOCOLS);
        }
        if let Some(filters) = &self.reading_without {
            command
                .env("GIT_OPTIONAL_LOCKS", "0")
                .args(["-c", &format!("core.hooksPath={NULL_DEVICE}")]);
            // Reading must not go to the remote for objects of a partial clone; an
            // unattended pull or checkout has to.
            if !self.is_unattended() {
                command.env("GIT_NO_LAZY_FETCH", "1");
            }
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
        finish(child, self.timeout)
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

    fn settings_of(prompts: Prompts) -> std::collections::HashMap<String, Option<String>> {
        let git = Git {
            exe: PathBuf::from("git"),
            root: PathBuf::from("."),
            reading_without: None,
            prompts,
            timeout: None,
        };
        let mut command = Command::new("git");
        git.apply_prompts(&mut command);
        command
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|found| found.to_string_lossy().into_owned()),
                )
            })
            .collect()
    }

    fn value<'a>(
        settings: &'a std::collections::HashMap<String, Option<String>>,
        key: &str,
    ) -> Option<&'a str> {
        settings.get(key).and_then(Option::as_deref)
    }

    #[test]
    fn an_interactive_call_points_git_and_ssh_at_the_broker() {
        let settings = settings_of(Prompts::Ask { job: 9 });
        assert!(value(&settings, "NCRS_ASKPASS").is_some_and(|found| found.contains(':')));
        assert!(value(&settings, "NCRS_ASKPASS_TOKEN").is_some_and(|found| found.ends_with(":9")));
        assert_eq!(value(&settings, "SSH_ASKPASS_REQUIRE"), Some("force"));
        assert!(settings.contains_key("GIT_ASKPASS") && settings.contains_key("SSH_ASKPASS"));
    }

    #[test]
    fn an_unattended_call_switches_every_prompt_off() {
        let settings = settings_of(Prompts::Unattended);
        assert_eq!(value(&settings, "GIT_ASKPASS"), Some(""));
        assert_eq!(value(&settings, "SSH_ASKPASS_REQUIRE"), Some("never"));
        assert!(value(&settings, "GIT_SSH_COMMAND")
            .is_some_and(|found| found.contains("BatchMode=yes")));
        assert!(!settings.contains_key("NCRS_ASKPASS"));
    }

    #[test]
    fn an_unattended_call_allows_no_command_transport_and_no_maintenance() {
        let settings = settings_of(Prompts::Unattended);
        assert_eq!(
            value(&settings, "GIT_ALLOW_PROTOCOL"),
            Some("https:http:ssh:git:file")
        );
        let git = Git {
            exe: PathBuf::from("git"),
            root: PathBuf::from("."),
            reading_without: None,
            prompts: Prompts::Unattended,
            timeout: None,
        };
        let given = git.extra_settings();
        for wanted in [
            "protocol.ext.allow=never",
            "maintenance.auto=false",
            "gc.auto=0",
        ] {
            assert!(given.contains(&wanted), "{wanted}");
        }
        assert!(!settings_of(Prompts::Plain).contains_key("GIT_ALLOW_PROTOCOL"));
    }

    #[test]
    fn a_plain_call_leaves_the_process_settings_alone() {
        assert!(settings_of(Prompts::Plain).is_empty());
    }
}
