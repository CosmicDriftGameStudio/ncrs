use super::types::{
    Area, Branch, Level, LocationStructure, NamePart, NameScheme, Placeholder, PlaceholderLabel,
};

const CHOICE_SEPARATOR: &str = " · ";
const MISSING_NAME_SCHEME: &str = "…";

impl LocationStructure {
    /// One line per branch of every area, then one per unordered folder.
    pub fn describe(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .areas
            .iter()
            .flat_map(|area| area.branches.iter().map(|branch| branch_line(area, branch)))
            .collect();
        lines.extend(self.unordered.iter().map(|folder| {
            format!(
                "unordered: {}  {}",
                if folder.folder.is_empty() {
                    "(location root)"
                } else {
                    folder.folder.as_str()
                },
                folder.documents
            )
        }));
        lines
    }
}

fn branch_line(area: &Area, branch: &Branch) -> String {
    let mut parts = vec![area.folder.clone()];
    parts.extend(branch.levels.iter().map(level_text));
    parts.push(
        branch
            .name
            .as_ref()
            .map_or_else(|| MISSING_NAME_SCHEME.to_owned(), scheme_text),
    );
    format!(
        "{}  {}/{}",
        parts.join(" / "),
        branch.documents,
        branch.following
    )
}

fn level_text(level: &Level) -> String {
    match level {
        Level::Literal(name) => name.clone(),
        Level::Choice(names) => names.join(CHOICE_SEPARATOR),
        Level::Placeholder(placeholder) => placeholder_text(placeholder),
    }
}

fn placeholder_text(placeholder: &Placeholder) -> String {
    let counting = matches!(
        placeholder.label,
        PlaceholderLabel::Year
            | PlaceholderLabel::Month
            | PlaceholderLabel::Date
            | PlaceholderLabel::Number
    );
    if counting || placeholder.examples.is_empty() {
        format!("{{{}}}", label_text(placeholder.label))
    } else {
        format!(
            "{{{}: {}}}",
            label_text(placeholder.label),
            placeholder.examples.join(", ")
        )
    }
}

fn label_text(label: PlaceholderLabel) -> &'static str {
    match label {
        PlaceholderLabel::Year => "Year",
        PlaceholderLabel::Month => "Month",
        PlaceholderLabel::Date => "Date",
        PlaceholderLabel::Number => "Number",
        PlaceholderLabel::Sender => "Sender",
        PlaceholderLabel::Kind => "Kind",
        PlaceholderLabel::Bank => "Bank",
        PlaceholderLabel::Insurer => "Insurer",
        PlaceholderLabel::Vehicle => "Vehicle",
        PlaceholderLabel::Child => "Child",
        PlaceholderLabel::Property => "Property",
        PlaceholderLabel::Subfolder => "Subfolder",
    }
}

fn scheme_text(scheme: &NameScheme) -> String {
    scheme
        .parts
        .iter()
        .map(|part| match part {
            NamePart::Literal(word) => word.clone(),
            NamePart::Separator(character) => character.to_string(),
            NamePart::Date(_) => "{Date}".to_owned(),
            NamePart::Year => "{Year}".to_owned(),
            NamePart::Number => "{Number}".to_owned(),
            NamePart::Free => "{Free}".to_owned(),
        })
        .collect()
}
