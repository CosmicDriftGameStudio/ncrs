//! The paths of a recognize task, taken apart level by level, with the fields
//! of the document that each level shows. This only matches text; which
//! levels are folders, fixed words or placeholders is left to the model.

use ncrs_index::{DocumentFields, Kinds};
use ncrs_intake::{render_pattern_in, FILING_PLACEHOLDERS};

use super::verify::path_text;

fn value_of(name: &str, fields: &DocumentFields, kinds: &Kinds) -> Option<String> {
    render_pattern_in(&format!("{{{name}}}"), fields, kinds).map(|path| path_text(&path))
}

/// The automatic file name of the document, which has no extension here.
fn automatic_name(fields: &DocumentFields, kinds: &Kinds) -> Option<String> {
    let path = render_pattern_in("{year}/", fields, kinds)?;
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

fn without_extension(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, extension))
            if !extension.is_empty()
                && extension.len() <= 5
                && extension.chars().all(|ch| ch.is_ascii_alphanumeric()) =>
        {
            stem
        }
        _ => name,
    }
}

/// The spellings in which a value can show up in a file name.
fn spellings(value: &str) -> [String; 2] {
    [value.to_owned(), value.replace(' ', "-")]
}

fn note(level: &str, is_last: bool, fields: &DocumentFields, kinds: &Kinds) -> String {
    let stem = if is_last {
        without_extension(level)
    } else {
        level
    };
    if is_last && automatic_name(fields, kinds).as_deref() == Some(stem) {
        return " = the automatic file name".to_owned();
    }
    let values: Vec<(&str, [String; 2])> = FILING_PLACEHOLDERS
        .into_iter()
        .filter_map(|name| Some((name, spellings(&value_of(name, fields, kinds)?))))
        .collect();
    let equal: Vec<&str> = values
        .iter()
        .filter(|(_, spelled)| spelled.iter().any(|text| text == stem))
        .map(|(name, _)| *name)
        .collect();
    if !equal.is_empty() {
        return format!(" = {}", equal.join(", "));
    }
    let written = template(stem, &values);
    if written.contains('{') {
        format!(" reads \"{written}\"")
    } else {
        String::new()
    }
}

/// The level with every field value in it replaced by its placeholder, so
/// the text left outside the braces is what no field explains. A value only
/// counts as a whole word: "03" is no part of "0314".
fn template(stem: &str, values: &[(&str, [String; 2])]) -> String {
    let mut candidates: Vec<(&str, &str)> = values
        .iter()
        .flat_map(|(name, spelled)| spelled.iter().map(move |text| (*name, text.as_str())))
        .filter(|(_, text)| text.chars().count() >= 2)
        .collect();
    candidates.sort_by_key(|(_, text)| std::cmp::Reverse(text.len()));
    let mut written = String::new();
    let mut before: Option<char> = None;
    let mut rest = stem;
    while let Some(next) = rest.chars().next() {
        let found = candidates.iter().find_map(|(name, text)| {
            let after = rest.strip_prefix(text)?;
            let whole = before.is_none_or(|ch| !ch.is_alphanumeric())
                && after.chars().next().is_none_or(|ch| !ch.is_alphanumeric());
            whole.then_some((*name, after, text.chars().last()))
        });
        match found {
            Some((name, after, last)) => {
                written.push_str(&format!("{{{name}}}"));
                before = last;
                rest = after;
            }
            None => {
                written.push(next);
                before = Some(next);
                rest = &rest[next.len_utf8()..];
            }
        }
    }
    written
}

/// One block per path: the path, then each of its levels with the fields it
/// shows.
pub fn annotated_paths(files: &[(String, DocumentFields)], kinds: &Kinds) -> String {
    let mut blocks = Vec::new();
    for (number, (path, fields)) in files.iter().enumerate() {
        let levels: Vec<&str> = path.split('/').collect();
        let mut block = format!("PATH {}: {path}", number + 1);
        for (index, level) in levels.iter().enumerate() {
            let is_last = index + 1 == levels.len();
            let found = note(level, is_last, fields, kinds);
            block.push_str(&format!("\n  {} \"{level}\"{found}", index + 1));
        }
        blocks.push(block);
    }
    blocks.join("\n")
}

#[cfg(test)]
mod tests {
    use ncrs_index::KindLanguage;
    use ncrs_intake::parse_date;

    use super::*;

    fn otto() -> DocumentFields {
        DocumentFields {
            sender: Some("Otto GmbH".to_owned()),
            kind: Some("invoice".to_owned()),
            number: Some("5512".to_owned()),
            date: parse_date("2021-04-02"),
            ..DocumentFields::default()
        }
    }

    #[test]
    fn each_level_shows_the_fields_it_equals_or_contains() {
        let kinds = Kinds::built_in(KindLanguage::German);
        let path = "Post/2021/Otto GmbH/Rechnung_Otto-GmbH_2021-04-02.pdf".to_owned();
        let text = annotated_paths(&[(path, otto())], &kinds);
        assert_eq!(
            text,
            "PATH 1: Post/2021/Otto GmbH/Rechnung_Otto-GmbH_2021-04-02.pdf\n  1 \"Post\"\n  2 \"2021\" = year\n  3 \"Otto GmbH\" = sender\n  4 \"Rechnung_Otto-GmbH_2021-04-02.pdf\" reads \"{kind}_{sender}_{date}\""
        );
    }

    #[test]
    fn the_automatic_file_name_is_named() {
        let kinds = Kinds::built_in(KindLanguage::German);
        let path = "2021/2021-04-02_Otto-GmbH_5512.pdf".to_owned();
        let text = annotated_paths(&[(path, otto())], &kinds);
        assert!(text.ends_with("2 \"2021-04-02_Otto-GmbH_5512.pdf\" = the automatic file name"));
    }

    #[test]
    fn a_word_no_field_explains_stays_in_the_template() {
        let kinds = Kinds::built_in(KindLanguage::German);
        let path = "Bank/Beleg_2021-04-02_Otto-GmbH_Kopie.pdf".to_owned();
        let text = annotated_paths(&[(path, otto())], &kinds);
        assert!(
            text.ends_with("reads \"Beleg_{date}_{sender}_Kopie\""),
            "{text}"
        );
    }
}
