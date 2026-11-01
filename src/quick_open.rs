//! Quick Open (Cmd/Ctrl+P): fuzzy search for a file name below a directory.
//! No widgets: the file walk feeds [`QuickOpen::apply_events`], the typed text
//! goes to [`QuickOpen::set_query`], and the view reads the hits.

use std::cmp::Ordering;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{self, AtomicU64};
use std::sync::Arc;

use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::filing::FoundDocument;
use crate::fs::search::SearchEvent;
use crate::messages::{Message, QuickOpenMessage};

/// Hits listed under the field.
pub const MAX_HITS: usize = 12;

/// Document hits listed below the name hits.
pub const MAX_DOCUMENT_HITS: usize = 5;

/// Files listed before the walk gives up. A repository checkout with `target/`
/// already has about 90 000. The walk runs in parallel and in no order by
/// depth, so the limit has to sit well above any tree worth searching, or the
/// files that are cut off are arbitrary ones. Only the path relative to the
/// root is kept.
pub const QUICK_OPEN_FILE_LIMIT: usize = 250_000;

/// Items scored between two looks at whether the query has changed.
const CANCEL_CHECK_INTERVAL: usize = 4096;

/// The relative paths of one batch of the walk.
pub type Chunk = Arc<[Box<str>]>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub chunk: usize,
    pub index: usize,
    pub score: u32,
    /// Byte ranges into the path, consecutive matched characters merged.
    pub highlight: Vec<Range<usize>>,
}

/// Where the full-text search for the typed text stands. Only the documents
/// section depends on it; the name hits never wait for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentsState {
    Idle,
    Searching,
    /// No index, or the search failed: the section stays away, no error shown.
    Failed,
}

/// What the app needs to start the full-text search for the typed text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentRequest {
    pub walk_generation: u64,
    pub generation: u64,
    pub words: String,
}

#[derive(Debug)]
pub struct QuickOpen {
    root: PathBuf,
    query: String,
    hits: Vec<Hit>,
    cursor: usize,
    walking: bool,
    truncated: bool,
    file_count: usize,
    /// The walk these events belong to; a batch of another walk is dropped.
    walk_generation: u64,
    candidates: Vec<Chunk>,
    /// Counts the queries; a ranking of an older one is dropped.
    query_generation: u64,
    /// The generation the background ranking works for, shared so it can see a
    /// newer query and stop.
    current_generation: Arc<AtomicU64>,
    /// Chunks that `hits` already accounts for.
    scored_chunks: usize,
    /// The query generation whose background ranking has not come back yet.
    pending_rescore: Option<u64>,
    /// Full-text hits, listed after the name hits; the cursor runs on from the
    /// last name hit into them.
    documents: Vec<FoundDocument>,
    /// Counts the full-text searches; an answer of an older one is dropped.
    documents_generation: u64,
    documents_state: DocumentsState,
    /// Off without Hermes or an index: then there is no documents section.
    documents_enabled: bool,
}

/// A full ranking over a snapshot of the candidates, for a worker thread.
#[derive(Debug)]
pub struct Rescore {
    walk_generation: u64,
    generation: u64,
    query: String,
    chunks: Vec<Chunk>,
    current_generation: Arc<AtomicU64>,
}

impl Rescore {
    /// A ranking that was cancelled reports nothing scored.
    pub fn run(self) -> QuickOpenMessage {
        let current = &self.current_generation;
        let generation = self.generation;
        let cancelled = || current.load(atomic::Ordering::Relaxed) != generation;
        let (hits, chunks_scored) = match rank(&self.query, &self.chunks, 0, &cancelled) {
            Some(hits) => (hits, self.chunks.len()),
            None => (Vec::new(), 0),
        };
        QuickOpenMessage::Ranked {
            walk_generation: self.walk_generation,
            generation,
            hits,
            chunks_scored,
        }
    }

    /// On the blocking pool. Should that fail, an empty ranking comes back and
    /// the receiver ranks everything itself.
    pub async fn run_in_background(self) -> Message {
        let (walk_generation, generation) = (self.walk_generation, self.generation);
        let ranked = tokio::task::spawn_blocking(move || self.run())
            .await
            .unwrap_or(QuickOpenMessage::Ranked {
                walk_generation,
                generation,
                hits: Vec::new(),
                chunks_scored: 0,
            });
        Message::QuickOpen(ranked)
    }
}

pub fn pattern_for(query: &str) -> Pattern {
    // `new`, not `parse`: `!`, `^`, `'` and `$` are file name characters here.
    Pattern::new(
        query,
        CaseMatching::Smart,
        Normalization::Smart,
        AtomKind::Fuzzy,
    )
}

fn path_of(chunks: &[Chunk], chunk: usize, index: usize) -> &str {
    chunks
        .get(chunk)
        .and_then(|paths| paths.get(index))
        .map_or("", |path| &**path)
}

/// Best first: score, then the shorter path, then alphabetical, so equal
/// scores never reorder between runs.
fn order(chunks: &[Chunk], first: &Hit, second: &Hit) -> Ordering {
    let first_path = path_of(chunks, first.chunk, first.index);
    let second_path = path_of(chunks, second.chunk, second.index);
    second
        .score
        .cmp(&first.score)
        .then_with(|| first_path.len().cmp(&second_path.len()))
        .then_with(|| first_path.cmp(second_path))
        .then_with(|| (first.chunk, first.index).cmp(&(second.chunk, second.index)))
}

/// Byte ranges of the characters at `char_positions`.
fn byte_ranges(text: &str, char_positions: &[u32]) -> Vec<Range<usize>> {
    let mut positions: Vec<u32> = char_positions.to_vec();
    positions.sort_unstable();
    positions.dedup();
    let mut wanted = positions.into_iter().peekable();
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for (position, (start, character)) in text.char_indices().enumerate() {
        let Some(&next) = wanted.peek() else {
            break;
        };
        if usize::try_from(next) != Ok(position) {
            continue;
        }
        wanted.next();
        let end = start + character.len_utf8();
        match ranges.last_mut() {
            Some(last) if last.end == start => last.end = end,
            _ => ranges.push(start..end),
        }
    }
    ranges
}

/// The best [`MAX_HITS`] paths of `chunks` from `start_chunk` on, in order.
/// `cancelled` is asked every few thousand paths; once it says yes there is no
/// result.
pub fn rank(
    query: &str,
    chunks: &[Chunk],
    start_chunk: usize,
    cancelled: &dyn Fn() -> bool,
) -> Option<Vec<Hit>> {
    let pattern = pattern_for(query);
    if pattern.atoms.is_empty() {
        return Some(Vec::new());
    }
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let mut buffer = Vec::new();
    let mut best: Vec<Hit> = Vec::with_capacity(MAX_HITS + 1);
    let mut seen: usize = 0;
    for (chunk, paths) in chunks.iter().enumerate().skip(start_chunk) {
        for (index, path) in paths.iter().enumerate() {
            seen += 1;
            if seen.is_multiple_of(CANCEL_CHECK_INTERVAL) && cancelled() {
                return None;
            }
            let Some(score) = pattern.score(Utf32Str::new(path, &mut buffer), &mut matcher) else {
                continue;
            };
            if best.len() == MAX_HITS && best.last().is_some_and(|worst| score < worst.score) {
                continue;
            }
            let hit = Hit {
                chunk,
                index,
                score,
                highlight: Vec::new(),
            };
            let at = best.partition_point(|kept| order(chunks, kept, &hit) == Ordering::Less);
            best.insert(at, hit);
            best.truncate(MAX_HITS);
        }
    }
    for hit in &mut best {
        let path = path_of(chunks, hit.chunk, hit.index);
        hit.highlight = highlight_ranges(&pattern, &mut matcher, &mut buffer, path);
    }
    Some(best)
}

/// Byte ranges of the characters of `text` that `pattern` matches, consecutive
/// ones merged. The matcher carries the config, so a caller that ranks names
/// rather than paths does not inherit the path bonuses.
pub fn highlight_ranges(
    pattern: &Pattern,
    matcher: &mut Matcher,
    buffer: &mut Vec<char>,
    text: &str,
) -> Vec<Range<usize>> {
    let mut positions = Vec::new();
    pattern.indices(Utf32Str::new(text, buffer), matcher, &mut positions);
    byte_ranges(text, &positions)
}

/// The best [`MAX_HITS`] of both lists.
pub fn merge_hits(chunks: &[Chunk], mut kept: Vec<Hit>, found: Vec<Hit>) -> Vec<Hit> {
    kept.extend(found);
    kept.sort_by(|first, second| order(chunks, first, second));
    kept.truncate(MAX_HITS);
    kept
}

impl Drop for QuickOpen {
    /// A ranking still running for this box has nothing left to find.
    fn drop(&mut self) {
        self.current_generation
            .store(u64::MAX, atomic::Ordering::Relaxed);
    }
}

impl QuickOpen {
    pub fn new(root: PathBuf, walk_generation: u64) -> Self {
        Self {
            root,
            query: String::new(),
            hits: Vec::new(),
            cursor: 0,
            walking: true,
            truncated: false,
            file_count: 0,
            walk_generation,
            candidates: Vec::new(),
            query_generation: 0,
            current_generation: Arc::new(AtomicU64::new(0)),
            scored_chunks: 0,
            pending_rescore: None,
            documents: Vec::new(),
            documents_generation: 0,
            documents_state: DocumentsState::Idle,
            documents_enabled: false,
        }
    }

    /// Turns the documents section on, for a box opened while Hermes has an
    /// index to ask.
    pub fn with_documents(mut self, enabled: bool) -> Self {
        self.documents_enabled = enabled;
        self
    }

    pub fn documents(&self) -> &[FoundDocument] {
        &self.documents
    }

    #[cfg(test)]
    pub fn documents_generation(&self) -> u64 {
        self.documents_generation
    }

    #[cfg(test)]
    pub fn documents_state(&self) -> DocumentsState {
        self.documents_state
    }

    /// Starts a full-text search for the typed text, apart from the name
    /// ranking. Nothing to ask when the section is off or the text is empty;
    /// documents of an earlier text are dropped then.
    pub fn request_documents(&mut self) -> Option<DocumentRequest> {
        self.documents_generation = self.documents_generation.wrapping_add(1);
        self.documents.clear();
        self.clamp_cursor();
        let words = self.query.trim();
        if !self.documents_enabled || words.is_empty() {
            self.documents_state = DocumentsState::Idle;
            return None;
        }
        self.documents_state = DocumentsState::Searching;
        Some(DocumentRequest {
            walk_generation: self.walk_generation,
            generation: self.documents_generation,
            words: words.to_owned(),
        })
    }

    /// The answer of a full-text search. One of an older text or another box
    /// is dropped. Name hits are never touched.
    pub fn apply_documents(
        &mut self,
        walk_generation: u64,
        generation: u64,
        result: Result<Vec<FoundDocument>, String>,
    ) {
        if walk_generation != self.walk_generation
            || generation != self.documents_generation
            || self.documents_state != DocumentsState::Searching
        {
            return;
        }
        match result {
            Ok(mut found) => {
                found.truncate(MAX_DOCUMENT_HITS);
                self.documents = found;
                self.documents_state = DocumentsState::Idle;
            }
            Err(_) => self.documents_state = DocumentsState::Failed,
        }
        self.clamp_cursor();
    }

    /// Name hits, document hits and the row that opens them all in the search
    /// page, one cursor over the lot.
    fn row_count(&self) -> usize {
        self.hits.len() + self.documents.len() + usize::from(self.has_all_hits_row())
    }

    /// The row below the documents that hands the text to the search page. It
    /// is there while documents are listed, so it has a text to take over.
    pub fn has_all_hits_row(&self) -> bool {
        self.documents_enabled && !self.documents.is_empty()
    }

    /// The position of the row that opens the search page, when it is there.
    pub fn all_hits_row(&self) -> Option<usize> {
        self.has_all_hits_row()
            .then(|| self.hits.len() + self.documents.len())
    }

    /// The cursor stands on the row that opens the search page.
    pub fn all_hits_selected(&self) -> bool {
        self.all_hits_row() == Some(self.cursor)
    }

    fn clamp_cursor(&mut self) {
        self.cursor = self.cursor.min(self.row_count().saturating_sub(1));
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn hits(&self) -> &[Hit] {
        &self.hits
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn walking(&self) -> bool {
        self.walking
    }

    pub fn truncated(&self) -> bool {
        self.truncated
    }

    pub fn file_count(&self) -> usize {
        self.file_count
    }

    pub fn walk_generation(&self) -> u64 {
        self.walk_generation
    }

    /// The path of a hit, relative to the root.
    pub fn relative_path(&self, hit: &Hit) -> &str {
        path_of(&self.candidates, hit.chunk, hit.index)
    }

    /// A background ranking is on its way, so the hits are not those of the
    /// query yet.
    pub fn is_ranking(&self) -> bool {
        self.pending_rescore.is_some()
    }

    pub fn has_query(&self) -> bool {
        !pattern_for(&self.query).atoms.is_empty()
    }

    /// The file under the cursor: a name hit, then the documents.
    pub fn selected_path(&self) -> Option<PathBuf> {
        if let Some(hit) = self.hits.get(self.cursor) {
            return Some(self.root.join(self.relative_path(hit)));
        }
        let document = self
            .documents
            .get(self.cursor.checked_sub(self.hits.len())?)?;
        Some(document.path.clone())
    }

    pub fn move_cursor(&mut self, delta: isize) {
        self.cursor = self.cursor.saturating_add_signed(delta);
        self.clamp_cursor();
    }

    pub fn select(&mut self, index: usize) {
        if index < self.row_count() {
            self.cursor = index;
        }
    }

    /// Takes the walk's files in as one chunk. With a ranking under way the
    /// chunk only waits for it; otherwise it is ranked now, a batch being small.
    pub fn apply_events(&mut self, events: Vec<SearchEvent>) {
        let mut paths: Vec<Box<str>> = Vec::new();
        for event in events {
            match event {
                SearchEvent::File { path, .. } => {
                    if let Ok(relative) = path.strip_prefix(&self.root) {
                        paths.push(relative.to_string_lossy().into());
                    }
                }
                SearchEvent::Progress { .. } => {}
                SearchEvent::Finished(totals) => {
                    self.walking = false;
                    self.truncated = totals.truncated;
                }
            }
        }
        if paths.is_empty() {
            return;
        }
        self.file_count += paths.len();
        self.candidates.push(paths.into());
        if self.pending_rescore.is_none() {
            self.score_unscored_chunks();
        }
    }

    /// The typed text changed. Returns the ranking to run in the background, or
    /// nothing when the text asks for nothing.
    pub fn set_query(&mut self, query: String) -> Option<Rescore> {
        self.query = query;
        self.query_generation = self.query_generation.wrapping_add(1);
        self.current_generation
            .store(self.query_generation, atomic::Ordering::Relaxed);
        self.hits.clear();
        self.cursor = 0;
        if !self.has_query() {
            self.pending_rescore = None;
            self.scored_chunks = self.candidates.len();
            return None;
        }
        self.pending_rescore = Some(self.query_generation);
        self.scored_chunks = 0;
        Some(Rescore {
            walk_generation: self.walk_generation,
            generation: self.query_generation,
            query: self.query.clone(),
            chunks: self.candidates.clone(),
            current_generation: Arc::clone(&self.current_generation),
        })
    }

    /// The result of a background ranking. One of an older query is dropped;
    /// chunks that came in meanwhile are ranked here and merged.
    pub fn apply_ranked(
        &mut self,
        walk_generation: u64,
        generation: u64,
        hits: Vec<Hit>,
        chunks_scored: usize,
    ) {
        if walk_generation != self.walk_generation
            || generation != self.query_generation
            || self.pending_rescore != Some(generation)
        {
            return;
        }
        self.pending_rescore = None;
        self.hits = hits;
        self.cursor = 0;
        self.scored_chunks = chunks_scored.min(self.candidates.len());
        self.score_unscored_chunks();
    }

    /// Ranks everything now, on this thread, instead of waiting for the
    /// background ranking, whose late result is then dropped. For Enter pressed
    /// before the hits are there.
    pub fn rank_now(&mut self) {
        if self.pending_rescore.take().is_none() {
            return;
        }
        self.scored_chunks = 0;
        self.hits.clear();
        self.cursor = 0;
        self.score_unscored_chunks();
    }

    fn score_unscored_chunks(&mut self) {
        let found =
            rank(&self.query, &self.candidates, self.scored_chunks, &|| false).unwrap_or_default();
        self.scored_chunks = self.candidates.len();
        if found.is_empty() {
            return;
        }
        // Name hits arriving above the documents push them down; the cursor
        // on a document goes with it.
        let on_document = self
            .cursor
            .checked_sub(self.hits.len())
            .filter(|_| !self.hits.is_empty());
        let kept_selection = self.hits.get(self.cursor).map(|hit| (hit.chunk, hit.index));
        self.hits = merge_hits(&self.candidates, std::mem::take(&mut self.hits), found);
        self.cursor = kept_selection
            .and_then(|(chunk, index)| {
                self.hits
                    .iter()
                    .position(|hit| hit.chunk == chunk && hit.index == index)
            })
            .unwrap_or(self.cursor);
        if let Some(offset) = on_document {
            self.cursor = self.hits.len() + offset;
        }
        self.clamp_cursor();
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
    use crate::fs::search::SearchTotals;

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

    fn file_events(root: &str, relative: &[&str]) -> Vec<SearchEvent> {
        relative
            .iter()
            .map(|name| SearchEvent::File {
                path: PathBuf::from(root).join(name),
                lines: Vec::new(),
                size: 0,
                modified: None,
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
        quick_open.apply_events(vec![SearchEvent::Finished(SearchTotals {
            scanned: 1,
            files: 1,
            matches: 1,
            skipped: 0,
            truncated: true,
        })]);
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
            read_by: crate::filing::TextSource::Pdf,
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
}
