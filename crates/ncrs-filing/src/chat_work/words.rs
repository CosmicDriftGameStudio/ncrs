//! Name matching for folders and kinds the user typed.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_folded_and_plurals_match() {
        assert_eq!(normalize_word("„Größe?“"), "grosse");
        assert_eq!(normalize_word("Kontoauszüge"), "kontoauszuge");
        assert!(same_word("Kontoauszüge", "Kontoauszug"));
        assert!(same_word("Kontoauszug", "Kontoauszüge"));
        assert!(same_word("Rechnungen", "Rechnung"));
        assert!(same_word("invoices", "invoice"));
        assert!(same_word("Rechnung", "rechnung"));
        assert!(!same_word("Rechnung", "Rechner"));
        assert!(!same_word("Vertrag", "Vertrags-Kunde"));
        assert!(!same_word("", ""));
        assert!(!same_word("es", "e"));
    }
}
