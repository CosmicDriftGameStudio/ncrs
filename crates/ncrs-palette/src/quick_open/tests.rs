use super::*;

fn chunk(paths: &[&str]) -> Chunk {
    paths.iter().map(|path| Box::<str>::from(*path)).collect()
}

fn ranked(
    query: &str,
    chunks: &[Chunk],
    start_chunk: usize,
    cancelled: &dyn Fn() -> bool,
) -> Vec<Hit> {
    rank(query, chunks, start_chunk, cancelled).unwrap()
}

fn never() -> bool {
    false
}

fn texts<'a>(chunks: &'a [Chunk], hits: &[Hit]) -> Vec<&'a str> {
    hits.iter()
        .map(|hit| path_of(chunks, hit.chunk, hit.index))
        .collect()
}

fn file_events(root: &str, relative: &[&str]) -> Vec<WalkEvent> {
    relative
        .iter()
        .map(|name| WalkEvent::File {
            path: PathBuf::from(root).join(name),
        })
        .collect()
}

fn open(root: &str, relative: &[&str]) -> QuickOpen {
    let mut quick_open = QuickOpen::new(PathBuf::from(root), 1);
    quick_open.apply_events(file_events(root, relative));
    quick_open
}

fn type_and_rank(quick_open: &mut QuickOpen, query: &str) {
    let job = quick_open.set_query(query.to_owned()).unwrap();
    let QuickOpenMessage::Ranked {
        walk_generation,
        generation,
        hits,
        chunks_scored,
    } = job.run()
    else {
        panic!("a ranking answers with Ranked");
    };
    quick_open.apply_ranked(walk_generation, generation, hits, chunks_scored);
}

#[test]
fn scattered_letters_find_the_file_and_other_paths_are_left_out() {
    let chunks = [chunk(&[
        "README.md",
        "src/config.rs",
        "src/main.rs",
        "docs/guide.md",
    ])];
    let hits = ranked("cfgrs", &chunks, 0, &never);
    assert_eq!(texts(&chunks, &hits), ["src/config.rs"]);
    assert!(!hits[0].highlight.is_empty());
}

#[test]
fn the_highlight_cuts_at_characters_not_at_bytes() {
    let chunks = [chunk(&["docs/Ärger.rs"])];
    let hits = ranked("ärg", &chunks, 0, &never);
    let path = path_of(&chunks, 0, 0);
    let marked: Vec<&str> = hits[0]
        .highlight
        .iter()
        .map(|range| &path[range.clone()])
        .collect();
    assert_eq!(marked, ["Ärg"]);
}

#[test]
fn separated_matches_become_separate_ranges() {
    let chunks = [chunk(&["src/config.rs"])];
    let hits = ranked("cfg", &chunks, 0, &never);
    let path = path_of(&chunks, 0, 0);
    let marked: Vec<&str> = hits[0]
        .highlight
        .iter()
        .map(|range| &path[range.clone()])
        .collect();
    assert_eq!(marked, ["c", "f", "g"]);
}

#[test]
fn query_characters_that_mean_something_to_fzf_match_literally() {
    let chunks = [chunk(&["notes/!todo^.txt", "notes/todo.txt"])];
    let hits = ranked("!todo^", &chunks, 0, &never);
    assert_eq!(texts(&chunks, &hits), ["notes/!todo^.txt"]);
}

#[test]
fn equal_scores_are_ordered_by_length_then_alphabet() {
    let chunks = [chunk(&["b/mod.rs", "a/mod.rs", "mod.rs"])];
    let first = ranked("mod", &chunks, 0, &never);
    let second = ranked("mod", &chunks, 0, &never);
    assert_eq!(first, second);
    let shown = texts(&chunks, &first);
    assert_eq!(shown.first(), Some(&"mod.rs"));
    let rest: Vec<&str> = shown.iter().skip(1).copied().collect();
    assert_eq!(rest, ["a/mod.rs", "b/mod.rs"]);
}

#[test]
fn at_most_twelve_hits_and_they_are_the_best_twelve() {
    let names: Vec<String> = (0..40).map(|i| format!("dir{i}/file{i}.txt")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let chunks = [chunk(&refs[..20]), chunk(&refs[20..])];
    let hits = ranked("file", &chunks, 0, &never);
    assert_eq!(hits.len(), MAX_HITS);
    let ordered = merge_hits(&chunks, hits.clone(), Vec::new());
    assert_eq!(hits, ordered);
}

#[test]
fn a_chunk_that_arrives_while_a_query_is_set_keeps_the_top_hits_right() {
    let mut quick_open = open("/r", &["a/config.rs", "b/other.txt"]);
    type_and_rank(&mut quick_open, "cfg");
    assert_eq!(quick_open.hits.len(), 1);
    quick_open.apply_events(file_events("/r", &["config.rs", "c/xcfgx.txt"]));
    let shown: Vec<&str> = quick_open
        .hits
        .iter()
        .map(|hit| quick_open.relative_path(hit))
        .collect();
    assert_eq!(shown.len(), 3);
    let all_at_once = ranked("cfg", &quick_open.candidates, 0, &never);
    assert_eq!(quick_open.hits, all_at_once);
}

#[test]
fn the_cursor_stays_on_its_hit_when_better_ones_come_in() {
    let mut quick_open = open("/r", &["a/config.rs", "a/x/c/o/n/f/i/g.rs"]);
    type_and_rank(&mut quick_open, "config");
    quick_open.move_cursor(1);
    let before = quick_open.selected_path();
    quick_open.apply_events(file_events("/r", &["config.rs"]));
    assert_eq!(quick_open.selected_path(), before);
}

#[test]
fn a_ranking_of_an_older_query_is_dropped() {
    let mut quick_open = open("/r", &["src/config.rs", "README.md"]);
    let old = quick_open.set_query("cfg".to_owned()).unwrap().run();
    let _newer = quick_open.set_query("readme".to_owned()).unwrap();
    let QuickOpenMessage::Ranked {
        walk_generation,
        generation,
        hits,
        chunks_scored,
    } = old
    else {
        panic!("Ranked expected");
    };
    quick_open.apply_ranked(walk_generation, generation, hits, chunks_scored);
    assert!(quick_open.hits.is_empty());
    assert!(quick_open.is_ranking());
}

#[test]
fn a_ranking_that_misses_later_chunks_gets_them_ranked_on_arrival() {
    let mut quick_open = open("/r", &["a/config.rs"]);
    let job = quick_open.set_query("cfg".to_owned()).unwrap();
    quick_open.apply_events(file_events("/r", &["b/cfg.txt"]));
    assert!(quick_open.hits.is_empty(), "waits while the ranking runs");
    let QuickOpenMessage::Ranked {
        walk_generation,
        generation,
        hits,
        chunks_scored,
    } = job.run()
    else {
        panic!("Ranked expected");
    };
    assert_eq!(chunks_scored, 1);
    quick_open.apply_ranked(walk_generation, generation, hits, chunks_scored);
    assert_eq!(quick_open.hits.len(), 2);
    assert!(!quick_open.is_ranking());
}

#[test]
fn a_ranking_of_an_earlier_box_does_not_land_in_a_later_one() {
    let mut first_box = QuickOpen::new(PathBuf::from("/r"), 1);
    first_box.apply_events(file_events("/r", &["src/config.rs"]));
    let late = first_box.set_query("cfg".to_owned()).unwrap();

    let mut second_box = QuickOpen::new(PathBuf::from("/r"), 2);
    second_box.apply_events(file_events("/r", &["README.md"]));
    let waiting = second_box.set_query("cfg".to_owned());
    assert!(waiting.is_some());

    let QuickOpenMessage::Ranked {
        walk_generation,
        generation,
        hits,
        chunks_scored,
    } = late.run()
    else {
        panic!("Ranked expected");
    };
    second_box.apply_ranked(walk_generation, generation, hits, chunks_scored);
    assert!(second_box.hits.is_empty());
    assert!(second_box.is_ranking());
    assert_eq!(second_box.scored_chunks, 0);
}

#[test]
fn a_cancelled_rescore_reports_nothing_scored() {
    let mut quick_open = open("/r", &["a/config.rs"]);
    let stale = quick_open.set_query("cfg".to_owned()).unwrap();
    let _newer = quick_open.set_query("conf".to_owned()).unwrap();
    let names: Vec<String> = (0..CANCEL_CHECK_INTERVAL * 2)
        .map(|i| format!("file{i}.txt"))
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    quick_open.apply_events(file_events("/r", &refs));
    let cancelled = Rescore {
        chunks: quick_open.candidates.clone(),
        ..stale
    };
    let QuickOpenMessage::Ranked {
        hits,
        chunks_scored,
        ..
    } = cancelled.run()
    else {
        panic!("Ranked expected");
    };
    assert!(hits.is_empty());
    assert_eq!(chunks_scored, 0);
}

#[test]
fn ranking_now_gives_the_hits_and_drops_the_late_background_result() {
    let mut quick_open = open("/r", &["src/config.rs", "README.md"]);
    let background = quick_open.set_query("cfgrs".to_owned()).unwrap();
    assert!(quick_open.hits.is_empty());
    quick_open.rank_now();
    assert!(!quick_open.is_ranking());
    assert_eq!(
        quick_open.selected_path(),
        Some(PathBuf::from("/r").join("src/config.rs"))
    );
    let QuickOpenMessage::Ranked {
        walk_generation,
        generation,
        hits,
        chunks_scored,
    } = background.run()
    else {
        panic!("Ranked expected");
    };
    quick_open.apply_ranked(walk_generation, generation, hits, chunks_scored);
    assert_eq!(quick_open.hits.len(), 1);
}

#[test]
fn a_cancelled_ranking_stops_early() {
    let names: Vec<String> = (0..CANCEL_CHECK_INTERVAL * 2)
        .map(|i| format!("file{i}.txt"))
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let chunks = [chunk(&refs)];
    assert_eq!(rank("file", &chunks, 0, &|| true), None);
}

#[test]
fn an_empty_query_has_no_hits_and_nothing_to_rank() {
    let mut quick_open = open("/r", &["a.txt"]);
    assert!(quick_open.set_query(String::new()).is_none());
    assert!(quick_open.set_query("  ".to_owned()).is_none());
    assert!(quick_open.hits.is_empty());
    assert_eq!(quick_open.file_count, 1);
}

#[test]
fn finishing_the_walk_is_recorded() {
    let mut quick_open = open("/r", &["a.txt"]);
    assert!(quick_open.walking);
    quick_open.apply_events(vec![WalkEvent::Finished { truncated: true }]);
    assert!(!quick_open.walking);
    assert!(quick_open.truncated);
}

#[test]
fn the_selected_path_joins_the_root() {
    let mut quick_open = open("/r", &["src/config.rs"]);
    type_and_rank(&mut quick_open, "config");
    assert_eq!(
        quick_open.selected_path(),
        Some(PathBuf::from("/r").join("src/config.rs"))
    );
}

fn document(path: &str) -> FoundDocument {
    FoundDocument {
        path: PathBuf::from(path),
        read_by: ncrs_core::TextSource::Pdf,
    }
}

fn with_documents_on(files: &[&str], query: &str) -> QuickOpen {
    let mut quick_open = QuickOpen::new(PathBuf::from("/r"), 1).with_documents(true);
    quick_open.apply_events(file_events("/r", files));
    type_and_rank(&mut quick_open, query);
    quick_open
}

fn names(quick_open: &QuickOpen) -> Vec<String> {
    quick_open
        .hits()
        .iter()
        .map(|hit| quick_open.relative_path(hit).to_owned())
        .collect()
}

#[test]
fn name_hits_stand_while_the_full_text_search_runs_and_after_it_failed() {
    let mut quick_open = with_documents_on(&["rechnung.txt", "notes.md"], "rechnung");
    let before = names(&quick_open);
    assert_eq!(before, ["rechnung.txt"]);

    let request = quick_open.request_documents().unwrap();
    assert_eq!(quick_open.documents_state(), DocumentsState::Searching);
    assert_eq!(names(&quick_open), before);
    assert!(quick_open.documents().is_empty());

    quick_open.apply_documents(
        request.walk_generation,
        request.generation,
        Err("index busy".to_owned()),
    );
    assert_eq!(quick_open.documents_state(), DocumentsState::Failed);
    assert_eq!(names(&quick_open), before);
    assert!(quick_open.documents().is_empty());
    assert_eq!(
        quick_open.selected_path(),
        Some(PathBuf::from("/r/rechnung.txt"))
    );
}

#[test]
fn arriving_documents_leave_the_name_hits_and_their_cursor_alone() {
    let mut quick_open = with_documents_on(&["rechnung.txt"], "rechnung");
    let before = quick_open.hits().to_vec();
    let request = quick_open.request_documents().unwrap();
    quick_open.apply_documents(
        request.walk_generation,
        request.generation,
        Ok(vec![document("/d/a.pdf"), document("/d/b.pdf")]),
    );
    assert_eq!(quick_open.hits(), before);
    assert_eq!(quick_open.cursor(), 0);
    assert_eq!(quick_open.documents().len(), 2);
}

#[test]
fn the_cursor_runs_from_the_last_name_hit_into_the_documents_and_back() {
    let mut quick_open = with_documents_on(&["a1.txt", "a2.txt"], "a");
    let request = quick_open.request_documents().unwrap();
    quick_open.apply_documents(
        request.walk_generation,
        request.generation,
        Ok(vec![document("/d/one.pdf"), document("/d/two.pdf")]),
    );
    let names_count = quick_open.hits().len();
    assert_eq!(names_count, 2);
    quick_open.move_cursor(1);
    assert_eq!(
        quick_open.selected_path(),
        Some(PathBuf::from("/r").join(names(&quick_open)[1].as_str()))
    );
    quick_open.move_cursor(1);
    assert_eq!(
        quick_open.selected_path(),
        Some(PathBuf::from("/d/one.pdf"))
    );
    quick_open.move_cursor(1);
    assert_eq!(
        quick_open.selected_path(),
        Some(PathBuf::from("/d/two.pdf"))
    );
    // Below the documents stands the row that opens the search page.
    quick_open.move_cursor(5);
    assert!(quick_open.all_hits_selected());
    quick_open.move_cursor(-3);
    assert_eq!(quick_open.cursor(), 1);
}

#[test]
fn the_last_row_opens_all_hits_and_the_cursor_reaches_it() {
    let mut quick_open = with_documents_on(&["a1.txt", "a2.txt"], "a");
    assert_eq!(quick_open.all_hits_row(), None);
    let request = quick_open.request_documents().unwrap();
    quick_open.apply_documents(
        request.walk_generation,
        request.generation,
        Ok(vec![document("/d/one.pdf")]),
    );

    assert_eq!(quick_open.all_hits_row(), Some(3));
    quick_open.move_cursor(10);
    assert_eq!(quick_open.cursor(), 3);
    assert!(quick_open.all_hits_selected());
    assert_eq!(quick_open.selected_path(), None);
    quick_open.move_cursor(-1);
    assert!(!quick_open.all_hits_selected());
    quick_open.select(3);
    assert!(quick_open.all_hits_selected());
}

#[test]
fn there_is_no_last_row_without_documents_or_with_the_section_off() {
    let mut off = QuickOpen::new(PathBuf::from("/r"), 1);
    off.apply_events(file_events("/r", &["a.txt"]));
    type_and_rank(&mut off, "a");
    assert_eq!(off.all_hits_row(), None);

    let mut empty = with_documents_on(&["a.txt"], "a");
    let request = empty.request_documents().unwrap();
    empty.apply_documents(request.walk_generation, request.generation, Ok(Vec::new()));
    assert_eq!(empty.all_hits_row(), None);
}

#[test]
fn at_most_five_documents_are_kept() {
    let mut quick_open = with_documents_on(&["a.txt"], "a");
    let request = quick_open.request_documents().unwrap();
    let many = (0..9).map(|i| document(&format!("/d/{i}.pdf"))).collect();
    quick_open.apply_documents(request.walk_generation, request.generation, Ok(many));
    assert_eq!(quick_open.documents().len(), MAX_DOCUMENT_HITS);
}

#[test]
fn an_answer_for_older_text_is_dropped() {
    let mut quick_open = with_documents_on(&["a.txt"], "a");
    let old = quick_open.request_documents().unwrap();
    type_and_rank(&mut quick_open, "ab");
    let new = quick_open.request_documents().unwrap();
    quick_open.apply_documents(
        old.walk_generation,
        old.generation,
        Ok(vec![document("/d/old.pdf")]),
    );
    assert!(quick_open.documents().is_empty());
    assert_eq!(quick_open.documents_state(), DocumentsState::Searching);
    quick_open.apply_documents(
        new.walk_generation,
        new.generation,
        Ok(vec![document("/d/new.pdf")]),
    );
    assert_eq!(quick_open.documents().len(), 1);
}

#[test]
fn without_documents_enabled_nothing_is_requested() {
    let mut quick_open = open("/r", &["a.txt"]);
    type_and_rank(&mut quick_open, "a");
    assert_eq!(quick_open.request_documents(), None);
    assert_eq!(quick_open.documents_state(), DocumentsState::Idle);
}

#[test]
fn an_empty_text_requests_nothing_and_drops_old_documents() {
    let mut quick_open = with_documents_on(&["a.txt"], "a");
    let request = quick_open.request_documents().unwrap();
    quick_open.apply_documents(
        request.walk_generation,
        request.generation,
        Ok(vec![document("/d/a.pdf")]),
    );
    assert_eq!(quick_open.set_query("  ".to_owned()).map(|_| ()), None);
    assert_eq!(quick_open.request_documents(), None);
    assert!(quick_open.documents().is_empty());
}

/// Run with `cargo test --release quick_open_timing -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check"]
fn quick_open_timing_with_the_file_limit() {
    let names: Vec<Box<str>> = (0..QUICK_OPEN_FILE_LIMIT)
        .map(|i| format!("crate{}/src/module{}/file{i}.rs", i % 97, i % 1013).into())
        .collect();
    let chunks: Vec<Chunk> = names.chunks(256).map(|part| part.to_vec().into()).collect();
    let started = std::time::Instant::now();
    let hits = ranked("a", &chunks, 0, &never);
    println!(
        "ranked {} paths for 'a' in {} ms",
        QUICK_OPEN_FILE_LIMIT,
        started.elapsed().as_millis()
    );
    assert_eq!(hits.len(), MAX_HITS);
}
