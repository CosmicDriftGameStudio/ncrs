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

use ncrs_core::FoundDocument;

use crate::message::QuickOpenMessage;

pub mod action;
pub mod ctx;
pub mod update;
pub mod view;
pub mod walk;

pub use action::QuickOpenAction;
pub use ctx::QuickOpenCtx;
pub use update::{close, update};
pub use walk::find_documents;

/// What the file walk reports to Quick Open: the files, and that it is over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalkEvent {
    File { path: PathBuf },
    Finished { truncated: bool },
}

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
    pub async fn run_in_background(self) -> QuickOpenMessage {
        let (walk_generation, generation) = (self.walk_generation, self.generation);
        tokio::task::spawn_blocking(move || self.run())
            .await
            .unwrap_or(QuickOpenMessage::Ranked {
                walk_generation,
                generation,
                hits: Vec::new(),
                chunks_scored: 0,
            })
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

    #[cfg(any(test, feature = "test-support"))]
    pub fn documents_generation(&self) -> u64 {
        self.documents_generation
    }

    #[cfg(any(test, feature = "test-support"))]
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
    pub fn apply_events(&mut self, events: Vec<WalkEvent>) {
        let mut paths: Vec<Box<str>> = Vec::new();
        for event in events {
            match event {
                WalkEvent::File { path } => {
                    if let Ok(relative) = path.strip_prefix(&self.root) {
                        paths.push(relative.to_string_lossy().into());
                    }
                }
                WalkEvent::Finished { truncated } => {
                    self.walking = false;
                    self.truncated = truncated;
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
mod tests;
