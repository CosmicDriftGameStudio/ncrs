//! What a path pasted into the path field may look like before it is a path:
//! quoted (Explorer's "Copy as path"), starting with `~`, or holding
//! variables.

use std::path::Path;

use crate::platform::Platform;

/// The text without one pair of quotes around it, and without the space
/// around that.
pub fn strip_quotes(typed: &str) -> &str {
    let trimmed = typed.trim();
    for quote in ['"', '\''] {
        if let Some(inner) = trimmed
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner.trim();
        }
    }
    trimmed
}

/// `typed` with a leading `~` replaced by `home` and the variables replaced by
/// what `lookup` knows: `%NAME%` on Windows, `$NAME` and `${NAME}` elsewhere.
/// A variable `lookup` does not know stays as typed.
pub fn expand(
    typed: &str,
    platform: Platform,
    home: &Path,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> String {
    let after_tilde = match typed.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            format!("{}{rest}", home.display())
        }
        _ => typed.to_owned(),
    };
    match platform {
        Platform::Windows => expand_percent(&after_tilde, lookup),
        Platform::Mac | Platform::Unix => expand_dollar(&after_tilde, lookup),
    }
}

fn expand_percent(text: &str, lookup: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('%') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let known = after
            .find('%')
            .filter(|&close| close > 0)
            .and_then(|close| lookup(&after[..close]).map(|value| (value, close)));
        match known {
            Some((value, close)) => {
                out.push_str(&value);
                rest = &after[close + 1..];
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn is_name_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn expand_dollar(text: &str, lookup: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('$') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let (name, length) = match after.strip_prefix('{') {
            Some(braced) => match braced.find('}') {
                Some(close) => (&braced[..close], close + 2),
                None => ("", 0),
            },
            None => {
                let end = after
                    .find(|character: char| !is_name_character(character))
                    .unwrap_or(after.len());
                (&after[..end], end)
            }
        };
        let starts_like_a_name = name
            .chars()
            .next()
            .is_some_and(|first| !first.is_ascii_digit());
        match lookup(name).filter(|_| starts_like_a_name) {
            Some(value) => {
                out.push_str(&value);
                rest = &after[length..];
            }
            None => {
                out.push('$');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment(name: &str) -> Option<String> {
        match name {
            "HOME" => Some("/home/m".to_owned()),
            "USERPROFILE" => Some(r"C:\Users\m".to_owned()),
            "ProgramFiles(x86)" => Some(r"C:\Program Files (x86)".to_owned()),
            _ => None,
        }
    }

    fn unix(typed: &str) -> String {
        expand(typed, Platform::Unix, Path::new("/home/m"), &environment)
    }

    fn windows(typed: &str) -> String {
        expand(
            typed,
            Platform::Windows,
            Path::new(r"C:\Users\m"),
            &environment,
        )
    }

    #[test]
    fn quotes_around_the_path_go_and_quotes_inside_stay() {
        assert_eq!(
            strip_quotes(r#"  "C:\Users\m\My Docs"  "#),
            r"C:\Users\m\My Docs"
        );
        assert_eq!(strip_quotes("'/tmp/a b'"), "/tmp/a b");
        assert_eq!(strip_quotes("/tmp/it's"), "/tmp/it's");
        assert_eq!(strip_quotes(r#""/tmp/open"#), r#""/tmp/open"#);
        assert_eq!(strip_quotes(r#""""#), "");
    }

    #[test]
    fn a_tilde_is_the_home_folder_only_at_the_start_of_a_name() {
        assert_eq!(unix("~"), "/home/m");
        assert_eq!(unix("~/docs"), "/home/m/docs");
        assert_eq!(unix("~other/docs"), "~other/docs");
        assert_eq!(unix("/tmp/~"), "/tmp/~");
        assert_eq!(windows(r"~\docs"), r"C:\Users\m\docs");
    }

    #[test]
    fn dollar_variables_are_known_ones_with_or_without_braces() {
        assert_eq!(unix("$HOME/docs"), "/home/m/docs");
        assert_eq!(unix("${HOME}docs"), "/home/mdocs");
        assert_eq!(unix("$NOPE/docs"), "$NOPE/docs");
        assert_eq!(unix("/tmp/$1"), "/tmp/$1");
        assert_eq!(unix("/tmp/cost$"), "/tmp/cost$");
        assert_eq!(unix("${HOME"), "${HOME");
    }

    #[test]
    fn percent_variables_are_for_windows_and_a_dollar_is_a_letter_there() {
        assert_eq!(windows(r"%USERPROFILE%\Downloads"), r"C:\Users\m\Downloads");
        assert_eq!(
            windows(r"%ProgramFiles(x86)%\App"),
            r"C:\Program Files (x86)\App"
        );
        assert_eq!(windows(r"%NOPE%\x"), r"%NOPE%\x");
        assert_eq!(windows(r"C:\100%\x"), r"C:\100%\x");
        assert_eq!(windows(r"\\server\share\$HOME"), r"\\server\share\$HOME");
        assert_eq!(unix("%USERPROFILE%"), "%USERPROFILE%");
    }
}
