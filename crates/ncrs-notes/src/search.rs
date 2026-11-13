//! Full-text search over the notes in memory: a linear scan, which is fine up
//! to a few thousand notes.

use std::path::PathBuf;

use crate::store::{Kind, Note};

const MAX_HITS: usize = 200;
const CONTEXT_CHARS: usize = 36;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub note: PathBuf,
    /// The text around the first match, with the ends marked by `…`.
    pub snippet: String,
}

fn folded(text: &str) -> Vec<char> {
    text.chars()
        .map(|c| c.to_lowercase().next().unwrap_or(c))
        .collect()
}

fn find(haystack: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn snippet_around(text: &str, at: usize, length: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let from = at.saturating_sub(CONTEXT_CHARS);
    let to = (at + length + CONTEXT_CHARS).min(chars.len());
    let body: String = chars
        .get(from..to)
        .unwrap_or_default()
        .iter()
        .map(|c| if c.is_control() { ' ' } else { *c })
        .collect();
    let lead = if from > 0 { "\u{2026} " } else { "" };
    let tail = if to < chars.len() { " \u{2026}" } else { "" };
    format!("{lead}{}{tail}", body.trim())
}

/// The notes that have `query` in their title, their text or, for a note on a
/// file, the path of the file; titles first.
pub fn search(notes: &[Note], query: &str) -> Vec<Hit> {
    let needle = folded(query.trim());
    if needle.is_empty() {
        return Vec::new();
    }
    let mut by_title = Vec::new();
    let mut by_text = Vec::new();
    for note in notes {
        let in_text = find(&folded(&note.text), &needle);
        let in_title = find(&folded(&note.title), &needle).is_some()
            || matches!(&note.kind, Kind::File { file, .. }
                if find(&folded(&file.to_string_lossy()), &needle).is_some());
        let snippet = |at| snippet_around(&note.text, at, needle.len());
        match (in_title, in_text) {
            (true, text_at) => by_title.push(Hit {
                note: note.path.clone(),
                snippet: text_at
                    .map_or_else(|| note.first_line().unwrap_or_default().to_owned(), snippet),
            }),
            (false, Some(at)) => by_text.push(Hit {
                note: note.path.clone(),
                snippet: snippet(at),
            }),
            (false, None) => {}
        }
    }
    by_title.extend(by_text);
    by_title.truncate(MAX_HITS);
    by_title
}
