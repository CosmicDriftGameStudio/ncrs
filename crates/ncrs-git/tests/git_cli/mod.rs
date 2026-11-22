//! Real repositories made with the git command, for the write actions.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    dead_code,
    reason = "test fixtures: a failing setup is the signal; every test file uses a part"
)]

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Once;

use ncrs_git::Git;
use tempfile::TempDir;

static ISOLATE: Once = Once::new();

/// Keeps the user's and the system's git config out of the tests, in this
/// process and in the `git` the code under test starts.
fn isolate_config() {
    ISOLATE.call_once(|| {
        let empty = std::env::temp_dir().join("ncrs-git-tests-empty-config");
        fs::write(&empty, "").expect("empty config");
        std::env::set_var("GIT_CONFIG_GLOBAL", &empty);
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
    });
}

/// Runs git in `dir`, asserts success and returns trimmed stdout.
pub fn sh(dir: &Path, args: &[&str]) -> String {
    isolate_config();
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("start git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

pub fn write(dir: &Path, relative: &str, content: &str) {
    let path = dir.join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("folder");
    fs::write(path, content).expect("write");
}

pub fn read(dir: &Path, relative: &str) -> String {
    fs::read_to_string(dir.join(relative)).expect("read")
}

pub struct Repo {
    pub dir: TempDir,
    pub git: Git,
}

impl Repo {
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn sh(&self, args: &[&str]) -> String {
        sh(self.path(), args)
    }

    pub fn commit_file(&self, relative: &str, content: &str, message: &str) {
        write(self.path(), relative, content);
        self.sh(&["add", "--", relative]);
        self.sh(&["commit", "-q", "-m", message]);
    }
}

fn configure(dir: &Path) {
    sh(dir, &["config", "user.name", "Test"]);
    sh(dir, &["config", "user.email", "test@example.com"]);
    sh(dir, &["config", "commit.gpgsign", "false"]);
    sh(dir, &["config", "pull.rebase", "false"]);
    sh(dir, &["config", "merge.conflictStyle", "merge"]);
}

fn wrap(dir: TempDir) -> Repo {
    let git = Git::open(dir.path()).expect("git available");
    Repo { dir, git }
}

/// A repository on `main` with one commit holding `a.txt`.
pub fn repo() -> Repo {
    isolate_config();
    let dir = TempDir::new().expect("tempdir");
    sh(dir.path(), &["init", "-q", "-b", "main"]);
    configure(dir.path());
    let repo = wrap(dir);
    repo.commit_file("a.txt", "one\ntwo\nthree\n", "first");
    repo
}

/// A bare "origin" and a repository with `main` pushed to it and tracking it.
pub fn with_origin() -> (TempDir, Repo) {
    isolate_config();
    let origin = TempDir::new().expect("tempdir");
    sh(origin.path(), &["init", "-q", "--bare", "-b", "main"]);
    let repo = repo();
    let url = origin.path().to_str().expect("utf8 path").to_string();
    repo.sh(&["remote", "add", "origin", &url]);
    repo.sh(&["push", "-q", "-u", "origin", "main"]);
    (origin, repo)
}

/// A second working copy of the same origin.
pub fn clone_of(origin: &TempDir) -> Repo {
    isolate_config();
    let dir = TempDir::new().expect("tempdir");
    let url = origin.path().to_str().expect("utf8 path").to_string();
    sh(dir.path(), &["clone", "-q", &url, "."]);
    configure(dir.path());
    wrap(dir)
}

/// `count` numbered lines, so changes at both ends land in separate hunks.
pub fn numbered(count: usize, line_ending: &str) -> String {
    (1..=count)
        .map(|number| format!("line {number}{line_ending}"))
        .collect()
}
