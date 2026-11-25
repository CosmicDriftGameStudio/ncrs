use crate::classify::{DOCUMENT_EXTENSIONS, IMAGE_EXTENSIONS};

/// `name` with the ending of the file itself. A known document ending at the
/// end of `name` is only how the pattern was written and is replaced; any
/// other dot (`Nr. 5`, `v1.2`) belongs to the name. A file without an ending
/// gets none.
pub fn file_name_with_extension(name: &str, extension: Option<&str>) -> String {
    let stem = match name.rsplit_once('.') {
        Some((stem, written)) if !stem.is_empty() && is_known_extension(written) => stem,
        _ => name,
    };
    match extension
        .map(|ending| ending.trim_start_matches('.'))
        .filter(|ending| !ending.is_empty())
    {
        Some(ending) => format!("{stem}.{ending}"),
        None => stem.to_owned(),
    }
}

fn is_known_extension(written: &str) -> bool {
    let lowered = written.to_ascii_lowercase();
    DOCUMENT_EXTENSIONS
        .iter()
        .chain(IMAGE_EXTENSIONS.iter())
        .any(|known| *known == lowered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extension_of_the_file_wins_over_the_written_one() {
        let cases = [
            ("Bill_4711", Some("pdf"), "Bill_4711.pdf"),
            ("Bill_4711.pdf", Some("pdf"), "Bill_4711.pdf"),
            ("Bill_4711.pdf", Some("PDF"), "Bill_4711.PDF"),
            ("Bill_4711.PDF", Some("pdf"), "Bill_4711.pdf"),
            ("Bill_4711", Some("jpg"), "Bill_4711.jpg"),
            ("Bill_4711.pdf", Some("jpg"), "Bill_4711.jpg"),
            ("Bill_4711.docx", Some("pdf"), "Bill_4711.pdf"),
            ("Nr. 5", Some("pdf"), "Nr. 5.pdf"),
            ("Bill v1.2", Some("pdf"), "Bill v1.2.pdf"),
            ("Bill_4711.pdf", None, "Bill_4711"),
            ("Bill_4711", None, "Bill_4711"),
        ];
        for (name, extension, expected) in cases {
            assert_eq!(
                file_name_with_extension(name, extension),
                expected,
                "{name}"
            );
        }
    }
}
