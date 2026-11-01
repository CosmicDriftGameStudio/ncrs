use crate::app::*;
use crate::fs::FileEntry;
use std::time::{Duration, Instant};

/// Synthetic entries, so the measurement does not depend on what is in the
/// working directory.
fn entries(n: usize) -> Vec<FileEntry> {
    (0..n)
        .map(|i| FileEntry {
            name: format!("file_{i}.txt").into(),
            path: std::path::PathBuf::from(format!("/tmp/file_{i}.txt")),
            is_dir: i % 5 == 0,
            is_symlink: false,
            is_parent: i == 0,
            size: 1024 * (i as u64 % 900),
            modified: None,
            folder_size: None,
        })
        .collect()
}

fn app_with_rows(n: usize) -> App {
    let mut app = App::with_environment(Environment::isolated()).0;
    app.panels.left.entries = entries(n);
    app.panels.right.entries = entries(n);
    app.visible_rows = 35;
    app
}

/// Regression: navigation felt laggy — the highlighted row visibly caught up
/// about a second after the key press.
///
/// `view` is rebuilt from scratch on every keystroke, so its cost is what
/// the user waits for. Measured at ~0.1 ms, which is well inside budget and
/// rules the view out as the cause: the remaining cost is rasterisation,
/// which this test cannot reach. See ROADMAP.md, T2.
#[test]
fn a_frame_costs_far_less_than_a_frame_budget() {
    let app = app_with_rows(200);

    let _ = app.view(); // warm-up
    let frames = 60;
    let start = Instant::now();
    for _ in 0..frames {
        let _ = app.view();
    }
    let per_frame = start.elapsed() / frames;

    assert!(
        per_frame < Duration::from_millis(16),
        "a frame costs {per_frame:?}; the 60fps budget is 16ms"
    );
}
