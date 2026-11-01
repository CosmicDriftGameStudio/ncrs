//! The `[open]` programs of the config file, which `open` starts.

use std::path::{Component, Path};

use serde::de::{Deserializer, Error as _};
use serde::Deserialize;

/// The `[open]` section: the programs behind F3 and F4. A missing one means
/// the system default of the platform.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenPrograms {
    #[serde(default)]
    pub view: Option<ProgramLine>,
    #[serde(default)]
    pub edit: Option<ProgramLine>,
}

/// Program plus leading arguments. `{path}` and `{line}` in an argument are
/// replaced by the file and the line to jump to; only when no argument has
/// `{path}` is the file appended as the last argument (see `open_command`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramLine {
    program: String,
    args: Vec<String>,
}

impl ProgramLine {
    /// Not validated: for the built-in defaults and tests. The file goes
    /// through `Deserialize`, which checks.
    pub fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        }
    }

    pub fn program(&self) -> &str {
        &self.program
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// The program, then the arguments: the array `[open]` holds.
    pub fn words(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.program.as_str()).chain(self.args.iter().map(String::as_str))
    }

    /// The line as the settings tab shows it: words split at spaces, a word
    /// with a space or a quote in quotes. There are no escapes: a word with
    /// both kinds of quote is written as quoted runs next to each other,
    /// which [`Self::from_text`] joins back into the one word.
    pub fn to_text(&self) -> String {
        self.words().map(quoted_word).collect::<Vec<_>>().join(" ")
    }

    /// The line typed in the settings tab, read as [`Self::to_text`] writes it.
    pub fn from_text(line: &str) -> Result<Self, ProgramLineProblem> {
        Self::parse(split_words(line)?)
    }

    /// Opens a file with the app bundle at `app` through `open -a`, the one
    /// way the sandboxed macOS build may start another program.
    pub fn opening_app(app: &str) -> Self {
        Self::new("open", &["-a", app])
    }

    fn parse(mut words: Vec<String>) -> Result<Self, ProgramLineProblem> {
        if words.is_empty() {
            return Err(ProgramLineProblem::NoProgram);
        }
        let program = words.remove(0);
        if program.trim().is_empty() {
            return Err(ProgramLineProblem::EmptyProgram);
        }
        if program.contains('\0') || words.iter().any(|arg| arg.contains('\0')) {
            return Err(ProgramLineProblem::Nul);
        }
        if depends_on_working_directory(&program) {
            return Err(ProgramLineProblem::RelativePath);
        }
        Ok(Self {
            program,
            args: words,
        })
    }
}

/// Why a program line cannot be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramLineProblem {
    NoProgram,
    EmptyProgram,
    Nul,
    RelativePath,
    /// Only typed text has quotes.
    UnclosedQuote,
}

impl ProgramLineProblem {
    /// The reason in a config error, which is not translated.
    pub fn reason(self) -> &'static str {
        match self {
            Self::NoProgram => "needs at least the program",
            Self::EmptyProgram => "the program is empty",
            Self::Nul => "contains a NUL character",
            Self::RelativePath => "use a program name from PATH or an absolute path",
            Self::UnclosedQuote => "a quote is not closed",
        }
    }
}

const QUOTES: [char; 2] = ['"', '\''];

fn quoted_word(word: &str) -> String {
    let needs_quotes =
        word.is_empty() || word.contains(char::is_whitespace) || word.contains(QUOTES);
    if !needs_quotes {
        return word.to_owned();
    }
    if !word.contains('"') {
        return format!("\"{word}\"");
    }
    if !word.contains('\'') {
        return format!("'{word}'");
    }
    let mut quoted = String::new();
    let mut run_quote: Option<char> = None;
    for c in word.chars() {
        let quote = if c == '"' { '\'' } else { '"' };
        if run_quote != Some(quote) {
            quoted.extend(run_quote);
            quoted.push(quote);
            run_quote = Some(quote);
        }
        quoted.push(c);
    }
    quoted.extend(run_quote);
    quoted
}

/// Words split at whitespace; a run in double or single quotes is one word
/// without its quotes.
fn split_words(line: &str) -> Result<Vec<String>, ProgramLineProblem> {
    let mut words = Vec::new();
    let mut current: Option<String> = None;
    let mut open_quote: Option<char> = None;
    for c in line.chars() {
        match open_quote {
            Some(quote) if c == quote => open_quote = None,
            Some(_) => current.get_or_insert_with(String::new).push(c),
            None if QUOTES.contains(&c) => {
                open_quote = Some(c);
                current.get_or_insert_with(String::new);
            }
            None if c.is_whitespace() => words.extend(current.take()),
            None => current.get_or_insert_with(String::new).push(c),
        }
    }
    if open_quote.is_some() {
        return Err(ProgramLineProblem::UnclosedQuote);
    }
    words.extend(current);
    Ok(words)
}

/// A relative program with a separator, or on Windows a drive-relative one
/// such as `C:edit.exe` or `\tools\edit.exe`, would depend on the directory
/// ncrs happened to start in.
fn depends_on_working_directory(program: &str) -> bool {
    let path = Path::new(program);
    let anchored_elsewhere = path
        .components()
        .any(|part| matches!(part, Component::Prefix(_) | Component::RootDir));
    !path.is_absolute() && (program.contains(is_separator) || anchored_elsewhere)
}

pub fn is_separator(c: char) -> bool {
    c == '/' || (cfg!(windows) && c == '\\')
}

impl<'de> Deserialize<'de> for ProgramLine {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let words = Vec::<String>::deserialize(deserializer)?;
        Self::parse(words).map_err(|problem| D::Error::custom(problem.reason()))
    }
}
