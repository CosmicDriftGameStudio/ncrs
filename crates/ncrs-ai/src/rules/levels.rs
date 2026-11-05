//! The answer of the model is a list of levels, not a pattern string: one level
//! per folder and one for the file name. This turns levels into the real
//! `filing_pattern` and back (for the "5 levels found" view and for the
//! expected answers of the tests).

use ncrs_intake::FILING_PLACEHOLDERS;

/// What joins the parts of a level. The empty text joins them directly.
pub const SEPARATORS: [&str; 5] = ["-", "_", " ", ".", ""];
/// Most parts in one level.
pub const MOST_PARTS: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    Fixed(String),
    Placeholder(&'static str),
}

impl Part {
    fn text(&self) -> String {
        match self {
            Self::Fixed(text) => text.clone(),
            Self::Placeholder(name) => format!("{{{name}}}"),
        }
    }

    fn json(&self) -> String {
        match self {
            Self::Fixed(text) => format!("{{\"fest\":\"{text}\"}}"),
            Self::Placeholder(name) => format!("\"{name}\""),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Level {
    pub parts: Vec<Part>,
    pub separator: &'static str,
}

/// The placeholder with this name, as the static name of the constant.
pub fn placeholder_named(name: &str) -> Option<&'static str> {
    FILING_PLACEHOLDERS
        .into_iter()
        .find(|placeholder| *placeholder == name)
}

pub fn separator_named(text: &str) -> Option<&'static str> {
    SEPARATORS.into_iter().find(|separator| *separator == text)
}

enum Token<'text> {
    Literal(&'text str),
    Placeholder(&'static str),
}

fn tokens(segment: &str) -> Option<Vec<Token<'_>>> {
    let mut found = Vec::new();
    let mut rest = segment;
    while !rest.is_empty() {
        match rest.find('{') {
            Some(0) => {
                let end = rest.find('}')?;
                let name = placeholder_named(rest.get(1..end)?)?;
                found.push(Token::Placeholder(name));
                rest = rest.get(end + 1..)?;
            }
            Some(start) => {
                found.push(Token::Literal(rest.get(..start)?));
                rest = rest.get(start..)?;
            }
            None => {
                found.push(Token::Literal(rest));
                rest = "";
            }
        }
    }
    let stray_brace = found
        .iter()
        .any(|token| matches!(token, Token::Literal(text) if text.contains('}')));
    (!stray_brace).then_some(found)
}

fn part_of(token: &Token<'_>) -> Part {
    match token {
        Token::Literal(text) => Part::Fixed((*text).to_owned()),
        Token::Placeholder(name) => Part::Placeholder(name),
    }
}

impl Level {
    fn from_segment(segment: &str) -> Option<Self> {
        let found = tokens(segment)?;
        if found.is_empty() {
            return None;
        }
        if let [only] = found.as_slice() {
            return Some(Self {
                parts: vec![part_of(only)],
                separator: "",
            });
        }
        for separator in ["-", "_", " ", "."] {
            if let Some(parts) = split_parts(&found, separator) {
                return Some(Self { parts, separator });
            }
        }
        if found.len() <= MOST_PARTS {
            return Some(Self {
                parts: found.iter().map(part_of).collect(),
                separator: "",
            });
        }
        None
    }

    pub fn text(&self) -> String {
        self.parts
            .iter()
            .map(Part::text)
            .collect::<Vec<_>>()
            .join(self.separator)
    }

    pub fn json(&self) -> String {
        let parts = self
            .parts
            .iter()
            .map(Part::json)
            .collect::<Vec<_>>()
            .join(",");
        format!("{{\"teile\":[{parts}],\"trenner\":\"{}\"}}", self.separator)
    }
}

/// One piece per run between separators: a piece is a lone literal or a lone
/// placeholder, or the split does not fit.
fn split_parts(found: &[Token<'_>], separator: &str) -> Option<Vec<Part>> {
    let mut pieces: Vec<Vec<Part>> = vec![Vec::new()];
    for token in found {
        match token {
            Token::Placeholder(name) => pieces.last_mut()?.push(Part::Placeholder(name)),
            Token::Literal(text) => {
                for (index, fragment) in text.split(separator).enumerate() {
                    if index > 0 {
                        pieces.push(Vec::new());
                    }
                    if !fragment.is_empty() {
                        pieces.last_mut()?.push(Part::Fixed(fragment.to_owned()));
                    }
                }
            }
        }
    }
    if pieces.len() > MOST_PARTS {
        return None;
    }
    pieces
        .into_iter()
        .map(|mut piece| {
            let single = piece.pop()?;
            piece.is_empty().then_some(single)
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Levels {
    pub folders: Vec<Level>,
    /// `None`: the file name is made automatically (the pattern ends with `/`).
    pub file: Option<Level>,
}

impl Levels {
    /// The levels of a pattern, or `None` for a text that is no pattern of
    /// this shape (an empty part, an unknown placeholder, too many parts).
    pub fn from_pattern(pattern: &str) -> Option<Self> {
        let (body, automatic) = match pattern.strip_suffix('/') {
            Some(body) => (body, true),
            None => (pattern, false),
        };
        let mut levels = body
            .split('/')
            .map(Level::from_segment)
            .collect::<Option<Vec<_>>>()?;
        let file = if automatic { None } else { levels.pop() };
        Some(Self {
            folders: levels,
            file,
        })
    }

    pub fn pattern(&self) -> String {
        let mut texts: Vec<String> = self.folders.iter().map(Level::text).collect();
        match &self.file {
            Some(file) => {
                texts.push(file.text());
                texts.join("/")
            }
            None => format!("{}/", texts.join("/")),
        }
    }

    /// How many levels the user sees: the folders and the file name.
    pub fn count(&self) -> usize {
        self.folders.len() + usize::from(self.file.is_some())
    }

    /// The `"ordner":[...],"datei":...` part of an answer.
    pub fn json_fields(&self) -> String {
        let folders = self
            .folders
            .iter()
            .map(Level::json)
            .collect::<Vec<_>>()
            .join(",");
        let file = self
            .file
            .as_ref()
            .map_or_else(|| "null".to_owned(), Level::json);
        format!("\"ordner\":[{folders}],\"datei\":{file}")
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;
    use crate::rules::matrix::all_cases;
    use crate::rules::matrix::Expected;

    fn round(pattern: &str) -> String {
        Levels::from_pattern(pattern).unwrap().pattern()
    }

    #[test]
    fn every_pattern_of_the_matrix_comes_back_unchanged() {
        let mut seen = 0;
        for case in all_cases() {
            let pattern = match &case.expected {
                Expected::Rule(spec) => spec.filing_pattern.clone(),
                Expected::Pattern(pattern) => pattern.clone(),
            };
            assert_eq!(round(&pattern), pattern);
            seen += 1;
        }
        assert!(seen > 1000);
    }

    #[test]
    fn folders_and_a_file_name_are_told_apart() {
        let levels = Levels::from_pattern("Bank/{year}/{date}_{sender}_{kind}").unwrap();
        assert_eq!(levels.count(), 3);
        assert_eq!(levels.folders.len(), 2);
        let file = levels.file.unwrap();
        assert_eq!(file.separator, "_");
        assert_eq!(
            file.parts,
            vec![
                Part::Placeholder("date"),
                Part::Placeholder("sender"),
                Part::Placeholder("kind")
            ]
        );
        let automatic = Levels::from_pattern("{year}/{month}/").unwrap();
        assert_eq!(automatic.file, None);
        assert_eq!(automatic.count(), 2);
    }

    #[test]
    fn the_date_stays_what_it_was_written_as() {
        assert_eq!(round("{date}-{sender}"), "{date}-{sender}");
        assert_eq!(round("{year}-{month}-{day}"), "{year}-{month}-{day}");
        let levels = Levels::from_pattern("{year}-{month}-{day}").unwrap();
        assert_eq!(levels.file.unwrap().parts.len(), 3);
    }

    #[test]
    fn a_fixed_word_sits_next_to_placeholders_in_one_level() {
        let file = Levels::from_pattern("{sender}_Tax_{year}")
            .unwrap()
            .file
            .unwrap();
        assert_eq!(file.separator, "_");
        assert_eq!(file.parts[1], Part::Fixed("Tax".to_owned()));
        assert_eq!(
            round("Rechnung_{sender}_{year}"),
            "Rechnung_{sender}_{year}"
        );
        assert_eq!(round("Mail-Eingang_{year}"), "Mail-Eingang_{year}");
        assert_eq!(round("Post{year}"), "Post{year}");
        assert_eq!(round("Dr. Schäfer/{year}/"), "Dr. Schäfer/{year}/");
    }

    #[test]
    fn the_answer_fields_name_the_levels() {
        let levels = Levels::from_pattern("Bank/{year}-{month}").unwrap();
        assert_eq!(
            levels.json_fields(),
            r#""ordner":[{"teile":[{"fest":"Bank"}],"trenner":""}],"datei":{"teile":["year","month"],"trenner":"-"}"#
        );
        assert!(Levels::from_pattern("{year}/")
            .unwrap()
            .json_fields()
            .ends_with("\"datei\":null"));
    }

    #[test]
    fn what_is_no_pattern_of_this_shape_is_refused() {
        for pattern in [
            "",
            "/",
            "a//b",
            "{nope}/",
            "{year",
            "a}/",
            "{year}{month}{day}{date}{kind}",
        ] {
            assert!(Levels::from_pattern(pattern).is_none(), "{pattern}");
        }
    }
}
