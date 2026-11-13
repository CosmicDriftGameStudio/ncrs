//! Rendered Markdown: parse a text once, draw the parsed items in the colors of
//! the window, and turn a click on a link into the caller's message. Shared by
//! the file viewer and the notes, so Markdown looks the same in both.

use std::path::{Path, PathBuf};

use iced::widget::markdown;
use iced::{border, Element};

use crate::palette::Palette;
use crate::theme::font_size;

/// One parsed block of a Markdown text.
pub use markdown::Item as MarkdownItem;

/// The blocks of `source`; keep them in the state, parsing is not for `view`.
pub fn parse_markdown(source: &str) -> Vec<MarkdownItem> {
    markdown::parse(source).collect()
}

/// `items` drawn in the palette. A click on a link (or an image) gives
/// `on_link` the address as written in the text.
pub fn markdown_view<'a, M: 'a>(
    items: &'a [MarkdownItem],
    palette: &Palette,
    on_link: impl Fn(String) -> M + 'a,
) -> Element<'a, M> {
    let style = markdown::Style {
        font: crate::APP_FONT,
        inline_code_highlight: markdown::Highlight {
            background: palette.panel.into(),
            border: border::rounded(crate::theme::radius::CORNER),
        },
        inline_code_padding: iced::padding::left(1).right(1),
        inline_code_color: palette.text,
        inline_code_font: crate::APP_FONT,
        code_block_font: crate::APP_FONT,
        link_color: palette.primary_button().0,
    };
    let settings = markdown::Settings::with_text_size(font_size::ROW, style);
    markdown::view(items, settings).map(on_link)
}

const MARKDOWN_EXTENSIONS: [&str; 2] = ["md", "markdown"];

pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .map(|extension| extension.to_string_lossy().to_lowercase())
        .is_some_and(|extension| MARKDOWN_EXTENSIONS.contains(&extension.as_str()))
}

/// What a link of a rendered text points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// An anchor inside the text, an empty link or a scheme ncrs does not open.
    Ignore,
    /// http, https or mailto.
    Url(String),
    /// A Markdown file.
    Markdown(PathBuf),
    /// Any other file.
    File(PathBuf),
}

const OPENED_SCHEMES: [&str; 3] = ["http", "https", "mailto"];

/// Sorts `written`, a link as written in the text of a file in `base_dir`.
pub fn classify(written: &str, base_dir: &Path) -> Link {
    let target = written.trim();
    if target.is_empty() || target.starts_with('#') {
        return Link::Ignore;
    }
    if let Some((scheme, rest)) = split_scheme(target) {
        let lower = scheme.to_lowercase();
        if OPENED_SCHEMES.contains(&lower.as_str()) {
            return Link::Url(target.to_owned());
        }
        if lower == "file" {
            let path = rest.strip_prefix("//").unwrap_or(rest);
            return path_link(path, base_dir);
        }
        return Link::Ignore;
    }
    path_link(target, base_dir)
}

/// `scheme:rest` when `target` starts with one; a single letter is a drive.
fn split_scheme(target: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = target.split_once(':')?;
    let is_scheme = scheme.len() > 1
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    is_scheme.then_some((scheme, rest))
}

fn path_link(written: &str, base_dir: &Path) -> Link {
    let without_anchor = written.split(['#', '?']).next().unwrap_or_default();
    if without_anchor.is_empty() {
        return Link::Ignore;
    }
    let path = base_dir.join(percent_decoded(without_anchor));
    if is_markdown(&path) {
        Link::Markdown(path)
    } else {
        Link::File(path)
    }
}

fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        let hex = bytes
            .get(index + 1..index + 3)
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match (byte, hex) {
            (b'%', Some(decoded)) => {
                out.push(decoded);
                index += 3;
            }
            _ => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

// A failing assertion in a test is the signal, so `unwrap` belongs here.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_heading_a_paragraph_and_a_code_block_are_three_items() {
        let items = parse_markdown("# Title\n\nSome *text*.\n\n```\ncode\n```\n");
        assert_eq!(items.len(), 3);
        assert!(matches!(items[0], MarkdownItem::Heading(..)));
        assert!(matches!(items[1], MarkdownItem::Paragraph(_)));
        assert!(matches!(items[2], MarkdownItem::CodeBlock { .. }));
    }

    #[test]
    fn the_view_builds_for_a_text_with_a_link_and_a_table() {
        let items = parse_markdown("[a](b.md)\n\n| x | y |\n|---|---|\n| 1 | 2 |\n");
        let _view: Element<'_, String> =
            markdown_view(&items, &Palette::default(), |address| address);
    }

    fn sorted(target: &str) -> Link {
        classify(target, Path::new("/notes/docs"))
    }

    #[test]
    fn web_and_mail_addresses_go_to_the_system() {
        assert_eq!(
            sorted("https://example.org/a#b"),
            Link::Url("https://example.org/a#b".into())
        );
        assert_eq!(sorted("mailto:a@b.de"), Link::Url("mailto:a@b.de".into()));
    }

    #[test]
    fn anchors_and_foreign_schemes_do_nothing() {
        assert_eq!(sorted("#install"), Link::Ignore);
        assert_eq!(sorted(""), Link::Ignore);
        assert_eq!(sorted("javascript:alert(1)"), Link::Ignore);
    }

    #[test]
    fn relative_markdown_stays_in_the_tab_and_other_files_are_revealed() {
        assert_eq!(
            sorted("../guide/Intro%20One.md#top"),
            Link::Markdown(PathBuf::from("/notes/docs/../guide/Intro One.md"))
        );
        assert_eq!(
            sorted("file:///tmp/a.pdf"),
            Link::File(PathBuf::from("/tmp/a.pdf"))
        );
        assert_eq!(
            sorted("img/logo.png"),
            Link::File(PathBuf::from("/notes/docs/img/logo.png"))
        );
    }
}
