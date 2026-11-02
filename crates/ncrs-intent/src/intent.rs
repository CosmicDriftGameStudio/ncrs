use std::path::PathBuf;

use crate::shell::looks_like_shell;
use crate::words::{is_path, tokens, Token};
use crate::{FolderRef, Intent, TimeUnit};

/// Spoken in front of a sentence without changing what it asks.
const LEADING_FILLER: &[&str] = &["bitte", "please", "hermes", "hey", "hallo", "hi"];
const UNDO_PHRASES: &[&str] = &[
    "zuruck",
    "undo",
    "undo that",
    "undo it",
    "put it back",
    "put that back",
    "put them back",
];
const MOVE_VERBS: &[&str] = &[
    "verschieb",
    "verschiebe",
    "verschieben",
    "kopier",
    "kopiere",
    "kopieren",
    "move",
    "copy",
];
const ALL_WORDS: &[&str] = &["alle", "all"];
const TO_WORDS: &[&str] = &["nach", "to"];
const BY_WORDS: &[&str] = &["nach", "by"];
const EXPLAIN_WORDS: &[&str] = &[
    "warum", "wieso", "weshalb", "why", "explain", "erklar", "erklare",
];
const SEARCH_WORDS: &[&str] = &[
    "such", "suche", "suchen", "finde", "finden", "find", "search",
];
const IS_WORDS: &[&str] = &["ist", "sind", "is", "are"];
const TIDY_WORDS: &[&str] = &[
    "raum",
    "raume",
    "aufraumen",
    "sortier",
    "sortiere",
    "sortieren",
    "ordne",
    "ordnen",
    "tidy",
    "clean",
    "sort",
    "organise",
    "organize",
];
const LEARN_WORDS: &[&str] = &[
    "lies",
    "lese",
    "lesen",
    "read",
    "learn",
    "scan",
    "struktur",
    "structure",
];
/// Words in front of the kind in "Kontoauszüge lieber nach Monat".
const KIND_FILLER: &[&str] = &[
    "ich",
    "will",
    "mochte",
    "bitte",
    "lieber",
    "besser",
    "doch",
    "mal",
    "gern",
    "gerne",
    "i",
    "want",
    "would",
    "like",
    "please",
    "rather",
    "instead",
    "my",
    "mein",
    "meine",
    "die",
    "der",
    "das",
    "the",
    "all",
    "alle",
    "sort",
    "sortiere",
    "sortier",
    "sortieren",
    "sorted",
    "sortiert",
];
/// Capitalised words that are nouns of the sentence, not a folder name.
const NOT_A_FOLDER_NAME: &[&str] = &[
    "ordner",
    "folder",
    "dateien",
    "files",
    "archiv",
    "archive",
    "struktur",
    "structure",
    "ordnung",
    "sachen",
    "hermes",
];
/// The most words of a kind name taken from the front of "... nach Monat".
const MAX_KIND_WORDS: usize = 2;

fn is_one_of(token: &Token, list: &[&str]) -> bool {
    list.contains(&token.norm.as_str())
}

fn time_unit(token: &Token) -> Option<TimeUnit> {
    match token.norm.as_str() {
        "monat" | "month" => Some(TimeUnit::Month),
        "jahr" | "year" => Some(TimeUnit::Year),
        _ => None,
    }
}

fn text_after(words: &[Token], from: usize) -> String {
    words
        .iter()
        .skip(from)
        .map(|token| token.typed.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn home_folder(token: &Token) -> Option<FolderRef> {
    match token.norm.as_str() {
        "downloads" | "download" => Some(FolderRef::Downloads),
        "dokumente" | "dokument" | "documents" | "document" => Some(FolderRef::Documents),
        "schreibtisch" | "desktop" => Some(FolderRef::Desktop),
        _ => None,
    }
}

/// A path wins; then a known home folder; then one capitalised word after the
/// verb that is not a noun of the sentence.
fn folder_after(words: &[Token], verb_at: usize) -> Option<FolderRef> {
    let after = words.get(verb_at + 1..).unwrap_or_default();
    if let Some(path) = after.iter().find(|token| is_path(token)) {
        return Some(FolderRef::Path(PathBuf::from(&path.typed)));
    }
    if let Some(folder) = after.iter().find_map(home_folder) {
        return Some(folder);
    }
    after
        .iter()
        .find(|token| {
            token.typed.chars().next().is_some_and(char::is_uppercase)
                && !is_one_of(token, NOT_A_FOLDER_NAME)
        })
        .map(|token| FolderRef::Named(token.typed.clone()))
}

fn is_undo(words: &[Token]) -> bool {
    if words.iter().any(|token| token.norm == "ruckgangig") {
        return true;
    }
    let normalised = words
        .iter()
        .map(|token| token.norm.as_str())
        .collect::<Vec<_>>();
    let phrase = normalised.join(" ");
    let without_polite = normalised
        .iter()
        .filter(|word| !matches!(**word, "mach" | "das" | "es" | "bitte" | "please"))
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    UNDO_PHRASES.contains(&phrase.as_str()) || UNDO_PHRASES.contains(&without_polite.as_str())
}

fn batch_move(words: &[Token]) -> Option<Intent> {
    let first = words.first()?;
    if !is_one_of(first, MOVE_VERBS) {
        return None;
    }
    let all_at = words.iter().position(|token| is_one_of(token, ALL_WORDS))?;
    words
        .iter()
        .skip(all_at + 1)
        .any(|token| is_one_of(token, TO_WORDS))
        .then(|| Intent::BatchMove {
            words: text_after(words, 1),
        })
}

fn change_order(words: &[Token]) -> Option<Intent> {
    let by_at = words
        .iter()
        .enumerate()
        .skip(1)
        .find(|(index, token)| {
            is_one_of(token, BY_WORDS) && words.get(index + 1).and_then(time_unit).is_some()
        })
        .map(|(index, _)| index)?;
    let unit = words.get(by_at + 1).and_then(time_unit)?;
    let mut kind: Vec<&str> = words
        .iter()
        .take(by_at)
        .filter(|token| !is_one_of(token, KIND_FILLER))
        .map(|token| token.typed.as_str())
        .collect();
    let skip = kind.len().saturating_sub(MAX_KIND_WORDS);
    kind.drain(..skip);
    if kind.is_empty() {
        return None;
    }
    Some(Intent::ChangeOrder {
        kind_word: kind.join(" "),
        unit,
    })
}

fn search(words: &[Token]) -> Option<Intent> {
    let first = words.first()?;
    let trigger_len = if is_one_of(first, SEARCH_WORDS) {
        1
    } else if matches!(first.norm.as_str(), "wo" | "where")
        && words
            .get(1)
            .is_some_and(|second| is_one_of(second, IS_WORDS))
    {
        2
    } else {
        return None;
    };
    Some(Intent::Search {
        words: text_after(words, trigger_len),
    })
}

fn learns_structure(words: &[Token]) -> bool {
    let mentions = |wanted: &[&str]| words.iter().any(|token| is_one_of(token, wanted));
    mentions(&["aussehen"])
        || (mentions(&["look"]) && mentions(&["archive", "archiv"]))
        || mentions(LEARN_WORDS)
}

/// The order of the checks matters: shell, undo, batch move, change order,
/// explain, search, tidy, learn structure.
pub fn intent_of(text: &str) -> Intent {
    if looks_like_shell(text) {
        return Intent::ShellCommand;
    }
    let all = tokens(text);
    let skipped = all
        .iter()
        .take_while(|token| is_one_of(token, LEADING_FILLER))
        .count();
    let words = all.get(skipped..).unwrap_or_default();
    if words.is_empty() {
        return Intent::Unknown;
    }
    if is_undo(words) {
        return Intent::Undo;
    }
    if let Some(intent) = batch_move(words).or_else(|| change_order(words)) {
        return intent;
    }
    if words
        .first()
        .is_some_and(|first| is_one_of(first, EXPLAIN_WORDS))
    {
        return Intent::Explain {
            words: text_after(words, 1),
        };
    }
    if let Some(intent) = search(words) {
        return intent;
    }
    if let Some(verb_at) = words.iter().position(|token| is_one_of(token, TIDY_WORDS)) {
        return Intent::TidyFolder {
            folder: folder_after_or_before(words, verb_at),
        };
    }
    if learns_structure(words) {
        let verb_at = words
            .iter()
            .position(|token| is_one_of(token, LEARN_WORDS))
            .unwrap_or(0);
        return Intent::LearnStructure {
            folder: folder_after_or_before(words, verb_at),
        };
    }
    Intent::Unknown
}

/// "räum meine Downloads auf" has the folder after the verb; "Downloads
/// aufräumen" before it. A path or home folder counts on either side.
fn folder_after_or_before(words: &[Token], verb_at: usize) -> Option<FolderRef> {
    folder_after(words, verb_at).or_else(|| {
        let before = words.get(..verb_at).unwrap_or_default();
        before
            .iter()
            .find(|token| is_path(token))
            .map(|token| FolderRef::Path(PathBuf::from(&token.typed)))
            .or_else(|| before.iter().find_map(home_folder))
    })
}
