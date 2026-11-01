use super::*;
use std::fs;

struct Outcome {
    /// (path relative to the root, lines), sorted by path.
    files: Vec<(String, Vec<LineMatch>)>,
    matches: usize,
    skipped: usize,
    truncated: bool,
}

fn run_compiled(search: CompiledSearch) -> Outcome {
    let root = search.root.clone();
    let (sender, mut receiver) = mpsc::channel(100_000);
    let totals = run_blocking(search, sender);
    let mut files = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        match event {
            SearchEvent::File { path, lines, .. } => {
                let relative = path.strip_prefix(&root).unwrap().to_string_lossy();
                files.push((relative.replace('\\', "/"), lines));
            }
            SearchEvent::Finished(_) | SearchEvent::Progress { .. } => {}
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Outcome {
        files,
        matches: totals.matches,
        skipped: totals.skipped,
        truncated: totals.truncated,
    }
}

fn run(query: &SearchQuery) -> Outcome {
    run_compiled(compile(query).unwrap())
}

fn names(outcome: &Outcome) -> Vec<&str> {
    outcome
        .files
        .iter()
        .map(|(name, _)| name.as_str())
        .collect()
}

fn tree() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("ncrs-search-")
        .tempdir()
        .expect("a scratch directory")
}

fn write(dir: &Path, name: &str, content: impl AsRef<[u8]>) {
    let path = dir.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn name_query(dir: &tempfile::TempDir, include: &str) -> SearchQuery {
    SearchQuery {
        include: include.to_owned(),
        ..SearchQuery::new(dir.path().to_path_buf(), "")
    }
}

fn text_query(dir: &tempfile::TempDir, text: &str) -> SearchQuery {
    SearchQuery::new(dir.path().to_path_buf(), text)
}

#[test]
fn a_decomposed_file_name_is_found_by_a_composed_pattern() {
    let dir = tree();
    write(dir.path(), "A\u{308}rger.txt", "x");
    write(dir.path(), "other.txt", "x");
    let outcome = run(&name_query(&dir, "*\u{e4}*"));
    assert_eq!(outcome.files.len(), 1);
    assert!(outcome.files[0].0.ends_with("rger.txt"));
}

#[test]
fn content_search_folds_case_of_umlauts_only_when_asked() {
    let dir = tree();
    write(dir.path(), "a.txt", "Viel \u{c4}RGER hier\n");
    let mut query = text_query(&dir, "\u{e4}rger");
    assert_eq!(run(&query).files.len(), 1);
    query.case_sensitive = true;
    assert!(run(&query).files.is_empty());
}

#[test]
fn star_dot_star_matches_a_name_without_a_dot() {
    let dir = tree();
    write(dir.path(), "Makefile", "x");
    write(dir.path(), "sub/a.rs", "x");
    assert_eq!(
        names(&run(&name_query(&dir, "*.*"))),
        ["Makefile", "sub/a.rs"]
    );
}

#[test]
fn a_path_pattern_only_hits_below_its_directory() {
    let dir = tree();
    write(dir.path(), "src/a.rs", "x");
    write(dir.path(), "src/deep/b.rs", "x");
    write(dir.path(), "docs/c.rs", "x");
    assert_eq!(
        names(&run(&name_query(&dir, "src/**"))),
        ["src/a.rs", "src/deep/b.rs"]
    );
}

#[test]
fn several_patterns_are_separated_by_semicolons() {
    let dir = tree();
    write(dir.path(), "a.rs", "x");
    write(dir.path(), "sub/Cargo.toml", "x");
    write(dir.path(), "c.md", "x");
    assert_eq!(
        names(&run(&name_query(&dir, " *.rs ; *.toml;;"))),
        ["a.rs", "sub/Cargo.toml"]
    );
}

#[test]
fn an_excluded_directory_is_not_entered() {
    let dir = tree();
    write(dir.path(), "target/debug/a.rs", "x");
    write(dir.path(), "src/target.rs", "x");
    write(dir.path(), "b.rs", "x");
    let mut query = name_query(&dir, "*.rs");
    query.exclude = "target/".to_owned();
    assert_eq!(names(&run(&query)), ["b.rs", "src/target.rs"]);
}

#[test]
fn hidden_and_gitignored_files_are_searched() {
    let dir = tree();
    fs::create_dir_all(dir.path().join(".git")).unwrap();
    write(dir.path(), ".gitignore", "secret.txt\nignored/\n");
    write(dir.path(), "secret.txt", "needle");
    write(dir.path(), "ignored/a.txt", "needle");
    write(dir.path(), ".hidden/b.txt", "needle");
    assert_eq!(
        names(&run(&text_query(&dir, "needle"))),
        [".hidden/b.txt", "ignored/a.txt", "secret.txt"]
    );
}

#[test]
fn a_binary_file_is_skipped() {
    let dir = tree();
    write(dir.path(), "bin.dat", b"needle\0needle");
    write(dir.path(), "text.txt", "needle");
    assert_eq!(names(&run(&text_query(&dir, "needle"))), ["text.txt"]);
}

#[test]
fn utf16_with_a_bom_is_searched() {
    let dir = tree();
    let mut bytes = vec![0xFF, 0xFE];
    for unit in "first\nan needle here\n".encode_utf16() {
        bytes.push(u8::try_from(unit & 0xFF).unwrap());
        bytes.push(u8::try_from(unit >> 8).unwrap());
    }
    write(dir.path(), "wide.txt", bytes);
    let outcome = run(&text_query(&dir, "needle"));
    assert_eq!(outcome.files.len(), 1);
    assert_eq!(outcome.files[0].1[0].number, 2);
}

#[test]
fn crlf_lines_lose_the_carriage_return_and_keep_a_correct_range() {
    let dir = tree();
    write(dir.path(), "a.txt", "one\r\nfind me\r\nthree\r\n");
    let outcome = run(&text_query(&dir, "me"));
    let line = &outcome.files[0].1[0];
    assert_eq!(line.text, "find me");
    assert_eq!(line.number, 2);
    assert_eq!(&line.text[line.ranges[0].clone()], "me");
}

#[test]
fn whole_word_rejects_a_match_inside_a_word() {
    let dir = tree();
    write(dir.path(), "a.txt", "category\n");
    write(dir.path(), "b.txt", "a cat sat\n");
    let mut query = text_query(&dir, "cat");
    assert_eq!(run(&query).files.len(), 2);
    query.whole_word = true;
    assert_eq!(names(&run(&query)), ["b.txt"]);
}

#[test]
fn regex_mode_matches_and_a_bad_regex_is_an_error() {
    let dir = tree();
    write(dir.path(), "a.txt", "id 42\n");
    let mut query = text_query(&dir, r"id \d+");
    assert!(run(&query).files.is_empty(), "literal mode must not match");
    query.regex = true;
    let outcome = run(&query);
    let line = &outcome.files[0].1[0];
    assert_eq!(&line.text[line.ranges[0].clone()], "id 42");

    query.text = "(".to_owned();
    let error = compile(&query).unwrap_err();
    assert!(matches!(error, SearchError::Regex(_)));
    assert!(!error.to_string().is_empty());
}

#[test]
fn a_bad_glob_is_an_error_with_a_reason() {
    let dir = tree();
    let error = compile(&name_query(&dir, "[abc")).unwrap_err();
    assert!(matches!(error, SearchError::Glob { .. }));
}

#[test]
fn an_empty_query_is_rejected() {
    let dir = tree();
    assert_eq!(
        compile(&text_query(&dir, "")).unwrap_err(),
        SearchError::Empty
    );
    assert_eq!(
        compile(&name_query(&dir, " ; ")).unwrap_err(),
        SearchError::Empty
    );
}

#[test]
fn a_long_line_is_cut_around_the_match() {
    let dir = tree();
    let line = format!("{}NEEDLE{}", "a".repeat(1000), "b".repeat(1000));
    write(dir.path(), "a.txt", &line);
    let outcome = run(&text_query(&dir, "needle"));
    let found = &outcome.files[0].1[0];
    assert!(found.text.chars().count() <= PREVIEW_CHARS);
    assert_eq!(found.ranges.len(), 1);
    assert_eq!(&found.text[found.ranges[0].clone()], "NEEDLE");
}

#[test]
fn matches_outside_the_preview_are_dropped() {
    let line = format!("hit{}hit", "x".repeat(1000));
    let found = line_match(1, line.as_bytes(), [0..3, 1003..1006].as_slice());
    assert_eq!(found.ranges, vec![0..3]);
}

#[test]
fn ranges_follow_the_text_after_invalid_utf8_became_replacement_characters() {
    let dir = tree();
    write(dir.path(), "a.txt", b"a\xff\xfe\xfdx needle\n");
    let outcome = run(&text_query(&dir, "needle"));
    let found = &outcome.files[0].1[0];
    assert!(found.text.contains('\u{fffd}'));
    assert_eq!(&found.text[found.ranges[0].clone()], "needle");
}

#[test]
fn dropping_the_receiver_ends_a_search_that_never_matches() {
    let dir = tree();
    for folder in 0..10 {
        for file in 0..30 {
            write(dir.path(), &format!("d{folder}/f{file}.txt"), "nothing");
        }
    }
    let search = compile(&text_query(&dir, "zzz-not-there")).unwrap();
    let (sender, receiver) = mpsc::channel(1);
    drop(receiver);
    let totals = std::thread::spawn(move || run_blocking(search, sender))
        .join()
        .unwrap();
    assert!(
        totals.scanned < 30,
        "the walk went on after the receiver was gone: {} of 300 files",
        totals.scanned
    );
}

#[test]
fn the_match_limit_truncates() {
    let dir = tree();
    write(dir.path(), "a.txt", "hit\n".repeat(10));
    write(dir.path(), "b.txt", "hit\n".repeat(10));
    let mut search = compile(&text_query(&dir, "hit")).unwrap();
    search.match_limit = 3;
    let outcome = run_compiled(search);
    assert!(outcome.truncated);
    assert_eq!(outcome.matches, 3);
    let sent: usize = outcome.files.iter().map(|(_, lines)| lines.len()).sum();
    assert_eq!(sent, 3);
}

#[test]
fn a_name_search_counts_against_the_limit_too() {
    let dir = tree();
    for index in 0..10 {
        write(dir.path(), &format!("f{index}.txt"), "x");
    }
    let mut search = compile(&name_query(&dir, "*.txt")).unwrap();
    search.match_limit = 3;
    let outcome = run_compiled(search);
    assert!(outcome.truncated);
    assert_eq!(outcome.files.len(), 3);
}

#[test]
fn backslashes_separate_directories_only_where_asked() {
    let globs = |spec: &str, backslash: bool| -> Vec<(String, bool)> {
        parse_patterns(spec, false, backslash)
            .into_iter()
            .map(|p| (p.glob, p.dirs_only))
            .collect()
    };
    assert_eq!(
        globs(r"src\**;target\", true),
        globs("src/**;target/", false)
    );
    assert_eq!(globs(r"src\**", false), [(r"**/src\**".to_owned(), false)]);
}

#[test]
fn the_reader_fails_once_the_receiver_is_gone() {
    use std::io::Read as _;
    let (sender, receiver) = mpsc::channel(1);
    let mut reader = CancelWhenClosed {
        inner: &b"abc"[..],
        sender: &sender,
    };
    let mut buffer: [u8; 8] = [0; 8];
    assert_eq!(reader.read(&mut buffer).unwrap(), 3);
    drop(receiver);
    let error = reader.read(&mut buffer).unwrap_err();
    assert_ne!(error.kind(), io::ErrorKind::Interrupted);
}

#[test]
fn below_the_limit_nothing_is_truncated() {
    let dir = tree();
    write(dir.path(), "a.txt", "hit\n");
    let outcome = run(&text_query(&dir, "hit"));
    assert!(!outcome.truncated);
    assert_eq!(outcome.matches, 1);
    assert_eq!(outcome.skipped, 0);
}

#[test]
fn all_files_lists_hidden_files_and_stops_at_the_limit() {
    let dir = tree();
    write(dir.path(), ".hidden/a.txt", "x");
    write(dir.path(), "src/config.rs", "x");
    write(dir.path(), "b.txt", "x");
    let outcome = run_compiled(all_files(dir.path().to_path_buf(), true, 100));
    assert_eq!(names(&outcome), [".hidden/a.txt", "b.txt", "src/config.rs"]);
    assert!(!outcome.truncated);

    let limited = run_compiled(all_files(dir.path().to_path_buf(), true, 2));
    assert!(limited.truncated);
    assert_eq!(limited.files.len(), 2);
}

#[cfg(unix)]
#[test]
fn all_files_does_not_follow_a_symlinked_directory() {
    let dir = tree();
    write(dir.path(), "real/a.txt", "x");
    std::os::unix::fs::symlink(dir.path().join("real"), dir.path().join("link")).unwrap();
    let outcome = run_compiled(all_files(dir.path().to_path_buf(), true, 100));
    assert_eq!(names(&outcome), ["link", "real/a.txt"]);
}

#[tokio::test]
async fn spawn_delivers_events_and_a_finish() {
    let dir = tree();
    write(dir.path(), "a.txt", "hit\n");
    let mut receiver = spawn(compile(&text_query(&dir, "hit")).unwrap());
    let mut found = 0;
    let mut finished = false;
    while let Some(event) = receiver.recv().await {
        match event {
            SearchEvent::File { .. } => found += 1,
            SearchEvent::Finished(totals) => {
                assert_eq!(totals.files, 1);
                finished = true;
            }
            SearchEvent::Progress { .. } => {}
        }
    }
    assert_eq!(found, 1);
    assert!(finished);
}

#[cfg(unix)]
#[test]
fn a_root_that_is_a_symlink_to_a_directory_is_searched() {
    let dir = tree();
    write(dir.path(), "real/a.txt", "hit");
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(dir.path().join("real"), &link).unwrap();
    let outcome = run(&SearchQuery::new(link, "hit"));
    assert_eq!(names(&outcome), ["a.txt"]);
}

#[cfg(unix)]
#[test]
fn a_symlink_to_a_file_is_listed_by_name_but_not_read() {
    let dir = tree();
    write(dir.path(), "target.txt", "hit");
    std::os::unix::fs::symlink(dir.path().join("target.txt"), dir.path().join("link.txt")).unwrap();
    assert_eq!(
        names(&run(&name_query(&dir, "*.txt"))),
        ["link.txt", "target.txt"]
    );
    assert_eq!(names(&run(&text_query(&dir, "hit"))), ["target.txt"]);
}

#[test]
fn a_found_file_carries_its_size_and_date_from_the_walk() {
    let dir = tree();
    write(dir.path(), "sized.txt", "twelve bytes");
    let (sender, mut receiver) = mpsc::channel(100);
    let _ = run_blocking(compile(&name_query(&dir, "sized.txt")).unwrap(), sender);
    let Some(SearchEvent::File { size, modified, .. }) = receiver.try_recv().ok() else {
        panic!("no file found");
    };
    assert_eq!(size, 12);
    assert!(modified.is_some());
}

#[cfg(unix)]
#[test]
fn a_found_symlink_carries_the_size_of_its_target() {
    let dir = tree();
    write(dir.path(), "target.txt", "hit");
    std::os::unix::fs::symlink(dir.path().join("target.txt"), dir.path().join("link.txt")).unwrap();
    let (sender, mut receiver) = mpsc::channel(100);
    let _ = run_blocking(compile(&name_query(&dir, "link.txt")).unwrap(), sender);
    let Some(SearchEvent::File { size, .. }) = receiver.try_recv().ok() else {
        panic!("no file found");
    };
    assert_eq!(size, 3);
}

#[cfg(target_os = "linux")]
#[test]
fn a_name_that_is_not_utf8_is_found_by_its_lossy_form() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;
    let dir = tree();
    fs::write(dir.path().join(OsStr::from_bytes(b"caf\xe9.txt")), "x").unwrap();
    let (sender, mut receiver) = mpsc::channel(100);
    let _ = run_blocking(compile(&name_query(&dir, "caf*.txt")).unwrap(), sender);
    let Some(SearchEvent::File { path, .. }) = receiver.try_recv().ok() else {
        panic!("no file found");
    };
    assert_eq!(path.file_name().unwrap().as_bytes(), b"caf\xe9.txt");
}

#[test]
fn windows_placeholder_attributes() {
    assert!(windows_attributes_are_cloud_placeholder(0x1000));
    assert!(windows_attributes_are_cloud_placeholder(0x4_0000 | 0x20));
    assert!(windows_attributes_are_cloud_placeholder(0x40_0000));
    assert!(!windows_attributes_are_cloud_placeholder(0x20 | 0x2));
    assert!(!windows_attributes_are_cloud_placeholder(0));
}

#[test]
fn macos_dataless_flag() {
    assert!(macos_flags_are_cloud_placeholder(0x4000_0000));
    assert!(macos_flags_are_cloud_placeholder(0x4000_0000 | 0x2));
    assert!(!macos_flags_are_cloud_placeholder(0x2));
    assert!(!macos_flags_are_cloud_placeholder(0));
}

fn paths_of(include: &str, exclude: &str) -> PathFilter {
    let mut query = SearchQuery::new(PathBuf::from("/data"), "");
    query.include = include.to_owned();
    query.exclude = exclude.to_owned();
    PathFilter::compile(&query).unwrap()
}

#[test]
fn the_path_filter_admits_only_files_below_its_root() {
    let paths = paths_of("", "");

    assert!(paths.admits_file(Path::new("/data/a/b.pdf")));
    assert!(!paths.admits_file(Path::new("/other/b.pdf")));
    assert!(!paths.admits_file(Path::new("/database/b.pdf")));
    assert!(!paths.admits_file(Path::new("/data/../other/b.pdf")));
    assert!(!paths.admits_file(Path::new("/data/a/../../other/b.pdf")));
}

#[test]
fn the_path_filter_takes_the_include_globs_and_leaves_the_excluded_out() {
    let paths = paths_of("*.pdf", "*-old.pdf");

    assert!(paths.admits_file(Path::new("/data/a/b.pdf")));
    assert!(!paths.admits_file(Path::new("/data/a/b.txt")));
    assert!(!paths.admits_file(Path::new("/data/a/b-old.pdf")));
}

#[test]
fn the_path_filter_leaves_out_what_is_below_an_excluded_folder() {
    let paths = paths_of("", "target/");

    assert!(paths.admits_file(Path::new("/data/src/a.rs")));
    assert!(!paths.admits_file(Path::new("/data/target/debug/a.rs")));
}

#[test]
fn the_path_filter_reports_a_bad_pattern() {
    let mut query = SearchQuery::new(PathBuf::from("/data"), "");
    query.include = "[".to_owned();

    assert!(matches!(
        PathFilter::compile(&query),
        Err(SearchError::Glob { .. })
    ));
}
