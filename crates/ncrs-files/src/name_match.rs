//! Comparing file names the way a user types them. macOS hands out names
//! decomposed (NFD) while the keyboard produces them composed (NFC), so a plain
//! comparison misses the file the user means.

use std::ffi::OsStr;
use unicode_normalization::UnicodeNormalization as _;

/// The name composed (NFC), for the cursor to find a file by the name it was asked for.
pub fn composed(name: &OsStr) -> String {
    name.to_string_lossy().nfc().collect()
}

/// The name composed and lower-cased, for type-ahead matching.
pub fn type_ahead_key(name: &OsStr) -> String {
    composed(name).to_lowercase()
}

/// The text typed so far, composed and lower-cased like [`type_ahead_key`].
pub fn typed_key(typed: &str) -> String {
    typed.nfc().collect::<String>().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_decomposed_name_matches_what_is_typed_composed() {
        let decomposed = OsStr::new("U\u{308}bung.txt");
        assert!(type_ahead_key(decomposed).starts_with(&typed_key("\u{dc}")));
        assert_eq!(composed(decomposed), "\u{dc}bung.txt");
    }
}
