//! The line that says what a copy or move left out while the rest went on.

use ncrs_fs::transfer::{SkipKind, Skipped};
use ncrs_i18n::{fill_template, Language, Msg};

/// How many names the line spells out; the rest is only counted.
const EXAMPLES: usize = 3;

/// `None` when nothing was left out.
pub fn skipped_text(lang: Language, skipped: &[Skipped]) -> Option<String> {
    if skipped.is_empty() {
        return None;
    }
    let mut kinds: Vec<&str> = Vec::new();
    for (kind, msg) in [
        (SkipKind::Link, Msg::SkippedKindLink),
        (SkipKind::Special, Msg::SkippedKindSpecial),
        (SkipKind::Attributes, Msg::SkippedKindAttributes),
        (SkipKind::NameTaken, Msg::SkippedKindNameTaken),
    ] {
        if skipped.iter().any(|item| item.kind == kind) {
            kinds.push(lang.text(msg));
        }
    }
    let mut examples: Vec<String> = skipped
        .iter()
        .take(EXAMPLES)
        .map(|item| {
            item.path
                .file_name()
                .map_or_else(
                    || item.path.to_string_lossy(),
                    |name| name.to_string_lossy(),
                )
                .into_owned()
        })
        .collect();
    if skipped.len() > EXAMPLES {
        examples.push("…".to_owned());
    }
    Some(fill_template(
        lang.text(Msg::TransferSkipped),
        &[
            ("{count}", &skipped.len().to_string()),
            ("{kinds}", &kinds.join(", ")),
            ("{examples}", &examples.join(", ")),
        ],
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn skip(path: &str, kind: SkipKind) -> Skipped {
        Skipped {
            path: PathBuf::from(path),
            kind,
            detail: String::new(),
        }
    }

    #[test]
    fn nothing_skipped_says_nothing() {
        assert_eq!(skipped_text(Language::default(), &[]), None);
    }

    #[test]
    fn the_line_counts_all_and_names_a_few() {
        let items: Vec<Skipped> = (0..5)
            .map(|n| skip(&format!("/a/link{n}"), SkipKind::Link))
            .collect();

        let text = skipped_text(Language::default(), &items).unwrap();

        assert!(text.contains('5'), "{text}");
        assert!(text.contains("link0") && text.contains("link2"), "{text}");
        assert!(!text.contains("link3"), "{text}");
    }
}
