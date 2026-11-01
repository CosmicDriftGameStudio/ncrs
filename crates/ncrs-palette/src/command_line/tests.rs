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
    assert!(line.recall_previous());
    assert_eq!(line.typed(), "two");
    assert!(line.recall_previous());
    assert_eq!(line.typed(), "one");
    assert!(!line.recall_previous());
    assert!(line.recall_next());
    assert_eq!(line.typed(), "two");
    assert!(line.recall_next());
    assert_eq!(line.typed(), "half");
    assert!(!line.recall_next());
}

#[test]
fn an_empty_history_has_nothing_to_walk() {
    let mut line = CommandLine::default();
    assert!(!line.recall_previous());
    assert!(!line.recall_next());
}

#[test]
fn typing_during_a_walk_ends_it() {
    let mut line = history_of(&["one", "two"]);
    line.recall_previous();
    line.input("two!".into());
    assert!(!line.recall_next());
    assert_eq!(line.typed(), "two!");
    line.recall_previous();
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
        let outcome = run_blocking("exit 0", directory.path(), OsStr::new("/nonexistent/shell"));
        assert!(matches!(outcome, ShellOutcome::NotStarted { .. }));
    }

    #[test]
    fn a_killed_shell_has_no_exit_code() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(sh("kill -9 $$", directory.path()), ShellOutcome::NoExitCode);
    }
}
