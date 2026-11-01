//! Numbers for comparing ncrs with other file managers, printed as a
//! Markdown table. Not a pass/fail check, so it is ignored by default and
//! run with `cargo test --release --bin ncrs perf_numbers -- --ignored
//! --nocapture`.
//!
//! The directories are created right before they are read, so the page
//! cache is warm. "Build view" is the widget tree `view` returns on every
//! keystroke; rasterising it on the CPU or GPU happens later in iced and
//! is not part of any number here.

use crate::app::*;
use crate::fs::search::{self, SearchEvent, SearchQuery, SearchTotals};
use crate::fs::transfer::{self, OnConflict};
use crate::fs::{read_directory, sort_entries, FileEntry, SortColumn, SortKey};
use std::time::{Duration, Instant};

const ENTRY_COUNTS: [usize; 3] = [1_000, 10_000, 100_000];
const MIDDLE_RUN: usize = 3;
const RUNS: usize = 2 * MIDDLE_RUN + 1;
const STEPS: u32 = 1_000;
/// Every `MARKED_EVERY`th file holds this word, for the content search.
const MARKER: &str = "needle-marker";
const MARKED_EVERY: usize = 1_000;
/// One file in twenty: the names ending in these two digits.
const NAME_GLOB: &str = "*05.txt;*25.txt;*45.txt;*65.txt;*85.txt";

fn median(mut samples: Vec<Duration>) -> Duration {
    assert_eq!(samples.len(), RUNS);
    samples.sort();
    samples[MIDDLE_RUN]
}

/// Microseconds below a millisecond, so a 30 µs view does not print as
/// "0.03 ms" next to a 266 ms read.
fn shown(duration: Duration) -> String {
    let micros = duration.as_secs_f64() * 1e6;
    if micros < 1000.0 {
        format!("{micros:.1} µs")
    } else {
        format!("{:.2} ms", micros / 1000.0)
    }
}

/// Median of `RUNS` timings of `operation` after one warm-up. `prepare`
/// and dropping the result stay outside the clock, so copying a 100k
/// listing is not counted as sorting it.
fn median_time<I, O>(
    mut prepare: impl FnMut() -> I,
    mut operation: impl FnMut(I) -> O,
) -> Duration {
    drop(operation(prepare()));
    let samples = std::iter::repeat_with(|| {
        let input = prepare();
        let start = Instant::now();
        let output = operation(input);
        let elapsed = start.elapsed();
        drop(output);
        elapsed
    })
    .take(RUNS)
    .collect();
    median(samples)
}

/// One directory in twenty, files of up to 4 KiB, and the marker word at
/// the start of every `MARKED_EVERY`th file.
fn populated_dir(entry_count: usize) -> tempfile::TempDir {
    let dir = tempfile::Builder::new()
        .prefix("ncrs-perf-")
        .tempdir()
        .unwrap();
    for i in 0..entry_count {
        if i % 20 == 0 {
            std::fs::create_dir(dir.path().join(format!("dir_{i:06}"))).unwrap();
        } else {
            let mut content = vec![b'x'; i % 4096];
            if i % MARKED_EVERY == 1 {
                content = format!("{MARKER} {}", "x".repeat(i % 4096)).into_bytes();
            }
            std::fs::write(dir.path().join(format!("File_{i:06}.txt")), content).unwrap();
        }
    }
    dir
}

async fn read_median(path: &std::path::Path, entry_count: usize) -> (Duration, Vec<FileEntry>) {
    let mut samples = Vec::with_capacity(RUNS + 1);
    let mut entries = Vec::new();
    for _ in 0..=RUNS {
        let start = Instant::now();
        let listing = read_directory(path.to_path_buf(), SortKey::default()).await;
        samples.push(start.elapsed());
        assert!(listing.error.is_none(), "{:?}", listing.error);
        assert_eq!(listing.entries.len(), entry_count + 1);
        entries = listing.entries;
    }
    samples.remove(0);
    (median(samples), entries)
}

/// What the panel does when a listing arrives for a directory it was not
/// showing before.
fn enter(app: &mut App, path: &std::path::Path, entries: Vec<FileEntry>) {
    app.panel_mut(PanelSide::Left).path = PathBuf::from("/");
    let request_id = app.panel_mut(PanelSide::Left).begin_load();
    drop(app.update(Message::Panel(PanelMessage::DirectoryLoaded {
        side: PanelSide::Left,
        request_id,
        path: path.to_path_buf(),
        entries,
        error: None,
        sorted_by: SortKey::default(),
        select: None,
        archive: None,
    })));
}

fn per_step(app: &mut App, message: &Message) -> Duration {
    let start = Instant::now();
    for _ in 0..STEPS {
        drop(app.update(message.clone()));
        drop(app.view());
    }
    start.elapsed() / STEPS
}

/// Runs the search the dialog runs, until its `Finished` event, and
/// returns the median of `RUNS` timings after one warm-up.
async fn search_median(query: &SearchQuery) -> (Duration, SearchTotals) {
    let mut samples = Vec::with_capacity(RUNS);
    let mut totals = None;
    for run in 0..=RUNS {
        let compiled = search::compile(query).unwrap();
        let start = Instant::now();
        let mut events = search::spawn(compiled);
        while let Some(event) = events.recv().await {
            if let SearchEvent::Finished(finished) = event {
                totals = Some(finished);
                break;
            }
        }
        if run > 0 {
            samples.push(start.elapsed());
        }
    }
    (median(samples), totals.unwrap())
}

/// Tags every entry of the left panel and builds the view, with the
/// tags cleared before each run.
fn tag_all_and_view(app: &mut App) -> Duration {
    let shared = std::cell::RefCell::new(app);
    median_time(
        || {
            drop(
                shared
                    .borrow_mut()
                    .update(Message::Panel(PanelMessage::ClearTags)),
            )
        },
        |()| {
            let mut borrowed = shared.borrow_mut();
            drop(borrowed.update(Message::Panel(PanelMessage::TagAll)));
            drop(borrowed.view());
        },
    )
}

/// What F5 costs on the UI thread with everything tagged: the dialog is
/// built and the next view drawn. The scan and the conflict count it
/// starts run on the blocking pool and are not part of it.
fn copy_dialog_and_view(app: &mut App) -> Duration {
    drop(app.update(Message::Panel(PanelMessage::TagAll)));
    let shared = std::cell::RefCell::new(app);
    median_time(
        || shared.borrow_mut().ops.transfer_dialog = None,
        |()| {
            let mut borrowed = shared.borrow_mut();
            drop(borrowed.update(Message::Transfer(TransferKind::Copy)));
            drop(borrowed.view());
        },
    )
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "prints timings, run with --release --ignored --nocapture"]
async fn print_performance_table() {
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    println!(
        "\n{} {}, {threads} threads, release build: {}\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
        !cfg!(debug_assertions)
    );
    println!(
        "| Entries | Open directory | Sort by name / size / date | Hand to panel \
         | Build view (top / end) | ↓ + view | PgDn + view | End + view |"
    );
    println!("|---:|---:|---:|---:|---:|---:|---:|---:|");

    let mut second_table = Vec::new();
    for entry_count in ENTRY_COUNTS {
        let dir = populated_dir(entry_count);
        let path = dir.path();
        let (open, entries) = read_median(path, entry_count).await;

        let sort_by = |column| {
            median_time(
                || entries.clone(),
                |mut listing| {
                    sort_entries(
                        &mut listing,
                        SortKey {
                            column,
                            descending: false,
                        },
                    );
                    listing
                },
            )
        };
        let by_name = sort_by(SortColumn::Name);
        let by_size = sort_by(SortColumn::Size);
        let by_date = sort_by(SortColumn::Modified);

        let mut app = App::with_environment(Environment::isolated()).0;
        app.visible_rows = 35;
        let hand_to_panel =
            median_time(|| entries.clone(), |listing| enter(&mut app, path, listing));
        let view_top = median_time(|| (), |()| drop(app.view()));
        let arrow = per_step(&mut app, &Message::Panel(PanelMessage::MoveSelection(1)));
        let page_down = per_step(&mut app, &Message::Panel(PanelMessage::PageDown));
        let end = per_step(&mut app, &Message::Panel(PanelMessage::SelectLast));
        let view_end = median_time(|| (), |()| drop(app.view()));

        println!(
            "| {entry_count} | {} | {} / {} / {} | {} | {} / {} | {} | {} | {} |",
            shown(open),
            shown(by_name),
            shown(by_size),
            shown(by_date),
            shown(hand_to_panel),
            shown(view_top),
            shown(view_end),
            shown(arrow),
            shown(page_down),
            shown(end),
        );

        let tag_all = tag_all_and_view(&mut app);
        let copy_dialog = copy_dialog_and_view(&mut app);
        app.ops.transfer_dialog = None;

        let by_name_query = SearchQuery {
            include: NAME_GLOB.to_owned(),
            ..SearchQuery::new(path.to_path_buf(), "")
        };
        let (name_search, name_totals) = search_median(&by_name_query).await;
        let (content_search, content_totals) =
            search_median(&SearchQuery::new(path.to_path_buf(), MARKER)).await;
        assert_eq!(content_totals.files, entry_count.div_ceil(MARKED_EVERY));

        let target = tempfile::Builder::new()
            .prefix("ncrs-perf-copy-")
            .tempdir()
            .unwrap();
        let start = Instant::now();
        let copied = transfer::transfer(
            path,
            target.path(),
            &transfer::Copy,
            &mut transfer::Counting,
            OnConflict::Fail,
        )
        .unwrap();
        let copy = start.elapsed();
        assert_eq!(copied, entry_count + 1);
        second_table.push(format!(
            "| {entry_count} | {} ({} hits) | {} ({} hits) | {} | {} | {} |",
            shown(name_search),
            name_totals.files,
            shown(content_search),
            content_totals.files,
            shown(copy),
            shown(tag_all),
            shown(copy_dialog),
        ));
    }
    println!(
        "\n| Entries | Search by name | Search by content | Copy folder (one run) \
         | Tag all + view | F5 with all tagged + view |"
    );
    println!("|---:|---:|---:|---:|---:|---:|");
    for row in second_table {
        println!("{row}");
    }
    println!(
        "\nOpen directory: readdir, one stat per entry and the sort by name, warm \
         page cache. Build view: the widget tree only, without rasterising. \
         Searches: median of seven runs from compile to the finished event, on the \
         blocking pool; name search matches one file in twenty, content search the \
         files holding the marker word. Copy: one run to a new folder on the same \
         disk. The last two columns run on the UI thread.\n"
    );
}
