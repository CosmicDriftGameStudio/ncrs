//! Recursive file-name and file-content search, the way VS Code does it.
//! No UI: events go out through a bounded channel and the caller renders them.

use std::fmt;
use std::fs::{File, Metadata};
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use grep_matcher::{LineTerminator, Matcher as _};
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkMatch};
use ignore::{WalkBuilder, WalkState};
use tokio::sync::mpsc;
use unicode_normalization::UnicodeNormalization as _;

/// Line matches over all files; the search stops and reports `truncated` after.
const MATCH_LIMIT: usize = 20_000;
const CHANNEL_CAPACITY: usize = 256;
const PREVIEW_CHARS: usize = 250;
/// Characters kept in front of the first match when a long line is cut.
const PREVIEW_LEAD_CHARS: usize = 40;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
const PROGRESS_FILES: usize = 500;

#[cfg(any(windows, test))]
const WINDOWS_ATTRIBUTE_OFFLINE: u32 = 0x1000;
#[cfg(any(windows, test))]
const WINDOWS_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x4_0000;
#[cfg(any(windows, test))]
const WINDOWS_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
/// `SF_DATALESS` from macOS `sys/stat.h`.
#[cfg(any(target_os = "macos", test))]
const MACOS_FLAG_DATALESS: u32 = 0x4000_0000;

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub root: PathBuf,
    pub text: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
    /// `;`-separated globs; empty means every file.
    pub include: String,
    /// `;`-separated globs; a trailing `/` excludes a directory with its content.
    pub exclude: String,
    pub same_file_system: bool,
}

impl SearchQuery {
    #[cfg(test)]
    pub fn new(root: PathBuf, text: impl Into<String>) -> Self {
        Self {
            root,
            text: text.into(),
            regex: false,
            case_sensitive: false,
            whole_word: false,
            include: String::new(),
            exclude: String::new(),
            same_file_system: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchError {
    /// Neither text nor an include pattern: it would list everything.
    Empty,
    Glob {
        pattern: String,
        reason: String,
    },
    Regex(String),
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "nothing to search for"),
            Self::Glob { pattern, reason } => write!(f, "invalid pattern '{pattern}': {reason}"),
            Self::Regex(reason) => write!(f, "invalid regular expression: {reason}"),
        }
    }
}

impl std::error::Error for SearchError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineMatch {
    pub number: u64,
    pub text: String,
    /// Byte ranges into `text`, as stored (after lossy conversion and cutting).
    pub ranges: Vec<Range<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchEvent {
    /// `lines` is empty for a pure name search.
    File {
        path: PathBuf,
        lines: Vec<LineMatch>,
        /// Read where the walk visits the file, so no row of the list has to.
        size: u64,
        modified: Option<SystemTime>,
    },
    Progress {
        scanned: usize,
        skipped: usize,
    },
    Finished(SearchTotals),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchTotals {
    pub scanned: usize,
    pub files: usize,
    pub matches: usize,
    pub skipped: usize,
    pub truncated: bool,
}

/// Which paths below `root` the include and exclude globs let through.
#[derive(Debug)]
pub struct PathFilter {
    root: PathBuf,
    include: Option<GlobSet>,
    exclude: GlobSet,
    exclude_dirs: GlobSet,
    case_sensitive: bool,
}

impl PathFilter {
    /// Whether the walk goes into the entry at `path`.
    fn walks_into(&self, path: &Path, is_dir: bool) -> bool {
        let relative = comparable_path(&self.root, path, self.case_sensitive);
        !self.exclude.is_match(&relative) && !(is_dir && self.exclude_dirs.is_match(&relative))
    }

    /// Whether the include globs take the file at `path`; all files when there
    /// are none.
    fn takes(&self, path: &Path) -> bool {
        self.include.as_ref().is_none_or(|include| {
            include.is_match(comparable_path(&self.root, path, self.case_sensitive))
        })
    }

    /// Whether a file the walk would have listed is `path`: no folder on the
    /// way to it and not the file itself is excluded, and the include globs
    /// take it.
    pub fn admits_file(&self, path: &Path) -> bool {
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return false;
        };
        // `strip_prefix` compares components, so `root/../x` would pass it.
        if !relative
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
        {
            return false;
        }
        let mut walked = self.root.clone();
        let mut parts = relative.components().peekable();
        while let Some(part) = parts.next() {
            walked.push(part);
            if !self.walks_into(&walked, parts.peek().is_some()) {
                return false;
            }
        }
        self.takes(path)
    }
}

#[derive(Debug)]
pub struct CompiledSearch {
    root: PathBuf,
    paths: PathFilter,
    content: Option<RegexMatcher>,
    same_file_system: bool,
    match_limit: usize,
}

struct Pattern {
    glob: String,
    dirs_only: bool,
}

/// On Windows `\` separates directories, as it does in the paths users paste;
/// elsewhere it stays what a glob makes of it, an escape.
fn normalize_separators(spec: &str, backslash_is_separator: bool) -> String {
    if backslash_is_separator {
        spec.replace('\\', "/")
    } else {
        spec.to_owned()
    }
}

/// VS Code semantics: a pattern without `/` applies to the name at any depth.
fn parse_patterns(spec: &str, include: bool, backslash_is_separator: bool) -> Vec<Pattern> {
    let normalized = normalize_separators(spec, backslash_is_separator);
    normalized
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .filter_map(|part| {
            let dirs_only = part.ends_with('/');
            let body = part.trim_end_matches('/');
            if body.is_empty() {
                return None;
            }
            // Norton style: `*.*` is every name, dot or not.
            let name = if body == "*.*" { "*" } else { body };
            let anchored = if name.contains('/') {
                name.to_owned()
            } else {
                format!("**/{name}")
            };
            let glob = if include && dirs_only {
                format!("{anchored}/**")
            } else {
                anchored
            };
            Some(Pattern {
                glob,
                dirs_only: dirs_only && !include,
            })
        })
        .collect()
}

fn build_glob_set<'a>(
    patterns: impl Iterator<Item = &'a Pattern>,
    case_sensitive: bool,
) -> Result<GlobSet, SearchError> {
    let mut set = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = GlobBuilder::new(&fold(&pattern.glob, case_sensitive))
            .literal_separator(true)
            .build()
            .map_err(|error| SearchError::Glob {
                pattern: pattern.glob.clone(),
                reason: error.kind().to_string(),
            })?;
        set.add(glob);
    }
    set.build().map_err(|error| SearchError::Glob {
        pattern: String::new(),
        reason: error.kind().to_string(),
    })
}

impl PathFilter {
    /// The filter of the file names and the excluded paths of `query`, to
    /// apply to paths that did not come from the walk.
    pub fn compile(query: &SearchQuery) -> Result<Self, SearchError> {
        let include = parse_patterns(&query.include, true, cfg!(windows));
        let exclude = parse_patterns(&query.exclude, false, cfg!(windows));
        Ok(Self {
            root: query.root.clone(),
            include: if include.is_empty() {
                None
            } else {
                Some(build_glob_set(include.iter(), query.case_sensitive)?)
            },
            exclude: build_glob_set(
                exclude.iter().filter(|pattern| !pattern.dirs_only),
                query.case_sensitive,
            )?,
            exclude_dirs: build_glob_set(
                exclude.iter().filter(|pattern| pattern.dirs_only),
                query.case_sensitive,
            )?,
            case_sensitive: query.case_sensitive,
        })
    }
}

pub fn compile(query: &SearchQuery) -> Result<CompiledSearch, SearchError> {
    if query.text.is_empty() && parse_patterns(&query.include, true, cfg!(windows)).is_empty() {
        return Err(SearchError::Empty);
    }
    let content = if query.text.is_empty() {
        None
    } else {
        let text: String = query.text.nfc().collect();
        let matcher = RegexMatcherBuilder::new()
            .fixed_strings(!query.regex)
            .case_insensitive(!query.case_sensitive)
            .word(query.whole_word)
            .crlf(true)
            .build(&text)
            .map_err(|error| SearchError::Regex(error.to_string()))?;
        Some(matcher)
    };

    Ok(CompiledSearch {
        root: query.root.clone(),
        paths: PathFilter::compile(query)?,
        content,
        same_file_system: query.same_file_system,
        match_limit: MATCH_LIMIT,
    })
}

/// Every file below `root`, hidden ones included, as a name search without a
/// pattern. Stops and reports `truncated` after `limit` files.
pub fn all_files(root: PathBuf, same_file_system: bool, limit: usize) -> CompiledSearch {
    CompiledSearch {
        paths: PathFilter {
            root: root.clone(),
            include: None,
            exclude: GlobSet::empty(),
            exclude_dirs: GlobSet::empty(),
            case_sensitive: true,
        },
        root,
        content: None,
        same_file_system,
        match_limit: limit,
    }
}

/// globset folds case for ASCII only, so `*ä*` would miss `Ä`. Both sides are
/// lowercased here instead and the glob itself stays case-sensitive.
fn fold(text: &str, case_sensitive: bool) -> String {
    let composed: String = text.nfc().collect();
    if case_sensitive {
        composed
    } else {
        composed.to_lowercase()
    }
}

/// Relative path with `/`, NFC. Only for comparing — never put into an event.
/// A name that is not UTF-8 is compared in its lossy form.
fn comparable_path(root: &Path, path: &Path, case_sensitive: bool) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let joined = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    fold(&joined, case_sensitive)
}

#[cfg(any(windows, test))]
pub fn windows_attributes_are_cloud_placeholder(attributes: u32) -> bool {
    attributes
        & (WINDOWS_ATTRIBUTE_OFFLINE
            | WINDOWS_ATTRIBUTE_RECALL_ON_OPEN
            | WINDOWS_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
        != 0
}

#[cfg(any(target_os = "macos", test))]
pub fn macos_flags_are_cloud_placeholder(st_flags: u32) -> bool {
    st_flags & MACOS_FLAG_DATALESS != 0
}

/// Reading such a file would download it, so its content is left alone.
#[cfg(windows)]
fn is_cloud_placeholder(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    windows_attributes_are_cloud_placeholder(metadata.file_attributes())
}

#[cfg(target_os = "macos")]
fn is_cloud_placeholder(metadata: &Metadata) -> bool {
    use std::os::macos::fs::MetadataExt as _;
    macos_flags_are_cloud_placeholder(metadata.st_flags())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn is_cloud_placeholder(_metadata: &Metadata) -> bool {
    false
}

struct Segment {
    original: usize,
    lossy: usize,
    valid: bool,
}

/// Lossy conversion plus the offsets of every run, because U+FFFD (3 bytes)
/// replaces invalid runs of any length and shifts everything behind it.
fn lossy_with_segments(bytes: &[u8]) -> (String, Vec<Segment>) {
    let mut text = String::with_capacity(bytes.len());
    let mut segments = Vec::new();
    let mut original = 0;
    for chunk in bytes.utf8_chunks() {
        let valid = chunk.valid();
        if !valid.is_empty() {
            segments.push(Segment {
                original,
                lossy: text.len(),
                valid: true,
            });
            text.push_str(valid);
            original += valid.len();
        }
        let invalid = chunk.invalid();
        if !invalid.is_empty() {
            segments.push(Segment {
                original,
                lossy: text.len(),
                valid: false,
            });
            text.push(char::REPLACEMENT_CHARACTER);
            original += invalid.len();
        }
    }
    (text, segments)
}

fn lossy_offset(segments: &[Segment], offset: usize, is_end: bool) -> usize {
    let Some(segment) = segments.iter().rev().find(|s| s.original <= offset) else {
        return 0;
    };
    if segment.valid {
        segment.lossy + (offset - segment.original)
    } else if is_end && offset > segment.original {
        segment.lossy + char::REPLACEMENT_CHARACTER.len_utf8()
    } else {
        segment.lossy
    }
}

/// Builds the stored line. `byte_ranges` are offsets into `line` as read from
/// disk; the result's ranges refer to the stored `text`, which differs after the
/// lossy conversion and the cut to a preview.
fn line_match(number: u64, line: &[u8], byte_ranges: &[Range<usize>]) -> LineMatch {
    let (text, segments) = lossy_with_segments(line);
    let mut ranges: Vec<Range<usize>> = byte_ranges
        .iter()
        .map(|r| lossy_offset(&segments, r.start, false)..lossy_offset(&segments, r.end, true))
        .collect();

    if text.chars().count() <= PREVIEW_CHARS {
        return LineMatch {
            number,
            text,
            ranges,
        };
    }

    let first_start = ranges.first().map_or(0, |r| r.start);
    let first_char = text
        .get(..first_start)
        .map_or(0, |before| before.chars().count());
    let window_start_char = first_char.saturating_sub(PREVIEW_LEAD_CHARS);
    let mut boundaries = text
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(text.len()));
    let window_start = boundaries.nth(window_start_char).unwrap_or(0);
    let window_end = boundaries.nth(PREVIEW_CHARS - 1).unwrap_or(text.len());

    ranges.retain(|r| r.end > window_start && r.start < window_end);
    for r in &mut ranges {
        r.start = r.start.clamp(window_start, window_end) - window_start;
        r.end = r.end.clamp(window_start, window_end) - window_start;
    }
    LineMatch {
        number,
        text: text.get(window_start..window_end).unwrap_or("").to_owned(),
        ranges,
    }
}

struct Shared {
    scanned: AtomicUsize,
    skipped: AtomicUsize,
    files: AtomicUsize,
    matches: AtomicUsize,
    truncated: AtomicBool,
}

/// Sink callbacks only run on matches, so a big file without any would keep the
/// walk busy after the receiver is gone. Failing the read stops it; the error
/// is not `Interrupted`, which readers retry.
struct CancelWhenClosed<'a, R> {
    inner: R,
    sender: &'a mpsc::Sender<SearchEvent>,
}

impl<R: io::Read> io::Read for CancelWhenClosed<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.sender.is_closed() {
            return Err(io::Error::other("search cancelled"));
        }
        self.inner.read(buf)
    }
}

struct LineSink<'a> {
    matcher: &'a RegexMatcher,
    shared: &'a Shared,
    limit: usize,
    sender: &'a mpsc::Sender<SearchEvent>,
    lines: Vec<LineMatch>,
}

impl Sink for LineSink<'_> {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, found: &SinkMatch<'_>) -> Result<bool, io::Error> {
        // Cancels a large file with matches; `CancelWhenClosed` covers one without.
        if self.sender.is_closed() {
            return Ok(false);
        }
        if self.shared.matches.fetch_add(1, Ordering::Relaxed) >= self.limit {
            self.shared.truncated.store(true, Ordering::Relaxed);
            return Ok(false);
        }
        let bytes = found.bytes();
        let without_newline = bytes.strip_suffix(b"\n").unwrap_or(bytes);
        let line = without_newline
            .strip_suffix(b"\r")
            .unwrap_or(without_newline);
        let mut ranges = Vec::new();
        let _ = self.matcher.find_iter(line, |m| {
            ranges.push(m.start()..m.end());
            true
        });
        self.lines
            .push(line_match(found.line_number().unwrap_or(0), line, &ranges));
        Ok(true)
    }
}

struct Throttle {
    last: Instant,
    since: usize,
}

impl Throttle {
    fn due(&mut self) -> bool {
        self.since += 1;
        if self.since >= PROGRESS_FILES || self.last.elapsed() >= PROGRESS_INTERVAL {
            self.since = 0;
            self.last = Instant::now();
            true
        } else {
            false
        }
    }
}

/// Blocks until the walk is done or the receiver is gone. Results arrive on the
/// channel from the walker's worker threads, in no particular order.
pub fn run_blocking(compiled: CompiledSearch, sender: mpsc::Sender<SearchEvent>) -> SearchTotals {
    let search = Arc::new(compiled);
    let shared = Arc::new(Shared {
        scanned: AtomicUsize::new(0),
        skipped: AtomicUsize::new(0),
        files: AtomicUsize::new(0),
        matches: AtomicUsize::new(0),
        truncated: AtomicBool::new(false),
    });

    let filter_search = Arc::clone(&search);
    let walker = WalkBuilder::new(&search.root)
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .follow_links(false)
        .same_file_system(search.same_file_system)
        .filter_entry(move |entry| {
            if entry.depth() == 0 {
                return true;
            }
            let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
            filter_search.paths.walks_into(entry.path(), is_dir)
        })
        .build_parallel();

    walker.run(|| {
        let worker_search = Arc::clone(&search);
        let worker_shared = Arc::clone(&shared);
        let worker_sender = sender.clone();
        let mut throttle = Throttle {
            last: Instant::now(),
            since: 0,
        };
        let mut searcher = SearcherBuilder::new()
            .line_number(true)
            .line_terminator(LineTerminator::crlf())
            .binary_detection(BinaryDetection::quit(0))
            .bom_sniffing(true)
            .build();

        Box::new(move |result| {
            if worker_sender.is_closed() || worker_shared.truncated.load(Ordering::Relaxed) {
                return WalkState::Quit;
            }
            let Ok(entry) = result else {
                worker_shared.skipped.fetch_add(1, Ordering::Relaxed);
                return WalkState::Continue;
            };
            if !entry.file_type().is_some_and(|t| !t.is_dir()) {
                return WalkState::Continue;
            }

            let scanned = worker_shared.scanned.fetch_add(1, Ordering::Relaxed) + 1;
            if throttle.due() {
                let progress = SearchEvent::Progress {
                    scanned,
                    skipped: worker_shared.skipped.load(Ordering::Relaxed),
                };
                if worker_sender.blocking_send(progress).is_err() {
                    return WalkState::Quit;
                }
            }

            if !worker_search.paths.takes(entry.path()) {
                return WalkState::Continue;
            }

            let lines = match &worker_search.content {
                None => {
                    if worker_shared.matches.fetch_add(1, Ordering::Relaxed)
                        >= worker_search.match_limit
                    {
                        worker_shared.truncated.store(true, Ordering::Relaxed);
                        return WalkState::Quit;
                    }
                    Vec::new()
                }
                Some(matcher) => {
                    match entry.metadata() {
                        // Links report their own metadata (links are not followed), so a
                        // link is listed by name but its content is never read; the same
                        // check keeps a FIFO or device from blocking the read.
                        Ok(metadata) if !metadata.is_file() => return WalkState::Continue,
                        Ok(metadata) if !is_cloud_placeholder(&metadata) => {}
                        _ => {
                            worker_shared.skipped.fetch_add(1, Ordering::Relaxed);
                            return WalkState::Continue;
                        }
                    }
                    let mut sink = LineSink {
                        matcher,
                        shared: &worker_shared,
                        limit: worker_search.match_limit,
                        sender: &worker_sender,
                        lines: Vec::new(),
                    };
                    let outcome = File::open(entry.path()).and_then(|file| {
                        let reader = CancelWhenClosed {
                            inner: file,
                            sender: &worker_sender,
                        };
                        searcher
                            .search_reader(matcher, reader, &mut sink)
                            .map_err(io::Error::other)
                    });
                    if outcome.is_err() && !worker_sender.is_closed() {
                        worker_shared.skipped.fetch_add(1, Ordering::Relaxed);
                    }
                    if sink.lines.is_empty() {
                        return if worker_sender.is_closed() {
                            WalkState::Quit
                        } else {
                            WalkState::Continue
                        };
                    }
                    sink.lines
                }
            };

            worker_shared.files.fetch_add(1, Ordering::Relaxed);
            // The walker's own path: the NFC and lossy forms are for comparing only.
            let metadata = entry.metadata().ok();
            let event = SearchEvent::File {
                path: entry.path().to_path_buf(),
                lines,
                size: metadata.as_ref().map_or(0, Metadata::len),
                modified: metadata.and_then(|stat| stat.modified().ok()),
            };
            if worker_sender.blocking_send(event).is_err() {
                WalkState::Quit
            } else {
                WalkState::Continue
            }
        })
    });

    let totals = SearchTotals {
        scanned: shared.scanned.load(Ordering::Relaxed),
        files: shared.files.load(Ordering::Relaxed),
        matches: shared
            .matches
            .load(Ordering::Relaxed)
            .min(search.match_limit),
        skipped: shared.skipped.load(Ordering::Relaxed),
        truncated: shared.truncated.load(Ordering::Relaxed),
    };
    let _ = sender.blocking_send(SearchEvent::Finished(totals));
    totals
}

/// Runs the search on the blocking pool. Dropping the receiver cancels it.
pub fn spawn(search: CompiledSearch) -> mpsc::Receiver<SearchEvent> {
    let (sender, receiver) = mpsc::channel(CHANNEL_CAPACITY);
    drop(tokio::task::spawn_blocking(move || {
        run_blocking(search, sender);
    }));
    receiver
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
        std::os::unix::fs::symlink(dir.path().join("target.txt"), dir.path().join("link.txt"))
            .unwrap();
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
}
