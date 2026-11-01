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
    #[cfg(any(test, feature = "test-support"))]
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
            // Through links, like the panel's rows, else a link shows its own size.
            let metadata = std::fs::metadata(entry.path())
                .or_else(|_| entry.metadata())
                .ok();
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
mod tests;
