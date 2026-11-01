//! The command line below the panels: what a typed line asks for, the history
//! of lines, and running a line through the shell. No widgets; the view is
//! [`view`].

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub mod action;
pub mod ctx;
pub mod update;
pub mod view;

pub use action::CommandLineAction;
pub use ctx::CommandLineCtx;
pub use update::update;

pub const MAX_HISTORY: usize = 100;

/// What a submitted line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Submission<'a> {
    Empty,
    ChangeDirectory(&'a str),
    Shell(&'a str),
}

/// Reads a line. `cd` and `cd <path>` change the directory of the active
/// panel, a pair of surrounding quotes around the path is dropped; bare `cd`
/// goes home. Everything else is for the shell.
pub fn parse(line: &str) -> Submission<'_> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Submission::Empty;
    }
    let (word, rest) = trimmed
        .split_once(char::is_whitespace)
        .unwrap_or((trimmed, ""));
    if word != "cd" {
        return Submission::Shell(trimmed);
    }
    let path = strip_matching_quotes(rest.trim());
    Submission::ChangeDirectory(if path.is_empty() { "~" } else { path })
}

fn strip_matching_quotes(text: &str) -> &str {
    ['"', '\'']
        .into_iter()
        .find_map(|quote| text.strip_prefix(quote)?.strip_suffix(quote))
        .unwrap_or(text)
}

/// How a shell command ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellOutcome {
    Exited(i32),
    NoExitCode,
    NotStarted { reason: String },
}

/// `$SHELL`, or `/bin/sh` when it is unset or empty.
#[cfg(any(unix, test))]
fn unix_shell(shell_env: Option<&OsStr>) -> OsString {
    match shell_env {
        Some(shell) if !shell.is_empty() => shell.to_owned(),
        _ => OsString::from("/bin/sh"),
    }
}

/// A login shell: an app started from Finder or the Dock inherits launchd's
/// bare PATH, and only the login profile adds the user's (Homebrew and the like).
#[cfg(unix)]
fn shell_command(line: &str, shell: &OsStr) -> Command {
    let mut command = Command::new(shell);
    command.arg("-l").arg("-c").arg(line);
    command
}

#[cfg(windows)]
fn shell_command(line: &str, _shell: &OsStr) -> Command {
    use std::os::windows::process::CommandExt as _;
    let mut command = Command::new("cmd");
    command.arg("/C").raw_arg(line);
    command
}

/// Blocks until the command ends; call it on the blocking pool. Nothing is
/// piped: a background child that kept a pipe open would block the wait.
fn run_blocking(line: &str, directory: &Path, shell: &OsStr) -> ShellOutcome {
    let waited = shell_command(line, shell)
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match waited {
        Ok(status) => status
            .code()
            .map_or(ShellOutcome::NoExitCode, ShellOutcome::Exited),
        Err(error) => ShellOutcome::NotStarted {
            reason: error.to_string(),
        },
    }
}

pub async fn run(line: String, directory: PathBuf) -> ShellOutcome {
    #[cfg(unix)]
    let shell = unix_shell(std::env::var_os("SHELL").as_deref());
    #[cfg(windows)]
    let shell = OsString::new();
    tokio::task::spawn_blocking(move || run_blocking(&line, &directory, &shell))
        .await
        .unwrap_or_else(|error| ShellOutcome::NotStarted {
            reason: error.to_string(),
        })
}

/// The command line's state. Mutated only by the app.
#[derive(Debug, Default)]
pub struct CommandLine {
    typed: String,
    focused: bool,
    /// Oldest first.
    history: Vec<String>,
    /// Index into `history` while walking it.
    recalled: Option<usize>,
    /// What was typed before the walk began.
    draft: String,
    running: usize,
    /// Bumped on every change, so a late `cd` answer with an older id is dropped.
    cd_check_id: u64,
}

impl CommandLine {
    pub fn focus(&mut self) {
        self.focused = true;
    }

    /// Gives up the focus and forgets the typed text.
    pub fn leave(&mut self) {
        self.typed.clear();
        self.draft.clear();
        self.recalled = None;
        self.focused = false;
        self.cd_check_id += 1;
    }

    pub fn input(&mut self, text: String) {
        self.typed = text;
        self.focused = true;
        self.recalled = None;
        self.cd_check_id += 1;
    }

    pub fn typed(&self) -> &str {
        &self.typed
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Adds a line to the history unless it is blank or equal to the newest.
    pub fn remember(&mut self, entered: &str) {
        let line = entered.trim();
        if line.is_empty() || self.history.last().is_some_and(|newest| newest == line) {
            return;
        }
        if self.history.len() == MAX_HISTORY {
            self.history.remove(0);
        }
        self.history.push(line.to_owned());
    }

    /// One entry back. Returns whether the text changed.
    pub fn recall_previous(&mut self) -> bool {
        let index = match self.recalled {
            Some(0) => return false,
            Some(index) => index - 1,
            None => {
                let Some(newest) = self.history.len().checked_sub(1) else {
                    return false;
                };
                self.draft = std::mem::take(&mut self.typed);
                newest
            }
        };
        self.show_entry(index);
        true
    }

    /// One entry forward; past the newest, the text from before the walk.
    /// Returns whether the text changed.
    pub fn recall_next(&mut self) -> bool {
        let Some(index) = self.recalled else {
            return false;
        };
        if index + 1 < self.history.len() {
            self.show_entry(index + 1);
        } else {
            self.recalled = None;
            self.typed = std::mem::take(&mut self.draft);
            self.cd_check_id += 1;
        }
        true
    }

    fn show_entry(&mut self, index: usize) {
        self.recalled = Some(index);
        self.typed = self.history.get(index).cloned().unwrap_or_default();
        self.cd_check_id += 1;
    }

    pub fn started(&mut self) {
        self.running += 1;
    }

    pub fn finished(&mut self) {
        self.running = self.running.saturating_sub(1);
    }

    pub fn running(&self) -> usize {
        self.running
    }

    pub fn next_cd_check(&mut self) -> u64 {
        self.cd_check_id += 1;
        self.cd_check_id
    }

    pub fn is_current_cd_check(&self, id: u64) -> bool {
        self.cd_check_id == id
    }
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests;
