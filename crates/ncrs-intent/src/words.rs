//! Word-level helpers shared by the intent and reply parsers.

/// Lowercase, umlauts and ß folded to ASCII, punctuation at both ends trimmed.
pub fn normalize_word(word: &str) -> String {
    let mut folded = String::with_capacity(word.len());
    for character in word.trim_matches(|c: char| !c.is_alphanumeric()).chars() {
        for lower in character.to_lowercase() {
            match lower {
                'ä' => folded.push('a'),
                'ö' => folded.push('o'),
                'ü' => folded.push('u'),
                'ß' => folded.push_str("ss"),
                other => folded.push(other),
            }
        }
    }
    folded
}

const PLURAL_ENDINGS: [&str; 4] = ["en", "e", "n", "s"];
/// A stem shorter than this is the word itself, not a plural ("es", "an").
const MIN_STEM: usize = 3;

fn without_plural_ending(word: &str) -> &str {
    for ending in PLURAL_ENDINGS {
        if let Some(stem) = word.strip_suffix(ending) {
            if stem.chars().count() >= MIN_STEM {
                return stem;
            }
        }
    }
    word
}

/// Whether two spellings name the same thing, ignoring case, umlauts and one
/// German or English plural ending on either side.
pub fn same_word(typed: &str, name: &str) -> bool {
    let typed_norm = normalize_word(typed);
    let name_norm = normalize_word(name);
    if typed_norm.is_empty() || name_norm.is_empty() {
        return false;
    }
    typed_norm == name_norm
        || without_plural_ending(&typed_norm) == name_norm
        || typed_norm == without_plural_ending(&name_norm)
}

/// One whitespace-separated word of the input: as typed (quotes and trailing
/// punctuation trimmed, paths intact) and normalised.
#[derive(Debug, Clone)]
pub struct Token {
    pub typed: String,
    pub norm: String,
}

const OPENING_MARKS: &[char] = &['„', '“', '”', '"', '\'', '‚', '‘', '’', '(', '«', '»'];
const CLOSING_MARKS: &[char] = &[
    '„', '“', '”', '"', '\'', '‚', '‘', '’', ')', '«', '»', '?', '!', '.', ',', ';', ':', '…',
];

pub fn tokens(text: &str) -> Vec<Token> {
    text.split_whitespace()
        .map(|raw| {
            raw.trim_start_matches(OPENING_MARKS)
                .trim_end_matches(CLOSING_MARKS)
        })
        .filter(|typed| !typed.is_empty())
        .map(|typed| Token {
            typed: typed.to_owned(),
            norm: normalize_word(typed),
        })
        .collect()
}

pub fn is_path(token: &Token) -> bool {
    ["/", "~/", "./"]
        .iter()
        .any(|start| token.typed.starts_with(start))
}
