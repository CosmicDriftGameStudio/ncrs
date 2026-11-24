//! A key to compare senders written in different ways: "Deutsche Bank AG",
//! "Ihre Deutsche Bank Aktiengesellschaft" and "Deutsche Bank AG Musterweg 4
//! 80331 München" are one sender.

/// Legal forms and filler words that say nothing about who the sender is.
/// Compared after the punctuation at the edge of a word is cut off.
const FILLER_WORDS: &[&str] = &[
    "ag",
    "aktiengesellschaft",
    "gmbh",
    "mbh",
    "se",
    "kg",
    "kgaa",
    "co",
    "ohg",
    "ug",
    "e.v",
    "ev",
    "inc",
    "ltd",
    "llc",
    "plc",
    "pbc",
    "sa",
    "sarl",
    "bv",
    "nv",
    "sprl",
    "bvba",
    "haftungsbeschränkt",
    "team",
    "ihre",
    "ihr",
    "dein",
    "deine",
    "your",
    "the",
    "und",
];
/// A sender of one word only matches itself: a prefix of one word is too
/// likely another sender.
const MIN_PREFIX_TOKENS: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SenderKey {
    tokens: Vec<String>,
}

impl SenderKey {
    /// `None` when nothing is left of the sender after the cleaning.
    pub fn of(sender: &str) -> Option<Self> {
        let lowered = sender.to_lowercase();
        let tokens: Vec<String> = lowered
            .split_whitespace()
            .take_while(|token| !token.chars().any(char::is_numeric))
            .map(|token| token.trim_matches(|character: char| !character.is_alphanumeric()))
            .filter(|token| !token.is_empty() && !FILLER_WORDS.contains(token))
            .map(str::to_owned)
            .collect();
        (!tokens.is_empty()).then_some(Self { tokens })
    }

    /// Equal, or the words of the shorter one start the longer one.
    pub fn matches(&self, other: &Self) -> bool {
        let (shorter, longer) = if self.tokens.len() <= other.tokens.len() {
            (&self.tokens, &other.tokens)
        } else {
            (&other.tokens, &self.tokens)
        };
        shorter == longer
            || (shorter.len() >= MIN_PREFIX_TOKENS && longer.starts_with(shorter.as_slice()))
    }

    pub fn joined(&self) -> String {
        self.tokens.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(sender: &str) -> SenderKey {
        SenderKey::of(sender).unwrap()
    }

    #[test]
    fn spellings_of_one_sender_match() {
        let spellings = [
            "Deutsche Bank AG",
            "Deutsche Bank",
            "Deutsche Bank Aktiengesellschaft",
            "Ihre Deutsche Bank AG",
            "Deutsche Bank Baufinanzierung",
        ];
        for left in spellings {
            for right in spellings {
                assert!(key(left).matches(&key(right)), "{left} / {right}");
            }
        }
        assert!(
            key("Baader Bank AG Weihenstephaner Straße 4 85716 Unterschleißheim")
                .matches(&key("Baader Bank Aktiengesellschaft"))
        );
        assert_eq!(key("Anthropic, PBC"), key("Anthropic"));
        assert_eq!(key("finanzen.net zero Team").joined(), "finanzen.net zero");
    }

    #[test]
    fn other_senders_do_not_match() {
        assert!(!key("Deutsche Kreditbank AG").matches(&key("Deutsche Bank AG")));
        assert!(!key("KfW").matches(&key("KfW Bankengruppe")));
        assert!(key("KfW").matches(&key("kfw")));
    }

    #[test]
    fn a_sender_of_only_fillers_or_numbers_has_no_key() {
        assert_eq!(SenderKey::of("GmbH"), None);
        assert_eq!(SenderKey::of("12345 Berlin"), None);
    }
}
