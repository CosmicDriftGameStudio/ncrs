//! What the App Sandbox of the Store build allows, probed with a copy of this
//! test binary that is signed ad hoc with the Store entitlements and run as a
//! child process. The copy runs in a real sandbox: it gets a container and is
//! refused files outside it.

// reason: a failing assertion is the signal in a test
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use super::delete::file_manager_trash;

const TEST_NAME: &str = "fs::sandbox_probe_tests::the_store_sandbox_allows_what_ncrs_relies_on";
const STEP_VARIABLE: &str = "NCRS_SANDBOX_PROBE_STEP";
const FOLDER_VARIABLE: &str = "NCRS_SANDBOX_PROBE_FOLDER";
const BUNDLE_ID: &str = "rocks.ncrs.sandbox-probe";
const REPORT_PREFIX: &str = "PROBE ";

fn report(key: &str, value: impl std::fmt::Display) {
    println!("{REPORT_PREFIX}{key}={value}");
}

fn outcome<T, E: std::fmt::Display>(result: Result<T, E>) -> String {
    match result {
        Ok(_) => "ok".to_string(),
        Err(err) => format!("refused: {err}"),
    }
}

/// In the container's home folder, where the sandboxed copy may write. The
/// name of the parent's folder keeps two runs at the same time apart.
fn in_the_container(folder: &Path, ending: &str) -> PathBuf {
    let run = folder.file_name().unwrap().to_string_lossy();
    PathBuf::from(std::env::var_os("HOME").unwrap()).join(format!("{run}.{ending}"))
}

/// Runs inside the sandbox and reports what it saw; the parent judges it.
fn sandboxed_step(step: &str, folder: &Path) {
    let probe_target = || in_the_container(folder, "txt");
    let probe_journal = || in_the_container(folder, "toml");
    report(
        "container",
        std::env::var_os("APP_SANDBOX_CONTAINER_ID").is_some(),
    );
    report(
        "read_outside",
        outcome(std::fs::read_to_string(folder.join("control.txt"))),
    );
    match step {
        "trash" => {
            std::fs::write(probe_target(), "probe").unwrap();
            let trash = file_manager_trash(probe_journal());
            report("trash", outcome(trash.trash(&probe_target())));
            for item in trash.list().unwrap() {
                report("landed", Path::new(&item.key).display());
            }
        }
        "restore" => {
            let trash = file_manager_trash(probe_journal());
            let items = trash.list();
            report("listed", outcome(items.as_ref()));
            for item in items.unwrap_or_default() {
                report(
                    "stat_trashed",
                    outcome(std::fs::symlink_metadata(Path::new(&item.key))),
                );
                report("restore", format!("{:?}", trash.restore(&item)));
            }
            report(
                "content_back",
                outcome(std::fs::read_to_string(probe_target())),
            );
            drop(std::fs::remove_file(probe_target()));
            drop(std::fs::remove_file(probe_journal()));
        }
        "paste" => {
            report(
                "read_pasted_directly",
                outcome(std::fs::read_to_string(folder.join("pasted.txt"))),
            );
            match crate::clipboard_paste::read_system_clipboard() {
                Some(crate::clipboard_paste::PastedContent::Files(paths)) => {
                    for path in paths {
                        report("pasted_path", path.display());
                        report(
                            "read_pasted",
                            match std::fs::read_to_string(&path) {
                                Ok(text) => text,
                                Err(err) => format!("refused: {err}"),
                            },
                        );
                    }
                }
                other => report("pasted", format!("no files: {}", other.is_some())),
            }
        }
        _ => panic!("unknown step {step}"),
    }
}

/// A bundle around a copy of this test binary, signed with the entitlements
/// of the Store build. Without a bundle identifier the sandbox refuses to
/// start the process.
fn sandboxed_copy(folder: &Path) -> PathBuf {
    let contents = folder.join("Probe.app/Contents");
    let executable = contents.join("MacOS/ncrs-sandbox-probe");
    std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
    std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    std::fs::write(
        contents.join("Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{BUNDLE_ID}</string>
<key>CFBundleExecutable</key><string>ncrs-sandbox-probe</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
"#
        ),
    )
    .unwrap();
    let entitlements =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging/macos/sandbox.entitlements");
    let signed = std::process::Command::new("codesign")
        .args(["--force", "--sign", "-", "--entitlements"])
        .arg(&entitlements)
        .arg(folder.join("Probe.app"))
        .output()
        .unwrap();
    assert!(
        signed.status.success(),
        "codesign failed: {}",
        String::from_utf8_lossy(&signed.stderr)
    );
    executable
}

/// The reports of one step, as `key=value` lines.
fn run_step(executable: &Path, step: &str, folder: &Path) -> Vec<String> {
    let output = std::process::Command::new(executable)
        .args([TEST_NAME, "--exact", "--nocapture"])
        .env(STEP_VARIABLE, step)
        .env(FOLDER_VARIABLE, folder)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("test result: ok. 1 passed"),
        "the step {step} failed in the sandbox\n--- stdout\n{stdout}\n--- stderr\n{stderr}"
    );
    format!("{stdout}\n{stderr}")
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix(REPORT_PREFIX))
        .map(str::to_owned)
        .collect()
}

/// The value the step reported under `key`.
fn reported<'lines>(lines: &'lines [String], key: &str) -> &'lines str {
    lines
        .iter()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
        .unwrap_or_else(|| panic!("the step did not report {key}: {lines:#?}"))
}

fn assert_sandboxed(lines: &[String]) {
    assert_eq!(
        reported(lines, "container"),
        "true",
        "no container: {lines:#?}"
    );
    assert!(
        reported(lines, "read_outside").starts_with("refused"),
        "the copy is not confined: {lines:#?}"
    );
}

/// Removes what the probe left in the user's trash. Never panics.
struct LeftInTheTrash(Vec<PathBuf>);

impl LeftInTheTrash {
    /// Only what the child reported landing in `~/.Trash` under the name of
    /// this run; a Finder rename such as "… 2.txt" keeps the name.
    fn of_this_run(lines: &[String], folder: &Path) -> Self {
        let run = folder.file_name().unwrap().to_string_lossy().into_owned();
        let user_trash = std::fs::canonicalize(crate::fs::home_dir().join(".Trash")).ok();
        Self(
            lines
                .iter()
                .filter_map(|line| line.strip_prefix("landed="))
                .map(PathBuf::from)
                .filter(|landed| {
                    landed
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy().contains(&run))
                        && landed
                            .parent()
                            .and_then(|holder| std::fs::canonicalize(holder).ok())
                            .is_some_and(|holder| Some(holder) == user_trash)
                })
                .collect(),
        )
    }
}

impl Drop for LeftInTheTrash {
    fn drop(&mut self) {
        for path in &self.0 {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => eprintln!("cleanup: {} stays in the trash: {err}", path.display()),
            }
        }
    }
}

#[test]
fn the_store_sandbox_allows_what_ncrs_relies_on() {
    if let (Some(step), Some(folder)) = (
        std::env::var(STEP_VARIABLE).ok(),
        std::env::var_os(FOLDER_VARIABLE),
    ) {
        sandboxed_step(&step, Path::new(&folder));
        return;
    }
    let folder = tempfile::Builder::new()
        .prefix("ncrs-sandbox-probe-")
        .tempdir()
        .unwrap();
    std::fs::write(folder.path().join("control.txt"), "control").unwrap();
    let executable = sandboxed_copy(folder.path());

    let trashed = run_step(&executable, "trash", folder.path());
    let left_in_the_trash = LeftInTheTrash::of_this_run(&trashed, folder.path());
    assert_eq!(
        left_in_the_trash.0.len(),
        1,
        "the cleanup would miss what landed: {trashed:#?}"
    );
    let restored = run_step(&executable, "restore", folder.path());

    let pasted_file = folder.path().join("pasted.txt");
    std::fs::write(&pasted_file, "pasted").unwrap();
    let _clipboard = crate::clipboard_system_tests::ClipboardSnapshot::take();
    crate::clipboard_system_tests::platform::put_files_as_the_file_manager_does(&[pasted_file]);
    let pasted = run_step(&executable, "paste", folder.path());

    assert_sandboxed(&trashed);
    assert_eq!(reported(&trashed, "trash"), "ok", "{trashed:#?}");
    assert!(
        reported(&trashed, "landed").contains("/.Trash/"),
        "not in the user's trash: {trashed:#?}"
    );

    // (a) A later process, as after a restart of the app, puts it back.
    assert_sandboxed(&restored);
    assert_eq!(reported(&restored, "stat_trashed"), "ok", "{restored:#?}");
    assert_eq!(reported(&restored, "restore"), "Ok(())", "{restored:#?}");
    assert_eq!(reported(&restored, "content_back"), "ok", "{restored:#?}");

    // (b) A file the sandbox may not read is readable once it was pasted.
    assert_sandboxed(&pasted);
    assert!(
        reported(&pasted, "read_pasted_directly").starts_with("refused"),
        "{pasted:#?}"
    );
    assert_eq!(reported(&pasted, "read_pasted"), "pasted", "{pasted:#?}");
}
