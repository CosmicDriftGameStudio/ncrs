//! The `[[open.by_type]]` rules of the config file: what F3 does with a file,
//! decided by its extension.

use std::path::Path;

use serde::Deserialize;

use crate::open_programs::ProgramLine;

/// What F3 does with a file a rule matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenWith {
    /// The viewer tab of ncrs.
    Internal,
    /// The program the system has for the file.
    System,
    /// This program, `{path}` standing for the file.
    Program(ProgramLine),
}

/// One `[[open.by_type]]` entry: the extensions (lower case, no dot) and what
/// to do with them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "RawRule")]
pub struct OpenRule {
    pub extensions: Vec<String>,
    pub with: OpenWith,
}

/// The rules in effect when the file has no `by_type` key: Markdown opens in
/// the viewer tab.
pub fn default_rules() -> Vec<OpenRule> {
    vec![OpenRule {
        extensions: vec!["md".to_owned(), "markdown".to_owned()],
        with: OpenWith::Internal,
    }]
}

/// The first rule that names the extension of `path`, case-insensitively.
pub fn matching_with(rules: &[OpenRule], path: &Path) -> Option<OpenWith> {
    let extension = path.extension()?.to_string_lossy().to_lowercase();
    rules
        .iter()
        .find(|rule| rule.extensions.contains(&extension))
        .map(|rule| rule.with.clone())
}

/// Extensions as typed in the settings, "md, .Markdown": split at commas and
/// whitespace, the dot dropped, lower case, no empty entries.
pub fn parse_extensions(text: &str) -> Vec<String> {
    text.split(|c: char| c == ',' || c.is_whitespace())
        .map(|word| word.trim().trim_start_matches('.').to_lowercase())
        .filter(|word| !word.is_empty())
        .collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    extensions: Vec<String>,
    with: String,
    #[serde(default)]
    program: Option<String>,
}

impl TryFrom<RawRule> for OpenRule {
    type Error = String;

    fn try_from(raw: RawRule) -> Result<Self, String> {
        let extensions: Vec<String> = raw
            .extensions
            .iter()
            .flat_map(|text| parse_extensions(text))
            .collect();
        if extensions.is_empty() {
            return Err("`extensions` needs at least one extension".to_owned());
        }
        let with = match (raw.with.as_str(), raw.program) {
            ("internal", None) => OpenWith::Internal,
            ("system", None) => OpenWith::System,
            ("internal" | "system", Some(_)) => {
                return Err("`program` only goes with `with = \"program\"`".to_owned())
            }
            ("program", Some(line)) => OpenWith::Program(
                ProgramLine::from_text(&line)
                    .map_err(|problem| format!("`program` {}", problem.reason()))?,
            ),
            ("program", None) => return Err("`with = \"program\"` needs `program`".to_owned()),
            _ => return Err("`with` is internal, system or program".to_owned()),
        };
        Ok(Self { extensions, with })
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_are_typed_loosely_and_stored_plain() {
        assert_eq!(
            parse_extensions("md, .Markdown  txt,,"),
            ["md", "markdown", "txt"]
        );
    }

    #[test]
    fn the_first_matching_rule_wins_whatever_the_case() {
        let rules = vec![
            OpenRule {
                extensions: vec!["md".into()],
                with: OpenWith::System,
            },
            OpenRule {
                extensions: vec!["md".into(), "txt".into()],
                with: OpenWith::Internal,
            },
        ];
        assert_eq!(
            matching_with(&rules, Path::new("/a/README.MD")),
            Some(OpenWith::System)
        );
        assert_eq!(
            matching_with(&rules, Path::new("/a/n.txt")),
            Some(OpenWith::Internal)
        );
        assert_eq!(matching_with(&rules, Path::new("/a/n.pdf")), None);
        assert_eq!(matching_with(&rules, Path::new("/a/README")), None);
    }
}
