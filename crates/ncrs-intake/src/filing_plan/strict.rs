//! The rendering `decide` uses: every placeholder needs a value, nothing is
//! left out, and each finished segment is checked as the file system would
//! read it.

use super::*;
use crate::sorting::file_name_with_extension;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unrenderable {
    /// A placeholder has no value, or only one a folder may not take.
    MissingValue(Field),
    /// The pattern does not parse, or a filled-in segment is not a name every
    /// system keeps as it is.
    BadName,
}

/// The relative path `source` gives, ending in the extension of the file.
/// No folder takes a value of `refused_in_folders`.
pub(crate) fn render_strict(
    source: &str,
    fields: &DocumentFields,
    refused_in_folders: &[Field],
    kinds: &Kinds,
    extension: Option<&str>,
) -> Result<PathBuf, Unrenderable> {
    let pattern = Pattern::parse(source).ok_or(Unrenderable::BadName)?;
    let mut segments = pattern
        .folders()
        .iter()
        .map(|tokens| fill(tokens, fields, refused_in_folders, kinds))
        .collect::<Result<Vec<_>, _>>()?;
    let file_name = match pattern.explicit_file_name() {
        Some(tokens) => fill(tokens, fields, &[], kinds)?,
        None => automatic_file_name(fields).ok_or(Unrenderable::MissingValue(Field::Date))?,
    };
    segments.push(file_name_with_extension(&file_name, extension));
    if !segments.iter().all(|segment| is_keepable_name(segment)) {
        return Err(Unrenderable::BadName);
    }
    let path: PathBuf = segments.iter().collect();
    stays_below(&path)
        .then_some(path)
        .ok_or(Unrenderable::BadName)
}

fn fill(
    tokens: &[Token],
    fields: &DocumentFields,
    refused: &[Field],
    kinds: &Kinds,
) -> Result<String, Unrenderable> {
    let mut segment = String::new();
    for token in tokens {
        match token {
            Token::Literal(literal) => segment.push_str(literal),
            Token::Placeholder(placeholder) => {
                let field = placeholder.field();
                let value = placeholder
                    .value(fields, Some(kinds))
                    .filter(|_| !refused.contains(&field))
                    .ok_or(Unrenderable::MissingValue(field))?;
                segment.push_str(&value);
            }
        }
    }
    Ok(segment)
}

fn is_keepable_name(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= MAX_COMPONENT_BYTES
        && !has_lost_ending(segment)
        && !is_device_name(segment)
}
