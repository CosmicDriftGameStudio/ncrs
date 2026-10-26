//! The command line below the panels: what a typed line asks for, the history
//! of lines, and running a line through the shell. No widgets; the view is
//! [`crate::ui::command_line`].

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
    pub fn previous(&mut self) -> bool {
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
    pub fn next(&mut self) -> bool {
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
mod tests {
    use super::*;

    #[test]
    fn blank_lines_are_empty() {
        assert_eq!(parse(""), Submission::Empty);
        assert_eq!(parse("   \t"), Submission::Empty);
    }

    #[test]
    fn bare_cd_goes_home() {
        assert_eq!(parse("cd"), Submission::ChangeDirectory("~"));
        assert_eq!(parse("  cd  "), Submission::ChangeDirectory("~"));
    }

    #[test]
    fn cd_takes_the_rest_of_the_line_as_the_path() {
        assert_eq!(parse("cd .."), Submission::ChangeDirectory(".."));
        assert_eq!(parse("cd ~/x "), Submission::ChangeDirectory("~/x"));
        assert_eq!(parse("cd\t.."), Submission::ChangeDirectory(".."));
    }

    #[test]
    fn one_pair_of_matching_quotes_is_dropped() {
        assert_eq!(
            parse("cd \"My Docs\""),
            Submission::ChangeDirectory("My Docs")
        );
        assert_eq!(parse("cd 'a b'"), Submission::ChangeDirectory("a b"));
        assert_eq!(parse("cd \"a b'"), Submission::ChangeDirectory("\"a b'"));
    }

    #[test]
    fn words_that_only_start_with_cd_go_to_the_shell() {
        assert_eq!(parse("cdx"), Submission::Shell("cdx"));
        assert_eq!(parse("cd."), Submission::Shell("cd."));
        assert_eq!(parse("./cd"), Submission::Shell("./cd"));
        assert_eq!(parse(" ls -la "), Submission::Shell("ls -la"));
    }

    #[test]
    fn the_shell_is_the_environment_one_or_bin_sh() {
        assert_eq!(unix_shell(Some(OsStr::new("/bin/zsh"))), "/bin/zsh");
        assert_eq!(unix_shell(None), "/bin/sh");
        assert_eq!(unix_shell(Some(OsStr::new(""))), "/bin/sh");
    }

    fn history_of(lines: &[&str]) -> CommandLine {
        let mut line = CommandLine::default();
        for each in lines {
            line.remember(each);
        }
        line
    }

    #[test]
    fn remember_skips_blank_lines_and_a_repeat_of_the_newest() {
        let mut line = history_of(&["ls", "  ", "ls", "pwd", "ls"]);
        line.remember("ls");
        assert_eq!(line.history, ["ls", "pwd", "ls"]);
    }

    #[test]
    fn the_history_drops_its_oldest_entry_at_the_cap() {
        let mut line = CommandLine::default();
        for number in 0..MAX_HISTORY + 5 {
            line.remember(&format!("echo {number}"));
        }
        assert_eq!(line.history.len(), MAX_HISTORY);
        assert_eq!(line.history.first().map(String::as_str), Some("echo 5"));
    }

    #[test]
    fn walking_back_and_forth_restores_the_draft() {
        let mut line = history_of(&["one", "two"]);
        line.input("half".into());
        assert!(line.previous());
        assert_eq!(line.typed(), "two");
        assert!(line.previous());
        assert_eq!(line.typed(), "one");
        assert!(!line.previous());
        assert!(line.next());
        assert_eq!(line.typed(), "two");
        assert!(line.next());
        assert_eq!(line.typed(), "half");
        assert!(!line.next());
    }

    #[test]
    fn an_empty_history_has_nothing_to_walk() {
        let mut line = CommandLine::default();
        assert!(!line.previous());
        assert!(!line.next());
    }

    #[test]
    fn typing_during_a_walk_ends_it() {
        let mut line = history_of(&["one", "two"]);
        line.previous();
        line.input("two!".into());
        assert!(!line.next());
        assert_eq!(line.typed(), "two!");
        line.previous();
        assert_eq!(line.typed(), "two");
    }

    #[test]
    fn leaving_clears_the_text_and_the_focus() {
        let mut line = history_of(&["one"]);
        line.input("abc".into());
        line.leave();
        assert_eq!(line.typed(), "");
        assert!(!line.is_focused());
    }

    #[test]
    fn an_older_cd_answer_is_not_current() {
        let mut line = CommandLine::default();
        let id = line.next_cd_check();
        assert!(line.is_current_cd_check(id));
        line.input("x".into());
        assert!(!line.is_current_cd_check(id));
    }

    #[test]
    fn the_running_count_never_goes_below_zero() {
        let mut line = CommandLine::default();
        line.finished();
        line.started();
        line.started();
        line.finished();
        assert_eq!(line.running(), 1);
    }

    #[cfg(unix)]
    mod shell {
        use super::*;

        fn sh(line: &str, directory: &Path) -> ShellOutcome {
            run_blocking(line, directory, OsStr::new("/bin/sh"))
        }

        #[test]
        fn the_line_goes_to_a_login_shell() {
            let command = shell_command("ls -la", OsStr::new("/bin/zsh"));
            assert_eq!(command.get_program(), "/bin/zsh");
            assert_eq!(
                command.get_args().collect::<Vec<_>>(),
                ["-l", "-c", "ls -la"]
            );
        }

        #[test]
        fn the_exit_code_is_reported() {
            let directory = tempfile::tempdir().unwrap();
            assert_eq!(sh("exit 3", directory.path()), ShellOutcome::Exited(3));
            assert_eq!(sh("exit 0", directory.path()), ShellOutcome::Exited(0));
        }

        #[test]
        fn the_line_runs_in_the_given_directory() {
            let directory = tempfile::tempdir().unwrap();
            sh("pwd > here.txt", directory.path());
            let written = std::fs::read_to_string(directory.path().join("here.txt")).unwrap();
            assert_eq!(
                std::fs::canonicalize(written.trim()).unwrap(),
                std::fs::canonicalize(directory.path()).unwrap()
            );
        }

        #[test]
        fn stdin_is_closed() {
            let directory = tempfile::tempdir().unwrap();
            sh("read x; echo $? > status.txt", directory.path());
            let status = std::fs::read_to_string(directory.path().join("status.txt")).unwrap();
            assert_ne!(status.trim(), "0");
        }

        #[test]
        fn a_missing_shell_is_not_started() {
            let directory = tempfile::tempdir().unwrap();
            let outcome =
                run_blocking("exit 0", directory.path(), OsStr::new("/nonexistent/shell"));
            assert!(matches!(outcome, ShellOutcome::NotStarted { .. }));
        }

        #[test]
        fn a_killed_shell_has_no_exit_code() {
            let directory = tempfile::tempdir().unwrap();
            assert_eq!(sh("kill -9 $$", directory.path()), ShellOutcome::NoExitCode);
        }
    }
}
