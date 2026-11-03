//! Values read from a document end up in a path. Nothing the text says may
//! change which folder that path points into.

use std::path::{Component, Path};

/// The longest a value taken from a document may be in a path.
pub const MAX_VALUE_CHARS: usize = 100;

/// Characters Windows refuses in a name, which also covers the drive prefix
/// `C:` on every platform.
const WINDOWS_RESERVED: [char; 7] = ['<', '>', ':', '"', '|', '?', '*'];

/// Zero-width and direction-changing characters: they do not show, so they
/// would let one name look like another.
fn is_invisible_format(character: char) -> bool {
    matches!(character, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

fn is_separator(character: char) -> bool {
    character == '/' || character == '\\'
}

/// Whether a literal piece of a pattern can be part of a name as it is.
pub fn is_plain_literal(literal: &str) -> bool {
    !literal.chars().any(|character| {
        is_separator(character)
            || character.is_control()
            || is_invisible_format(character)
            || WINDOWS_RESERVED.contains(&character)
    })
}

/// A value from a document as one path component: no separators, no control
/// or reserved characters, single spaces, no space or dot at either end, at
/// most `MAX_VALUE_CHARS` characters. `None` when nothing is left, so a value
/// of only dots counts as missing.
pub fn sanitise_value(value: &str) -> Option<String> {
    let cleaned: String = value
        .chars()
        .filter_map(|character| {
            if character.is_whitespace() {
                Some(' ')
            } else if is_separator(character)
                || character.is_control()
                || is_invisible_format(character)
                || WINDOWS_RESERVED.contains(&character)
            {
                None
            } else {
                Some(character)
            }
        })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let capped: String = collapsed
        .trim_matches([' ', '.'])
        .chars()
        .take(MAX_VALUE_CHARS)
        .collect();
    let trimmed = capped.trim_matches([' ', '.']);
    if trimmed.is_empty() {
        return None;
    }
    if is_windows_device_name(trimmed) {
        let (stem, extension) = split_stem(trimmed);
        return Some(format!("{stem}_{extension}"));
    }
    Some(trimmed.to_owned())
}

/// The name before its first dot, and the rest from that dot on.
fn split_stem(name: &str) -> (&str, &str) {
    name.split_at(name.find('.').unwrap_or(name.len()))
}

/// Whether Windows reads the name as a device (`CON`, `NUL`, `COM1` …) rather
/// than a file or folder, with or without an extension and in any case.
pub fn is_windows_device_name(name: &str) -> bool {
    let stem = split_stem(name).0.trim_end_matches(' ');
    const PLAIN: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];
    if PLAIN.iter().any(|device| stem.eq_ignore_ascii_case(device)) {
        return true;
    }
    let numbered = |prefix: &str| {
        stem.len() == prefix.len() + 1
            && stem
                .get(..prefix.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
            && stem
                .bytes()
                .last()
                .is_some_and(|digit| (b'1'..=b'9').contains(&digit))
    };
    numbered("COM") || numbered("LPT")
}

/// Whether a relative path stays inside the folder it is joined to. Checked
/// component by component: a text search for `..` would miss what the
/// platform reads as a parent, and would refuse a name like `a..b`.
pub fn stays_below(relative: &Path) -> bool {
    let mut components = relative.components().peekable();
    components.peek().is_some()
        && components.all(|component| matches!(component, Component::Normal(_)))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    #[test]
    fn separators_control_and_reserved_characters_are_removed() {
        assert_eq!(
            sanitise_value("../../etc/passwd").as_deref(),
            Some("etcpasswd")
        );
        assert_eq!(sanitise_value("a\\b\u{0}c\u{7}d").as_deref(), Some("abcd"));
        assert_eq!(sanitise_value("a<b>:\"|?*c").as_deref(), Some("abc"));
        assert_eq!(sanitise_value("x\u{202E}gpj").as_deref(), Some("xgpj"));
    }

    #[test]
    fn whitespace_is_collapsed_and_ends_are_trimmed() {
        assert_eq!(
            sanitise_value(" . Company \t\n A .. ").as_deref(),
            Some("Company A")
        );
    }

    #[test]
    fn a_value_of_only_dots_or_nothing_is_missing() {
        for value in ["", " ", ".", "..", "...", " . . ", "\u{0}", "/", "../.."] {
            assert_eq!(sanitise_value(value), None, "{value:?}");
        }
    }

    #[test]
    fn a_device_name_gets_an_underscore_after_its_stem() {
        for (value, expected) in [
            ("CON", "CON_"),
            ("nul", "nul_"),
            ("Com1", "Com1_"),
            ("LPT9", "LPT9_"),
            ("aux.txt", "aux_.txt"),
            ("PRN.tar.gz", "PRN_.tar.gz"),
            ("CONSULT", "CONSULT"),
            ("COM0", "COM0"),
            ("COM10", "COM10"),
            ("a.CON", "a.CON"),
        ] {
            assert_eq!(sanitise_value(value).as_deref(), Some(expected), "{value}");
        }
    }

    #[test]
    fn device_names_are_told_from_other_names() {
        for device in ["CON", "prn", "Aux.pdf", "NUL", "COM1", "lpt5.txt", "CON .x"] {
            assert!(is_windows_device_name(device), "{device}");
        }
        for other in ["", "COM", "COM0", "COM12", "LPT", "console", "x.CON"] {
            assert!(!is_windows_device_name(other), "{other}");
        }
    }

    #[test]
    fn a_long_value_is_cut_on_a_character_boundary() {
        let long = "ä".repeat(300);
        assert_eq!(
            sanitise_value(&long).unwrap().chars().count(),
            MAX_VALUE_CHARS
        );
        let ends_in_space = format!("{} tail", "b".repeat(MAX_VALUE_CHARS - 1));
        assert_eq!(
            sanitise_value(&ends_in_space).unwrap(),
            "b".repeat(MAX_VALUE_CHARS - 1)
        );
    }
}
