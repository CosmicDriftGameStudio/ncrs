//! The rows of "F3 by file type" on the programs page: what is typed and
//! chosen there, before it is a rule of the config.

use std::path::{Path, PathBuf};

use ncrs_config::config::{parse_extensions, OpenRule, OpenWith, ProgramLine};

use crate::message::SettingChange;
use crate::settings::state::{FieldProblem, SettingsState};

/// How a row opens its files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WithKind {
    Internal,
    System,
    Program,
}

/// One row as the user sees it: texts that may not be a valid rule yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRow {
    pub extensions: String,
    pub with: WithKind,
    pub program: String,
    pub problem: Option<FieldProblem>,
}

impl TypeRow {
    pub fn empty() -> Self {
        Self {
            extensions: String::new(),
            with: WithKind::Internal,
            program: String::new(),
            problem: None,
        }
    }

    pub fn from_rule(rule: &OpenRule) -> Self {
        let (with, program) = match &rule.with {
            OpenWith::Internal => (WithKind::Internal, String::new()),
            OpenWith::System => (WithKind::System, String::new()),
            OpenWith::Program(line) => (WithKind::Program, line.to_text()),
        };
        Self {
            extensions: rule.extensions.join(", "),
            with,
            program,
            problem: None,
        }
    }

    /// The rule the row stands for, or why it is not one yet.
    pub fn rule(&self) -> Result<OpenRule, FieldProblem> {
        let extensions = parse_extensions(&self.extensions);
        if extensions.is_empty() {
            return Err(FieldProblem::NoExtension);
        }
        let with = match self.with {
            WithKind::Internal => OpenWith::Internal,
            WithKind::System => OpenWith::System,
            WithKind::Program => {
                if self.program.trim().is_empty() {
                    return Err(FieldProblem::NoProgram);
                }
                OpenWith::Program(
                    ProgramLine::from_text(&self.program).map_err(FieldProblem::Program)?,
                )
            }
        };
        Ok(OpenRule { extensions, with })
    }
}

pub fn rows_of(rules: &[OpenRule]) -> Vec<TypeRow> {
    rules.iter().map(TypeRow::from_rule).collect()
}

/// Everything the rows of the block send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeRuleEdit {
    /// Typing in the extensions field; written with Enter.
    Extensions {
        row: usize,
        text: String,
    },
    /// Typing in the program field; written with Enter.
    Program {
        row: usize,
        text: String,
    },
    /// Enter in one of the fields: writes the rows.
    Commit(usize),
    With {
        row: usize,
        with: WithKind,
    },
    /// The button that opens the system file dialog for the program.
    Pick(usize),
    Picked {
        row: usize,
        path: Option<PathBuf>,
    },
    Remove(usize),
    Add,
}

/// A picked program file: an app bundle opens through `open -a`, any other
/// file is the program itself, which the App Store build refuses.
pub fn picked_program_line(path: &Path, sandboxed: bool) -> Result<ProgramLine, FieldProblem> {
    let text = path.to_str().ok_or(FieldProblem::PathNotText)?;
    let is_app = cfg!(target_os = "macos")
        && path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("app"));
    if is_app {
        Ok(ProgramLine::opening_app(text))
    } else if sandboxed {
        Err(FieldProblem::OnlyApps)
    } else {
        Ok(ProgramLine::new(text, &[]))
    }
}

impl SettingsState {
    /// The edit applied to the rows; the change to write when the rows that
    /// are valid differ from the file.
    pub fn edit_type_rule(&mut self, edit: TypeRuleEdit) -> Option<SettingChange> {
        match edit {
            TypeRuleEdit::Extensions { row, text } => {
                if let Some(each) = self.type_rows.get_mut(row) {
                    each.extensions = text;
                    each.problem = None;
                }
                None
            }
            TypeRuleEdit::Program { row, text } => {
                if let Some(each) = self.type_rows.get_mut(row) {
                    each.program = text;
                    each.problem = None;
                }
                None
            }
            TypeRuleEdit::Commit(row) => self.commit_type_rows(Some(row)),
            TypeRuleEdit::With { row, with } => {
                if let Some(each) = self.type_rows.get_mut(row) {
                    each.with = with;
                    each.problem = None;
                }
                self.commit_type_rows(None)
            }
            TypeRuleEdit::Pick(_) | TypeRuleEdit::Picked { path: None, .. } => None,
            TypeRuleEdit::Picked {
                row,
                path: Some(path),
            } => {
                let sandboxed = self.sandboxed;
                let each = self.type_rows.get_mut(row)?;
                match picked_program_line(&path, sandboxed) {
                    Ok(line) => {
                        each.program = line.to_text();
                        each.with = WithKind::Program;
                        each.problem = None;
                        self.commit_type_rows(Some(row))
                    }
                    Err(problem) => {
                        each.problem = Some(problem);
                        None
                    }
                }
            }
            TypeRuleEdit::Remove(row) => {
                if row < self.type_rows.len() {
                    self.type_rows.remove(row);
                }
                self.commit_type_rows(None)
            }
            TypeRuleEdit::Add => {
                self.type_rows.push(TypeRow::empty());
                None
            }
        }
    }

    /// The rows that are valid, in order; a row that is not stays as a draft,
    /// and tells why when it is the one `shown` that was just entered.
    fn commit_type_rows(&mut self, shown: Option<usize>) -> Option<SettingChange> {
        let mut rules = Vec::new();
        for (index, each) in self.type_rows.iter_mut().enumerate() {
            match each.rule() {
                Ok(rule) => rules.push(rule),
                Err(problem) if shown == Some(index) => each.problem = Some(problem),
                Err(_) => {}
            }
        }
        let saved = self
            .config()
            .map(|config| config.open.type_rules())
            .unwrap_or_default();
        (rules != saved).then_some(SettingChange::OpenRules(rules))
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#[cfg(test)]
mod tests {
    use super::*;

    fn rule(extensions: &[&str], with: OpenWith) -> OpenRule {
        OpenRule {
            extensions: extensions.iter().map(|each| (*each).to_owned()).collect(),
            with,
        }
    }

    #[test]
    fn a_row_is_a_rule_only_when_it_is_complete() {
        let mut row = TypeRow::empty();
        assert_eq!(row.rule(), Err(FieldProblem::NoExtension));
        row.extensions = "PDF, .Png".to_owned();
        assert_eq!(row.rule(), Ok(rule(&["pdf", "png"], OpenWith::Internal)));
        row.with = WithKind::Program;
        assert_eq!(row.rule(), Err(FieldProblem::NoProgram));
        row.program = "open -a Preview {path}".to_owned();
        assert_eq!(
            row.rule(),
            Ok(rule(
                &["pdf", "png"],
                OpenWith::Program(ProgramLine::new("open", &["-a", "Preview", "{path}"]))
            ))
        );
    }

    #[test]
    fn a_rule_comes_back_as_the_row_it_was() {
        let original = rule(
            &["md", "markdown"],
            OpenWith::Program(ProgramLine::new("typora", &[])),
        );
        assert_eq!(TypeRow::from_rule(&original).rule(), Ok(original));
    }
}
