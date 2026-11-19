//! The spellings names are compared by.

use unicode_normalization::UnicodeNormalization as _;

/// The spelling of a tag: lowercase words joined by single hyphens.
pub fn tag_name(text: &str) -> String {
    let mut name = String::new();
    let mut pending_hyphen = false;
    for character in text.trim().chars() {
        if character.is_alphanumeric() {
            if pending_hyphen && !name.is_empty() {
                name.push('-');
            }
            pending_hyphen = false;
            name.extend(character.to_lowercase());
        } else {
            pending_hyphen = true;
        }
    }
    name
}

/// What two names are compared by: trimmed, lower-cased and composed, so
/// "Mu\u{308}ller" from a macOS file name and "Müller" from a text are one.
pub fn name_key(name: &str) -> String {
    name.trim().to_lowercase().nfc().collect()
}
