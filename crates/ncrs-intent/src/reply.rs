use crate::words::tokens;
use crate::Reply;

const YES: &[&str] = &[
    "ja", "jo", "jep", "jup", "genau", "passt", "ok", "okay", "yes", "yep", "yeah", "sure",
];
const NO: &[&str] = &["nein", "nee", "no", "nope"];
const CANCEL: &[&str] = &["abbrechen", "stopp", "cancel", "stop"];
/// Words that may stand next to an answer without changing it.
const POLITE: &[&str] = &["bitte", "danke", "please", "thanks", "so"];

/// A short answer to a question: every word is a yes, a no or a cancel word,
/// with politeness words allowed beside it.
pub fn reply_of(text: &str) -> Option<Reply> {
    let words: Vec<String> = tokens(text).into_iter().map(|token| token.norm).collect();
    let in_list = |list: &[&str]| {
        words.iter().any(|word| list.contains(&word.as_str()))
            && words
                .iter()
                .all(|word| list.contains(&word.as_str()) || POLITE.contains(&word.as_str()))
    };
    if in_list(YES) {
        Some(Reply::Yes)
    } else if in_list(NO) {
        Some(Reply::No)
    } else if in_list(CANCEL) {
        Some(Reply::Cancel)
    } else {
        None
    }
}
